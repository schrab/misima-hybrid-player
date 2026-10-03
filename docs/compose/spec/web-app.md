# Web App Port — Implementation Plan

Port Misima Hybrid Player to a static browser application with an **identical DSP
chain**. Deploy to GitHub Pages with no backend.

- **Status:** ready for implementation
- **Repo:** `misima-hybrid-player`

---

## 1. Deployment

**GitHub Pages.** The app is fully static — `vite build` emits `dist/`, which is
one HTML file, hashed JS/CSS, the WASM binary, and the 3.6 MB skin folder.
There is no server component: no SSR, no API routes, no database.

### What Pages provides

| Requirement | Why the audio engine needs it | GitHub Pages |
|---|---|---|
| HTTPS | `AudioContext` + `AudioWorklet` require a secure context | ✅ |
| `application/wasm` MIME | `WebAssembly.compileStreaming` | ✅ |
| Range requests | Buffer seeking over HTTP (Phase 5) | ✅ |

### Required code changes

**Vite base path** — GitHub Pages project sites serve from a subpath
(`https://schrab.github.io/misima-hybrid-player/`), not the domain root:

```ts
// app/vite.config.ts
export default defineConfig({
  base: "/misima-hybrid-player/",   // matches the repo name
  // existing config...
});
```

**Asset base in main.ts** — currently hardcoded:

```ts
// app/src/main.ts:29  (currently: const BASE = "/sprite/";)
const BASE = `${import.meta.env.BASE_URL}sprite/`;
```

**AudioWorklet module URL** — must go through Vite's asset pipeline:

```ts
await ctx.audioWorklet.addModule(
  new URL("./worklet/dspWorklet.js", import.meta.url)
);
```

Any hand-written absolute path like `"/dsp.wasm"` works in `npm run dev` and
silently 404s in production because the Vite dev server ignores `base`.

### Also required

- `.nojekyll` — empty file, disables Jekyll processing so files are served
  verbatim.
- `.github/workflows/pages.yml` — new workflow; `release.yml` stays untouched.
- Pages Settings → Source → **GitHub Actions**.

The workflow runs `npm run build` (which is `tsc` → tests → `vite build`, so a
failing test fails the deploy), preceded by a WASM build step using
`wasm-pack build --target web` with `RUSTFLAGS="-C target-feature=+simd128"`.

### Known limits

- 100 GB/month bandwidth soft cap, no SLA. Fine for local files; if it ever
  matters, move audio to Cloudflare R2 with CORS headers and leave the app on
  Pages.
- 1 GB repo soft limit (100 MB per file). Current repo is ~3.6 MB of assets.
- Pages has no serverless functions — the radio CORS proxy cannot live here.
  Radio is out of scope for v1 (see §7).

---

## 2. Scope

**In:** local files (picker + drag/drop), the player's own hosted MP3s over
HTTP, and the full FX chain — bit-perfect bypass, time+pitch, 10-band EQ,
visualizer taps, FDN reverb, hard-clip and master volume.

**Out (v1):** live radio streams. Tracked in §7.

---

## 3. Architecture

```
 <audio> / AudioBufferSourceNode
        |  (browser-native decoder: mp3/flac/wav/ogg*)
        v
  AudioWorkletNode "dsp-processor"     ← Rust → WASM (+simd128), single thread
        |    1. bypass (speed=1.0, pitch=0)       bit-perfect passthrough
        |    2. WSOLA / phase vocoder             stretch = speed / pitch
        |    3. 10-band peaking EQ                RBJ biquads, seamless
        |    4. taps → 48-bin FFT + 226-pt scope
        |    5. FDN reverb                        envelope-normalized wet gain
        |    6. hard-clip + master volume
        v
   ctx.destination
        ^  port.postMessage:  params in | spectrum + waveform out
        |
  main.ts (canvas UI) ←→ transport.ts ←→ web/player.ts
```

\* Safari does not support OGG Vorbis — see §9, risk 5.

---

## 4. Key decisions

### 4.1 Port the DSP to WASM; do not rewrite in Web Audio nodes

