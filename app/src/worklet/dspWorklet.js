/**
 * `dsp-processor` — the AudioWorklet half of the web port.
 *
 * The wasm module is compiled on the main thread and handed over through
 * `processorOptions`, because `AudioWorkletGlobalScope` has no `fetch` and
 * cannot compile WASM itself. Everything else — the DSP — is the Rust from
 * `app/src-tauri/src/audio/`, compiled to WASM by `app/wasm-dsp`.
 *
 * This file is bundled by `scripts/build-worklet.mjs` into a single
 * self-contained `public/wasm/dspWorklet.js`. That is a requirement, not an
 * optimization: `AudioWorkletGlobalScope` does not reliably support static
 * `import`. A module that merely *has* an import resolves `addModule()`
 * successfully while never running its top-level statements, so
 * `registerProcessor` never executes, the processor name is never defined, and
 * `new AudioWorkletNode` throws `InvalidStateError`. Bundling to a single
 * import-free file sidesteps the problem entirely.
 *
 * `dsp-glue` is an esbuild alias for the wasm-bindgen glue that `wasm-pack`
 * generates beside the `.wasm` binary; it is inlined at build time.
 *
 * Note the import shape: `initSync` must be taken as a **named** import. The
 * glue's default export is `__wbg_init`, the *async* initializer. Bundling
 * `import initSync from "dsp-glue"` therefore silently wires up the async path,
 * which returns a promise — the worklet constructor runs straight past it, the
 * WASM instance is never assigned, and the first DSP call dies on
 * "Cannot read properties of undefined (reading 'dspprocessor_new')".
 *
 * The processor reads its audio from a decoded track buffer (sent over the
 * port by the main thread), not from its input node, so the node is created
 * with no inputs and the source position is tracked inside the DSP exactly the
 * way the desktop engine tracks it in `SharedPlay::play_pos`.
 */

import "./polyfill.js";
import { initSync, DspProcessor } from "dsp-glue";

/** Frames per render quantum — fixed by the Web Audio spec. */
const QUANTUM = 128;
const SPECTRUM_BINS = 48;
const WAVEFORM_POINTS = 226;
/** How often to report position and taps to the main thread (~30 fps). */
const REPORT_INTERVAL_MS = 33;

class DspProcessorNode extends AudioWorkletProcessor {
  constructor(options) {
    super();
    const opts = options.processorOptions || {};

    if (!opts.wasmModule) {
      throw new Error("dsp-processor: processorOptions.wasmModule is required");
    }
    initSync({ module: opts.wasmModule });

    this.processor = new DspProcessor(sampleRate);
    this.hasTrack = false;

    this.lastReport = 0;
    this.lastPos = 0;
    this.reportedEnded = false;

    this.port.onmessage = (ev) => this.handleMessage(ev.data);
    // The main thread needs to know the worklet booted at all, and at what
    // rate — a silent failure here is otherwise invisible.
    this.port.postMessage({
      type: "ready",
      sampleRate,
      spectrumBins: SPECTRUM_BINS,
      waveformPoints: WAVEFORM_POINTS,
    });
  }

  handleMessage(msg) {
    switch (msg.type) {
      case "track": {
        // `samples` arrives as a Float32Array transferred from the main
        // thread. Interleaved L,R at the context sample rate — decodeAudioData
        // has already resampled, which is why the worklet needs no resampler.
        this.processor.load_track(msg.samples, msg.channels);
        this.hasTrack = true;
        this.reportedEnded = false;
        this.lastPos = 0;
        this.port.postMessage({ type: "loaded", duration: this.processor.duration_secs() });
        break;
      }
      case "params":
        this.processor.set_params(
          msg.cutoff,
          msg.speed,
          msg.pitch,
          msg.reverb,
          msg.eq,
        );
        break;
      case "play":
        this.processor.set_playing(true);
        this.reportedEnded = false;
        break;
      case "pause":
      case "stop":
        this.processor.set_playing(false);
        break;
      case "seek":
        this.processor.seek_secs(msg.seconds);
        // A seek moves the playhead discontinuously; force the next report to
        // include the new position so the UI snaps instead of animating.
        this.lastPos = -1;
        break;
      default:
        break;
    }
  }

  /** Position + visualizer taps, throttled to REPORT_INTERVAL_MS. */
  maybeReport(now) {
    if (now - this.lastReport < REPORT_INTERVAL_MS) return;
    this.lastReport = now;

    if (this.processor.take_ended()) {
      this.reportedEnded = true;
      this.port.postMessage({ type: "ended" });
    }

    if (!this.hasTrack || !this.processor.is_playing()) return;

    const pos = this.processor.position_secs();
    // A jump larger than a second is a seek, not playback; still report it,
    // but the main thread uses the flag to snap rather than animate.
    this.port.postMessage({ type: "position", position: pos, seek: this.lastPos < 0 });
    this.lastPos = pos;

    // Both taps are returned by value. Passing a destination array into wasm
    // does not work: wasm-bindgen treats `&mut [f32]` as a return pointer and
    // the generated JS never copies the result back, so the arrays here would
    // stay permanently zero and the visualizer would never light up.
    this.port.postMessage({
      type: "taps",
      spectrum: this.processor.spectrum(),
      waveform: this.processor.waveform(),
    });
  }

  process(_inputs, outputs) {
    const output = outputs[0];
    if (!output || output.length === 0) return true;

    const left = output[0];
    const right = output.length > 1 ? output[1] : output[0];
    const frames = left.length;

    // Interleaved L,R straight out of the DSP. `process()` returns the samples
    // rather than writing into a caller-supplied array — see the note on the
    // wasm binding; an out-parameter would leave this stale.
    const pcm = this.processor.process();
    const n = Math.min(frames * 2, pcm.length);

    for (let i = 0; i < frames; i++) {
      const l = i * 2 < n ? pcm[i * 2] : 0;
      const r = i * 2 + 1 < n ? pcm[i * 2 + 1] : 0;
      left[i] = l;
      right[i] = r;
    }

    this.maybeReport(currentTime * 1000);
    return true;
  }
}

registerProcessor("dsp-processor", DspProcessorNode);
