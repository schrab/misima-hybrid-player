/**
 * Web audio engine: `AudioContext` + decoded-track playlist + the
 * `dsp-processor` worklet.
 *
 * Three things differ from the desktop engine and are worth stating up front:
 *
 * 1. **Decoding is the browser's job.** `decodeAudioData` handles mp3/flac/
 *    wav/ogg and, crucially, resamples to `AudioContext.sampleRate` — which is
 *    why the worklet has no resampler and why the DSP can assume source rate
 *    equals device rate (the desktop reaches the same state via
 *    `resample_interleaved`).
 *
 * 2. **Whole files live in memory.** ~115 MB for a 5-minute stereo track at
 *    48 kHz. That is the price of random access into the WSOLA; see §4.4 of
 *    the spec.
 *
 * 3. **The context starts suspended.** Autoplay policy forbids audio before a
 *    user gesture, so the context is created lazily on the first play or
 *    file-drop rather than at page load.
 */

import type { AudioParamsInput, PlaylistRow } from "../transport";
import { assetUrl } from "./base";

/** Formats the picker offers. Safari has no OGG Vorbis; see spec §9.5. */
const AUDIO_ACCEPT = ".mp3,.flac,.wav,.ogg,.m4a,.aac,.opus";

type TapHandler = (spectrum: Float32Array, waveform: Float32Array) => void;

export class WebPlayer {
  private ctx: AudioContext | null = null;
  private node: AudioWorkletNode | null = null;
  /**
   * Safety mute between the worklet and the destination. The DSP already
   * applies master volume, but a stuck `playing` flag should never produce
   * sound that no fader can stop.
   */
  private gain: GainNode | null = null;

  /** Decoded tracks by playlist row id. */
  private tracks = new Map<number, { buffer: AudioBuffer; title: string }>();
  private rows: PlaylistRow[] = [];
  private nextId = 1;
  private activeId: number | null = null;

  private playing = false;
  private position = 0;
  private params: AudioParamsInput = {
    volume: 1.0,
    speed: 1,
    pitch: 0,
    reverb: 0,
    eq: new Array(10).fill(0),
  };

  private onTap: TapHandler | null = null;
  private onEnded: (() => void) | null = null;
  private onPosition: ((pos: number, seek: boolean) => void) | null = null;
  private onError: ((msg: string) => void) | null = null;
  private onTrackChanged: ((id: number) => void) | null = null;
  private loading = false;

  // ---------------------------------------------------------------- context

  /**
   * A bare `AudioContext`, created on demand. All that decoding needs — and
   * deliberately *not* the worklet.
   *
   * Decoding and playback are separate concerns: a failure to bring up the
   * DSP worklet must not stop the user from building a playlist. (It did,
   * once, when `openFiles` awaited the whole engine and rejected.)
   *
   * Must be reachable from a user gesture — Safari refuses a context
   * constructed outside one.
   */
  private async ensureContext(): Promise<AudioContext> {
    if (this.ctx) {
      if (this.ctx.state === "suspended") await this.ctx.resume();
      return this.ctx;
    }
    const ctx = new AudioContext({ latencyHint: "interactive" });
    this.ctx = ctx;
    return ctx;
  }