The chain has been tuned by ear — the reverb's `TARGET` above 1.0 and its
envelope-normalized wet gain, the vocoder/WSOLA split at `stretch <= 1`, the
`REVERB_TIME` 0.55 loop-gain ceiling. Porting reproduces them exactly.
Rewriting in native nodes would mean a second implementation that does not
sound the same, and the FX would drift apart over time.

### 4.2 Share the DSP source files; do not fork them

The web crate includes the existing `src-tauri/src/audio/` modules via `#[path]`
module declarations. **Eight modules** are shared:

```
clouds_reverb.rs    eq.rs          spectrum.rs     wsola.rs
phase_vocoder.rs    dsp_utils.rs   reverb_mix.rs   stretcher.rs
```

The last two are new — extracted from `player.rs` before the port begins
(see §4.8). Their unit tests come with them and must stay green in both crates.
This is the most important structural choice in the plan: a DSP fix lands on
both platforms at once, and the web build can never silently diverge from the
desktop one.

#### Module tree contract

The shared modules reference each other via `use crate::audio::*` paths:

```rust
// phase_vocoder.rs, wsola.rs
use crate::audio::dsp_utils::{cubic_hermite, read_stereo_f64};
```

Both crates must expose the same module tree shape. The web crate's `lib.rs`:

```rust
// wasm-dsp/src/lib.rs
mod audio {
    #[path = "../../src-tauri/src/audio/clouds_reverb.rs"]
    pub mod clouds_reverb;
    #[path = "../../src-tauri/src/audio/dsp_utils.rs"]
    pub mod dsp_utils;
    #[path = "../../src-tauri/src/audio/eq.rs"]
    pub mod eq;
    #[path = "../../src-tauri/src/audio/phase_vocoder.rs"]
    pub mod phase_vocoder;
    #[path = "../../src-tauri/src/audio/spectrum.rs"]
    pub mod spectrum;
    #[path = "../../src-tauri/src/audio/wsola.rs"]
    pub mod wsola;
    #[path = "../../src-tauri/src/audio/reverb_mix.rs"]
    pub mod reverb_mix;
    #[path = "../../src-tauri/src/audio/stretcher.rs"]
    pub mod stretcher;
}
```

CI must run `cargo check --target wasm32-unknown-unknown` in `wasm-dsp/` on
every PR that touches `src/audio/`.

### 4.3 A separate crate, not a feature flag

`player.rs` pulls in cpal, tauri, parking_lot, crossbeam, and the
`OnceLock<Arc<SharedPlay>>` global. The web build replaces it rather than
compiling it. A small dedicated crate keeps the dependency graph clean:

- `app/wasm-dsp/Cargo.toml` — `wasm-bindgen`, `js-sys`, `rustfft`
- deliberately **no** `cpal`, `tauri`, `symphonia`, `crossbeam`, `parking_lot`

A pleasant side effect: the RT threading invariants from AGENTS.md §3.1 mostly
dissolve. A worklet is single-threaded, so the hoisted-lock discipline has
nothing to hoist — parameters arrive by message port, state lives in the
processor struct. The allocation rules and the `saturating_sub` guards around
the WSOLA FIFO still apply verbatim; a FIFO underrun in a worklet is a normal
realtime event that should emit silence, not panic.

### 4.4 `decodeAudioData` + `AudioBufferSourceNode`, not `MediaElementAudioSourceNode`

The desktop stretcher seeks freely inside a decoded buffer
(`get_play_pos` / `Stretcher::reset`). A `MediaElementAudioSourceNode` is a
*live stream* with no random access: WSOLA's `nominal_pos` and tau-search
cannot seek backwards, and scrubbing cannot work.

Decoding the file up front yields an `AudioBuffer` that the worklet can seek
into exactly like the desktop engine. Cost is memory — roughly 115 MB for a
5-minute stereo track at 48 kHz as f32. Fine for normal tracks. MediaSource
Extensions is the escape hatch if hour-long files ever need streaming.

Note: `AudioBuffer` stores channels as **separate `Float32Array`s** (one per
channel), not interleaved. The worklet must interleave on read or the stretcher
must accept separate L/R views. Interleaving in the worklet's input FIFO is
simplest and matches the desktop's data layout.

### 4.5 Feed `ctx.sampleRate` into the DSP

Browsers pick 48 kHz or 44.1 kHz. `Biquad::peaking` takes a sample rate and
the reverb already scales by `device_sample_rate / 32_000.0`, so both are
parameterized already — just pass the context rate at construction.

### 4.6 Transport abstraction, not a frontend rewrite

`main.ts` reaches Tauri through **17 unique `invoke()` commands** and
**7 `listen()` event channels**. Introduce `transport.ts` exposing that same
surface with two implementations: the existing Tauri one (desktop, untouched)
and a new web one.

#### Complete IPC surface — invoke commands

| Command | Args | Returns | Used at |
|---|---|---|---|
| `play` | — | `void` | L306 |
| `pause` | — | `void` | L301, L318 |
| `stop` | — | `void` | L323 |
| `toggle_play` | — | `boolean` | L162 |
| `next` | — | `void` | L195, L329, L827 |
| `prev` | — | `void` | L186, L329 |
| `play_index` | `{ index: number }` | `void` | L678 |
| `set_params` | `{ volume, speed, pitch, eq, reverb }` | `void` | L261 |
| `open_files` | `{ paths: string[] }` | `void` | L294 |
| `get_position` | — | `number` | L172 |
| `get_playlist` | — | `Row[]` | L280, L332 |
| `clear_playlist` | — | `void` | L339 |
| `seek` | `{ seconds: number }` | `void` | L175 |
| `cue_percent` | `{ fraction: number }` | `number` | L151 |
| `set_ui_scale` | `{ scale: number }` | `UiScaleInfo` | L52 |
| `get_ui_scale` | — | `UiScaleInfo` | L127, L700 |
| `cycle_ui_scale` | `{ direction: number }` | `UiScaleInfo` | L75 |

#### Complete IPC surface — listen events

| Event | Payload | Used at |
|---|---|---|
| `spectrum` | `Float32Array` (48 bins) | L784 |
| `waveform` | `Float32Array` (226 pts) | L804 |
| `play_started` | — | L812 |
| `error` | `string` | L816 |
| `track_changed` | `number` (row ID) | L820 |
| `track_ended` | — | L825 |
| `ui_scale` | `UiScaleInfo` | L695 |

#### Web transport mappings

- **Desktop-only commands** (no-ops on web): `set_ui_scale`, `get_ui_scale`,
  `cycle_ui_scale`. Zoom becomes CSS `transform: scale()` on the canvas.
- **Desktop-only events** (not emitted on web): `ui_scale`.
- **Desktop-only actions** (no-ops on web): `getCurrentWindow().startDragging()`,
  `getCurrentWindow().minimize()`, `getCurrentWindow().close()`.
- **Replaced on web**: `open_files` (uses `<input type=file>` + drag/drop
  instead of the Tauri dialog; paths become in-memory `File` objects).

The 584+ lines of canvas, fader, and skin logic in `main.ts` never learn it
moved platforms.

### 4.7 Zoom becomes CSS

On desktop, Rust owns window size and zoom because WebView2 otherwise steals
Ctrl+±. On web there is no native window, so zoom is a CSS scale on the canvas
and `main.ts` `preventDefault`s Ctrl/± itself.

Removed (no-ops): `resize_window_px`, `get_ui_scale`, `set_ui_scale`,
`cycle_ui_scale`. Kept: `applyScale`, rewritten as pure CSS transform.
`minimize` / `power` / `startDragging` become no-ops.

### 4.8 Pre-port refactor — extract `Reverb` and `Stretcher` from `player.rs`

Before the web work begins, extract these types out of `player.rs` into shared
modules so both crates can `#[path]`-include them:

**New file: `src-tauri/src/audio/reverb_mix.rs`** — extracted from
`player.rs:100–214`:

| Item | Lines | Why shared |
|---|---|---|
| `struct Reverb` | L106–L192 | Envelope-normalized wet gain, level followers, TARGET=1.8 |
| `fn follow()` | L194–L202 | Envelope follower helper |
| `fn mix_reverb_frame()` | L210–L214 | The `w/√(1+w²)` soft-clip crossfade |