  /**
   * Bring up the worklet: compile the WASM, register the processor, wire it to
   * the destination. Separate from `ensureContext` so decoding never depends
   * on it, and memoized so a failed attempt is retried on the next play.
   */
  private async ensureEngine(): Promise<AudioWorkletNode> {
    const ctx = await this.ensureContext();
    if (this.node) return this.node;

    // The worklet global scope cannot fetch, so the main thread compiles the
    // module and passes the compiled `WebAssembly.Module` through
    // `processorOptions` (spec §9.1).
    const wasmUrl = assetUrl("wasm/misima_wasm_dsp_bg.wasm");
    const workletUrl = assetUrl("wasm/dspWorklet.js");
    const [wasmResponse] = await Promise.all([
      fetch(wasmUrl),
      ctx.audioWorklet.addModule(workletUrl),
    ]);
    if (!wasmResponse.ok) {
      throw new Error(`wasm fetch failed: ${wasmResponse.status}`);
    }
    // compileStreaming needs the right MIME type; GitHub Pages serves wasm
    // correctly, but a plain static host may not, so fall back to ArrayBuffer.
    let wasmModule: WebAssembly.Module;
    try {
      wasmModule = await WebAssembly.compileStreaming(wasmResponse.clone());
    } catch {
      const bytes = await wasmResponse.arrayBuffer();
      wasmModule = await WebAssembly.compile(bytes);
    }

    const node = new AudioWorkletNode(ctx, "dsp-processor", {
      numberOfInputs: 0,
      numberOfOutputs: 1,
      outputChannelCount: [2],
      processorOptions: { wasmModule },
    });

    // The worklet posts "ready" from its constructor. If that never arrives,
    // something threw during its module evaluation and the DSP is dead —
    // `addModule()` resolving is *not* proof the processor registered, and the
    // only symptom otherwise is silence. Fail loudly instead.
    const ready = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error("dsp worklet did not start")),
        5000,
      );
      node.port.onmessage = (ev) => {
        const msg = ev.data as { type?: string };
        if (msg?.type === "ready") {
          clearTimeout(timer);
          resolve();
        }
        this.handleWorkletMessage(ev.data);
      };
      node.onprocessorerror = () => {
        clearTimeout(timer);
        reject(new Error("dsp worklet crashed"));
      };
    });

    // A safety mute on the far side of the worklet. The DSP already applies
    // master volume, but a stuck `playing` flag should never produce sound
    // that no fader can stop.
    const gain = ctx.createGain();
    gain.gain.value = 1;
    node.connect(gain).connect(ctx.destination);

    try {
      await ready;
    } catch (err) {
      // Leave `this.node` unset so the next play retries instead of reusing a
      // dead processor.
      node.disconnect();
      throw err;
    }

    this.node = node;
    this.gain = gain;
    return node;
  }

  private handleWorkletMessage(msg: Record<string, unknown>) {
    switch (msg.type) {
      case "ready":
        this.pushParams();
        break;
      case "loaded":
        this.onPosition?.(this.position, true);
        break;
      case "position":
        this.position = msg.position as number;
        this.onPosition?.(this.position, Boolean(msg.seek));
        break;
      case "taps":
        this.onTap?.(msg.spectrum as Float32Array, msg.waveform as Float32Array);
        break;
      case "ended":
        this.playing = false;
        this.onEnded?.();
        break;
      default:
        break;
    }
  }

  // --------------------------------------------------------------- callbacks

  setCallbacks(cb: {
    tap?: TapHandler;
    ended?: () => void;
    position?: (pos: number, seek: boolean) => void;
    error?: (msg: string) => void;
    trackChanged?: (id: number) => void;
  }) {
    this.onTap = cb.tap ?? null;
    this.onEnded = cb.ended ?? null;
    this.onPosition = cb.position ?? null;
    this.onError = cb.error ?? null;
    this.onTrackChanged = cb.trackChanged ?? null;
  }

  // ---------------------------------------------------------------- playlist

  async openFiles(files: File[]): Promise<void> {
    if (files.length === 0) return;
    // Built up front so a user gesture is still in progress — Safari needs the
    // context constructed inside the handler.
    await this.ensureContext();
    this.loading = true;
    try {
      for (const file of files) {
        try {
          const bytes = await file.arrayBuffer();
          const buffer = await this.ctx!.decodeAudioData(bytes);
          const id = this.nextId++;
          this.tracks.set(id, { buffer, title: file.name });
          this.rows.push({ id, title: file.name, duration: fmtDur(buffer.duration) });
        } catch {
          this.onError?.(`Cannot decode ${file.name}`);
        }
      }
    } finally {
      this.loading = false;
    }
  }

  /** Fetch and decode a hosted track. Same pipeline as a local file. */
  async addUrl(url: string, title?: string): Promise<void> {
    await this.ensureContext();
    const name = title ?? url.split("/").pop() ?? url;
    try {
      const res = await fetch(url);
      if (!res.ok) throw new Error(String(res.status));
      const buffer = await this.ctx!.decodeAudioData(await res.arrayBuffer());
      const id = this.registerRemote(url, name);
      this.tracks.set(id, { buffer, title: name });
      this.setDuration(id, buffer.duration);
    } catch {
      this.onError?.(`Cannot load ${name}`);
    }
  }

  /**
   * Add a playlist row for a remote track *without* fetching it.
   *
   * The bundled startup set is four tracks totalling ~28 MB, and decoding all
   * of them on arrival would make every page load pay for the whole library.
   * Rows registered here carry a `sourceUrl` and are fetched by `playIndex`
   * when the listener actually reaches them.
   */
  registerRemote(url: string, title: string): number {
    const existing = this.rows.find((r) => r.sourceUrl === url);
    if (existing) return existing.id;
    const id = this.nextId++;
    this.rows.push({ id, title, duration: "--:--", sourceUrl: url });
    return id;
  }

  private setDuration(id: number, seconds: number) {
    const row = this.rows.find((r) => r.id === id);
    if (row) row.duration = fmtDur(seconds);
  }

  /** Fetch and decode a registered-but-unloaded row. */
  private async ensureLoaded(id: number): Promise<boolean> {
    if (this.tracks.has(id)) return true;
    const row = this.rows.find((r) => r.id === id);
    if (!row?.sourceUrl) return false;
    try {
      const ctx = await this.ensureContext();
      const res = await fetch(row.sourceUrl);
      if (!res.ok) throw new Error(String(res.status));
      const buffer = await ctx.decodeAudioData(await res.arrayBuffer());
      this.tracks.set(id, { buffer, title: row.title });
      this.setDuration(id, buffer.duration);
      return true;
    } catch {
      this.onError?.(`Cannot load ${row.title}`);
      return false;
    }
  }

  playlist(): PlaylistRow[] {
    return this.rows;
  }

  clear(): void {
    this.stop();
    this.tracks.clear();
    this.rows = [];
    this.activeId = null;
  }

  // ---------------------------------------------------------------- transport

  /**
   * Start playback without a user gesture, where the browser allows it.
   *
   * Returns false when the `AudioContext` is still suspended — the autoplay
   * policy has blocked sound until the user interacts with the page. The track
   * stays loaded and decoded in that case, so the caller only has to arm a
   * gesture handler and start for real on the first click.
   *
   * Never reports success while the context is suspended: a UI that says
   * "Playing" over silence is worse than one that says "press play".
   */
  async tryAutoplay(): Promise<boolean> {
    try {
      await this.play();
    } catch {
      return false;
    }
    if (this.ctx?.state === "running") return true;
    this.pause();
    return false;
  }

  async play(): Promise<void> {
    const node = await this.ensureEngine();
    let id = this.activeId;
    if (id === null || !this.tracks.has(id)) {
      const first = this.rows[0];
      if (!first) throw new Error("no track loaded");
      id = first.id;
      this.activeId = id;
      if (!(await this.ensureLoaded(id))) throw new Error("track unavailable");
    }
    const track = this.tracks.get(id)!;
    node.port.postMessage({
      type: "track",
      samples: interleave(track.buffer),
      channels: track.buffer.numberOfChannels,
    });
    node.port.postMessage({ type: "play" });
    this.pushParams();
    this.restoreGain();
    this.playing = true;
    this.onTrackChanged?.(id);
  }

  pause(): void {
    this.node?.port.postMessage({ type: "pause" });
    // Duck the safety node too, so a pause is instant even if the worklet is
    // mid-block and has not yet observed the message.
    if (this.gain && this.ctx) {
      this.gain.gain.setTargetAtTime(0, this.ctx.currentTime, 0.005);
    }
    this.playing = false;
  }

  stop(): void {
    this.node?.port.postMessage({ type: "stop" });
    this.node?.port.postMessage({ type: "seek", seconds: 0 });
    this.restoreGain();
    this.playing = false;
    this.position = 0;
  }

  /** Undo `pause`'s duck, ramping rather than jumping to avoid a click. */
  private restoreGain() {
    if (this.gain && this.ctx) {
      this.gain.gain.setTargetAtTime(1, this.ctx.currentTime, 0.005);
    }
  }

  async togglePlay(): Promise<boolean> {
    if (this.playing) {
      this.pause();
      return false;
    }
    await this.play();
    return true;
  }

  private step(delta: number): void {
    if (this.rows.length === 0) return;
    const idx = this.rows.findIndex((r) => r.id === this.activeId);
    const from = idx < 0 ? 0 : idx;
    const next = (from + delta + this.rows.length) % this.rows.length;
    void this.playIndex(next);
  }

  next(): void {
    this.step(1);
  }

  prev(): void {
    this.step(-1);
  }

  async playIndex(index: number): Promise<void> {
    const row = this.rows[index];
    if (!row) return;
    this.activeId = row.id;
    this.position = 0;
    // A lazily-registered row has no audio yet; fetch it now that the listener
    // has actually asked for it.
    if (!(await this.ensureLoaded(row.id))) return;
    await this.play();
  }

  isPlaying(): boolean {
    return this.playing;
  }

  // ------------------------------------------------------------------ params

  setParams(params: AudioParamsInput): void {
    this.params = params;
    this.pushParams();
  }

  private pushParams(): void {
    const node = this.node;
    if (!node) return;
    node.port.postMessage({
      type: "params",
      volume: this.params.volume,
      speed: this.params.speed,
      pitch: this.params.pitch,
      reverb: this.params.reverb,
      eq: Float32Array.from(this.params.eq),
    });
  }

  // ---------------------------------------------------------------- position

  getPosition(): number {
    return this.position;
  }

  duration(): number {
    if (this.activeId === null) return 0;
    return this.tracks.get(this.activeId)?.buffer.duration ?? 0;
  }

  seek(seconds: number): void {
    const max = Math.max(0, this.duration() - 0.05);
    const target = Math.min(Math.max(0, seconds), max);
    this.position = target;
    this.node?.port.postMessage({ type: "seek", seconds: target });
  }

  /** Cue to a fraction of the track, starting playback if paused. */
  cuePercent(fraction: number): number {
    const dur = this.duration();
    if (dur <= 0) return -1;
    const max = Math.max(0, dur - 0.05);
    const target = Math.min(Math.max(0, fraction) * dur, max);
    this.seek(target);
    if (!this.playing) void this.play();
    return target;
  }

  get activeRowId(): number | null {
    return this.activeId;
  }

  get isLoading(): boolean {
    return this.loading;
  }
}

/**
 * Interleave an `AudioBuffer`'s per-channel `Float32Array`s into one buffer.
 * `AudioBuffer` stores channels separately; the DSP (like the desktop engine)
 * reads interleaved samples.
 */
function interleave(buffer: AudioBuffer): Float32Array {
  const ch = buffer.numberOfChannels;
  const frames = buffer.length;
  if (ch === 1) return buffer.getChannelData(0).slice();
  const l = buffer.getChannelData(0);
  const r = ch > 1 ? buffer.getChannelData(1) : l;
  const out = new Float32Array(frames * 2);
  for (let i = 0; i < frames; i++) {
    out[i * 2] = l[i];
    out[i * 2 + 1] = r[i];
  }
  return out;
}

/** m:ss for the playlist's duration column, matching the desktop format. */
function fmtDur(secs: number): string {
  const s = Math.max(0, Math.round(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export { AUDIO_ACCEPT };