These types have **zero platform dependencies** — no cpal, no tauri, no
parking_lot. They only use `clouds_reverb::CloudsReverb`. The extraction is
mechanical: move the code, add `use crate::audio::clouds_reverb::CloudsReverb`,
update `player.rs` to `use crate::audio::reverb_mix::*`.

**New file: `src-tauri/src/audio/stretcher.rs`** — extracted from
`player.rs:650–723`:

| Item | Lines | Why shared |
|---|---|---|
| `struct Stretcher` | L660–L723 | Engine selector (vocoder vs WSOLA), swap logic, seek-reset |

Depends only on `wsola::WsolaProcessor` and `phase_vocoder::PhaseVocoder`.
Same mechanical extraction.

**Not extracted** (stays in `player.rs`): `tempo_factor()`, `pitch_ratio()`,
`SharedPlay`, `build_stream`, `spawn_spectrum_task`, `resample_interleaved` —
these use cpal/tauri/parking_lot and must be reimplemented in the worklet.

### 4.9 Compile WASM with SIMD (`+simd128`)

`rustfft` automatically uses SIMD-accelerated FFT kernels when the target
supports them. On native x86 it uses AVX2/SSE; on ARM it uses NEON. On
`wasm32-unknown-unknown` **without** `simd128`, it falls back to scalar code
that is ~3–4× slower.

WebAssembly SIMD (`simd128`) is universally supported in all modern browsers:
- Chrome 91+ (May 2021)
- Firefox 89+ (June 2021)
- Safari 16.4+ (March 2023)

There is no need for a dual-binary fallback strategy. Compile with SIMD
unconditionally:

```bash
RUSTFLAGS="-C target-feature=+simd128" wasm-pack build --target web
```

This gives `rustfft`'s 1024-pt spectrum FFT and 2048-pt phase vocoder FFT
near-native throughput inside the worklet, with comfortable margin within the
128-sample render quantum budget.

---

## 5. Files

### New — Rust (pre-port refactor)

| Path | Purpose |
|---|---|
| `app/src-tauri/src/audio/reverb_mix.rs` | `Reverb`, `follow()`, `mix_reverb_frame()` extracted from `player.rs` |
| `app/src-tauri/src/audio/stretcher.rs` | `Stretcher` engine selector extracted from `player.rs` |

### New — Rust (web crate)

| Path | Purpose |
|---|---|
| `app/wasm-dsp/Cargo.toml` | `wasm-bindgen`, `js-sys`, `rustfft`; no cpal/tauri/symphonia |
| `app/wasm-dsp/src/lib.rs` | `#[path]`-includes the eight shared audio modules, exposes `DspProcessor` |
| `app/wasm-dsp/src/processor.rs` | `DspProcessor`: owns `EqState`, `Reverb`, `Stretcher`, `SpectrumAnalyzer`, input/output FIFOs, params; exposes `process()` and `set_params()` via `wasm_bindgen` |

### New — frontend

| Path | Purpose |
|---|---|
| `app/src/transport.ts` | `Transport` interface + `TauriTransport` (desktop) + `WebTransport` (web) |
| `app/src/web/player.ts` | `AudioContext` lifecycle, decoded-buffer playlist, position tracking, worklet wiring |
| `app/src/web/files.ts` | `<input type=file multiple>` + drag/drop → `File` → `decodeAudioData` |
| `app/src/worklet/dspWorklet.js` | `registerProcessor` shim; `initSync(wasmModule)` from `processorOptions` |

### Modified

| Path | Change |
|---|---|
| `app/src-tauri/src/audio/player.rs` | Extract `Reverb`/`Stretcher` → import from `reverb_mix`/`stretcher` |
| `app/src-tauri/src/audio/mod.rs` | Add `pub mod reverb_mix; pub mod stretcher;` |
| `app/src/main.ts` | Swap `invoke`/`listen` for `transport`; CSS zoom; `BASE` path fix |
| `app/vite.config.ts` | Add `base: "/misima-hybrid-player/"` |
| `.github/workflows/pages.yml` | New deployment workflow |
| `.nojekyll` | Empty marker file |

### Unchanged (reused as-is)

All eight shared DSP modules, `app/src/sprite/` (canvas compositor, font engine,
layout, visuals), `app/public/sprite/` (the 3.6 MB skin), all existing tests.

---

## 6. Phased build

Each phase ends with something audible, so the web port is demonstrable from
Phase 1 onward.

### Phase 0 — pre-port refactor (desktop only)

Extract `Reverb`, `follow()`, `mix_reverb_frame()` into `reverb_mix.rs`.
Extract `Stretcher` into `stretcher.rs`. Update `player.rs` and `mod.rs`
imports. **No behavioral change.**

**Verify:** `cargo test` passes (42/42 + 1 ignored), `cargo check` zero
warnings, `npm test && npm run build` green. Desktop plays identically.

### Phase 1 — WASM crate + worklet plumbing + EQ + reverb

Prove the worklet path before attempting the stretcher.

1. Create `wasm-dsp/` crate with `#[path]` includes of all eight shared modules.
2. Implement `DspProcessor` struct with `EqState`, `Reverb`, output FIFO.
3. Build WASM with `simd128`: `RUSTFLAGS="-C target-feature=+simd128" wasm-pack build --target web`.
4. Create `dspWorklet.js`:
   - Receive `WebAssembly.Module` via `processorOptions`.
   - Call `initSync(wasmModule)` in constructor.
   - Instantiate `DspProcessor` with `sampleRate`.
   - `process()`: receive params via port, push input into processor, pull 128
     samples out, write to output.
5. Create `transport.ts` with `Transport` interface covering all 17 commands
   and 7 events. Implement `TauriTransport` (wraps existing `invoke`/`listen`)
   and `WebTransport` (stub, wired incrementally).
6. Wire `main.ts` through `transport`.

**WASM loading sequence:**
```
main thread:
  fetch("dsp_bg.wasm")
  → WebAssembly.compileStreaming(response)
  → ctx.audioWorklet.addModule("dspWorklet.js")
  → new AudioWorkletNode(ctx, "dsp-processor", {
      processorOptions: { wasmModule }
    })

worklet constructor:
  initSync(options.processorOptions.wasmModule)
  this.processor = new DspProcessor(sampleRate)
```

**Verify:** Audible EQ sweep and reverb tail in the browser (on a test tone
or silent-then-noise buffer). Volume fader works. Desktop build untouched
and still passes all checks.

### Phase 2 — file playback + playlist

1. `files.ts`: `<input type=file multiple accept=".mp3,.flac,.wav,.ogg">` and
   drag/drop → `File[]`.
2. `player.ts`: `File` → `arrayBuffer()` → `ctx.decodeAudioData()` →
   `AudioBuffer`. Interleave channels into a single `Float32Array` and transfer
   to the worklet via `port.postMessage(buffer, [buffer.buffer])` (transferable).
3. Play/pause/stop/next/prev/play_index/toggle_play wired through transport.
4. Playlist rows, durations (from `AudioBuffer.duration`).
5. Keyboard transport (space, Z/X, arrows, 0–9 cue).
6. Position tracking: worklet counts source frames consumed by the stretcher
   (at speed 1.0 this is just sample offset). Reports position to main thread
   via `port.postMessage` every ~33ms. `get_position` reads the last received
   value.

**Verify:** Load an MP3 via picker. Drag-drop several files. Play, pause, stop.
Next/prev cycles playlist. Position display tracks playback. Replay a track
at end. Visualizer is dark (Phase 4).

### Phase 3 — time/pitch (the hard one)

WSOLA and the phase vocoder in the worklet, with the `Stretcher` engine
selector.

**128-sample quantum buffering strategy:**

The AudioWorklet's `process()` is called with a fixed 128-sample render quantum.
The WSOLA uses 1024-sample grains (512-sample hop) and the phase vocoder uses
2048-point FFTs (512-sample synthesis hop). Both engines produce output in
chunks larger than 128 samples.

Solution: `DspProcessor` maintains an **output ring buffer** (e.g. 4096
samples). When the ring buffer drops below a watermark (e.g. 512 samples),
call `Stretcher::process` requesting a batch of frames to refill it. Each
`process()` call drains exactly 128 stereo frames from the ring buffer.

The **input side** works similarly: the worklet receives decoded audio data via
`postMessage` (the full interleaved buffer, transferred once). The stretcher
reads directly from this buffer by sample offset, exactly like the desktop
engine reads from `shared.samples`.

Seek: main thread sends `{ type: "seek", frame: N }` via the port. The worklet
resets `Stretcher::reset(pos)`, `Reverb::clear()`, and flushes the output ring
buffer.

Engine swap: `Stretcher::select(speed, pr, pos)` handles the vocoder/WSOLA
crossover at `stretch <= 1.0`, same as desktop.

**Verify:** Speed 0.5–2.0× sweep. Pitch ±12 semitones. Rubber-band quality
matches desktop. Seek at various positions while time-stretched. Engine swap
boundary (speed fader crossing 1.0 at non-zero pitch) is glitch-free.

### Phase 4 — visualizer taps

48-bin FFT spectrum and 226-point echo scope, computed inside the worklet's
`process()` loop (post-EQ, pre-reverb — same tap point as desktop), returned to
main thread via `port.postMessage` at ~30 fps.

The idle-noise gate from AGENTS.md §3.2.4 must hold: no rendering when energy
<= 0.01.

Throttle: the worklet accumulates mono samples and runs the FFT + decimation
every ~33ms worth of samples (e.g. every ~1584 samples at 48 kHz). This avoids
flooding the message port.

**Verify:** Spectrum chips and echo scope animate during playback. Visualizer
settles to zero when paused. Band shapes match desktop on identical material.

### Phase 5 — hosted MP3s

Fetch URL → `Response` → `arrayBuffer()` → `decodeAudioData()`. Same pipeline
as local files, different source. Display loading state during fetch.

For seek on partially-loaded files: wait for full decode before enabling seek
(the entire file is in memory anyway per §4.4).

**Verify:** Play an MP3 hosted on the project's GitHub Pages domain. Seek works
after load. Error state on 404 / network failure.

### Phase 6 — ship

1. Set `base: "/misima-hybrid-player/"` in `vite.config.ts`.
2. Add `.nojekyll` to repo root.
3. Create `.github/workflows/pages.yml` (build `wasm-dsp` → `npm run build` →
   upload `app/dist`).
4. Configure Pages Settings → Source → GitHub Actions.
5. Deploy and verify the skin, worklet, and WASM all load from the subpath URL.

**Verify:** Full app loads at `https://schrab.github.io/misima-hybrid-player/`.
All phases' functionality works end-to-end. Desktop `release.yml` still works
independently.

---

## 7. Radio (deferred)

Live streams need `Access-Control-Allow-Origin` on the stream itself, or
`MediaElementAudioSourceNode` silences the whole graph. Public Icecast and
SHOUTcast endpoints generally do not send it, and there is no client-side
workaround.

When this is picked up, the fix is a ~30-line header-injecting byte-passthrough
edge worker (Cloudflare Worker or Fastly). **Not** a Pages function and **not**
a Vercel function: serverless request timeouts would kill an infinite stream.

Phase 5 already establishes the fetch-and-decode path, so radio is additive
work rather than a redesign.

---

## 8. Verification

Per AGENTS.md §5, after every phase:

```bash
# Rust — both crates, shared tests must pass in each
cd app/src-tauri && cargo test             # 42/42 + 1 ignored
cd app/wasm-dsp  && cargo test             # shared module tests

# Lints
cd app/src-tauri && cargo check            # zero warnings
cd app/wasm-dsp  && cargo clippy --target wasm32-unknown-unknown

# Frontend — typecheck, tests, build
cd app && npm test && npm run build
```

Manual, per phase:

- **Bypass fidelity:** render a test tone through desktop and web at
  speed 1.0 / pitch 0, compare samples. They must be bit-identical through
  the EQ (with all bands at 0 dB).
- **FX parity:** sweep EQ, sweep speed, audition reverb on identical material
  in both builds. Listen for tonal or loudness differences.
- **Position accuracy:** after a seek, compare the UI's reported position
  against actual audible output at 10s / 60s / end of track.

---

## 9. Risks

### 1. WASM inside an AudioWorklet (HIGH — mitigated by Phase 1)

AudioWorkletGlobalScope is not a normal window scope: no `fetch`, and
`registerProcessor` needs a separately-loaded module.

**Mitigation:** Main thread does `fetch` + `WebAssembly.compileStreaming`, then
passes the compiled `WebAssembly.Module` via `processorOptions` to the worklet
constructor. The worklet calls `initSync(module)` from `wasm-bindgen`'s
generated glue. Front-load a Hello-Square worklet before Phase 1's real DSP
so this risk is retired first.

**Note:** `AudioWorkletGlobalScope` may lack `TextDecoder`. If wasm-bindgen's
glue requires it, polyfill it in the worklet script before `initSync`.

### 2. Time/pitch quality (MEDIUM)

The stretcher is the most complex and least browser-tested code. Phase 1
front-loads EQ and reverb to validate plumbing independently of the hardest
module.

### 3. Position drift (MEDIUM)

`ctx.currentTime` counts **wall time (output time)**, not source time. At
speed 2.0×, 1 second of wall time = 2 seconds of source position. The worklet
must track source position internally via the stretcher's `get_play_pos()`,
not derive it from `ctx.currentTime`.

Sample counting plus periodic position reports via the message port. Jump
detection: if position changes by more than 1 second between reports, it's a
seek — the UI should snap, not animate.

### 4. Desktop regression (LOW — mitigated by shared modules)

Because the DSP modules are shared by `#[path]`, an edit affects both builds.
Keep `player.rs` (cpal + tauri) out of the shared set. Run both `cargo test`
suites before any DSP commit.

### 5. Safari OGG Vorbis (LOW)

Safari does **not** support OGG Vorbis via `decodeAudioData`. MP3, WAV, and
FLAC all work. If OGG support on Safari is needed, Symphonia's Vorbis decoder
could be compiled to WASM as a fallback — but this is out of scope for v1
unless demand materializes.

### 6. Autoplay policy (LOW)

`AudioContext` starts suspended and must `resume()` on a user gesture. Some
browsers (Safari) also require the `AudioContext` to be **constructed** during
a user gesture handler.

**Mitigation:** Create the `AudioContext` lazily on the first play button click
or file-drop, not at page load. Call `ctx.resume()` in the same handler.

### 7. Memory (LOW)

Whole-file decoding is fine for typical tracks (~115 MB for 5 min stereo at
48 kHz) but is a real limit for hour-long files. Note it now; MediaSource
Extensions is the fix if it bites.

### 8. 128-sample worklet quantum vs grain sizes (LOW — mitigated by output FIFO)

The WSOLA's 1024-sample grains and the vocoder's 2048-pt FFT both produce
output in chunks larger than the 128-sample render quantum. The output ring
buffer in `DspProcessor` (see Phase 3) handles this transparently.

---

## 10. Estimate

| Phase | Content | Rough effort |
|---|---|---|
| 0 | Pre-port refactor (extract Reverb/Stretcher) | 2–4 hours |
| 1 | WASM crate, worklet, EQ + reverb, transport | 1.5–2 weeks |
| 2 | File playback, playlist, transport keys | 0.5–1 week |
| 3 | WSOLA + phase vocoder + output FIFO | 1–2 weeks |
| 4 | Visualizer taps | 2–4 days |
| 5 | Hosted MP3s | 2–3 days |
| 6 | Pages deploy, base path | hours |

Roughly 4–6 weeks of focused work, with Phase 3 the least predictable.

---

## 11. Open questions

1. **File formats.** The desktop filter is mp3/flac/wav/ogg; `decodeAudioData`
   covers all four in modern Chromium and Firefox. Safari lacks OGG Vorbis.
   Accept as a known limitation for v1?
2. **Hosted MP3 origin.** Where the library lives for Phase 5 — same GitHub
   repo (simplest, subject to the bandwidth cap), or R2/other object storage?
3. **Skin loading.** The desktop build can open a `.mskin` skin at runtime
   (`skin.rs`). On web, ship the skin as static files and read the bundled
   `skin.json` only. Custom web skins are a v2 feature if there's demand.
4. **Desktop build during the port.** Keep `src-tauri` building and passing
   throughout. The shared-module approach requires it — both crates' tests
   must stay green on every commit.