# Web Port — Implementation Log

Running record of the static-browser port described in
[`compose/spec/web-app.md`](compose/spec/web-app.md). One section per phase,
committed as each phase lands, so the history shows what shipped and what it
was verified against.

The spec is the plan of record. This file is the diary: what actually
happened, what verification really reported, and where the plan turned out to
be wrong or optimistic.

---

## Phase 0 — pre-port refactor (desktop only) — **complete**

Extract the two platform-free types out of `player.rs` so both crates can
`#[path]`-include them. No behavioural change intended.

**Done:**

- New `src/audio/reverb_mix.rs`: `Reverb`, `follow()`, `mix_reverb_frame()`.
- New `src/audio/stretcher.rs`: `Stretcher` (engine selector).
- `player.rs` now imports both instead of defining them; `mod.rs` registers
  the new modules.
- Tests moved with their code: `reverb_dry_ish_stability` and
  `reverb_mix_loudness_constant` → `reverb_mix.rs`; `wsola_pipeline_integration`
  → `stretcher.rs`. Added `select_swaps_engine_on_pitch_up` to pin the
  vocoder/WSOLA crossover, which had no direct test before.
- `docs/DSP.md`: module map updated, plus a new *Shared vs desktop-only*
  section naming the eight shared modules and the two that stay desktop-side.

**Why these two, specifically:** `Reverb` and `Stretcher` are the only types in
`player.rs` that touch no cpal, tauri, or parking_lot API. `player.rs` also
holds `SharedPlay`, the stream-owner thread, and `resample_interleaved`, which
all do, and which the worklet reimplements rather than shares.

**Verified:** `cargo test` 43 passed / 1 ignored / 0 failed (was 42 — the new
crossover test is the delta). `cargo check` zero warnings. `npm test` green,
`npm run build` green. Desktop behaviour unchanged: the extracted code is
byte-identical, only relocated.

**Note for whoever runs this next:** a stale `target/` cache in this checkout
made `cargo check` fail with `failed to read plugin permissions ... misima-hybrid-winamp\...`
— a path from a previous, renamed checkout. `cargo clean -p tauri -p tauri-build
-p tauri-plugin-dialog -p tauri-plugin-fs` clears it. Unrelated to any source
change.

---

## Phase 1 — WASM crate + worklet plumbing + EQ + reverb — **complete**

The Rust half of the port, and the transport split that lets `main.ts` speak
one interface on both platforms.

**Done:**

- `app/wasm-dsp/` — new crate (`wasm-bindgen`, `js-sys`, `rustfft`; deliberately
  no cpal/tauri/symphonia/crossbeam/parking_lot). `lib.rs` `#[path]`-includes
  all eight shared DSP modules.
- `src/processor.rs` — `DspProcessor`, the worklet's whole engine. Plain Rust,
  no `wasm_bindgen`, so it is unit-testable on the host target.
- `src/bindings.rs` — the `wasm_bindgen` surface, `wasm32` only.
- `src/worklet/dspWorklet.js` + `polyfill.js` — the worklet, bundled to a single
  import-free file by `scripts/build-worklet.mjs` (esbuild).
- `src/transport.ts` + `transportTauri.ts` + `transportWeb.ts` — the `Transport`
  interface and both implementations. `main.ts` no longer imports
  `@tauri-apps/api` at all.
- `src/web/player.ts`, `src/web/files.ts`, `src/web/base.ts` — AudioContext
  lifecycle, decode, picker + drag/drop.
- `src/transport.test.ts` — 14 checks on the web transport's contract.
- `scripts/build-wasm.mjs`, `build-worklet.mjs`, `build-pages.mjs`.
- Vite base path is now conditional on `GH_PAGES=1` (see below).

**Verified:** `cargo test` 36 passed in `wasm-dsp` (26 shared-module tests plus
10 new processor tests); `cargo clippy --target wasm32-unknown-unknown` clean
for every file this port added. Frontend `tsc` clean, 14 transport checks pass.

### The plan's output-ring-buffer prediction was wrong — and it was right to be

§6 Phase 3 specifies a 4096-frame output ring with a 1024-frame watermark, to
paper over the fact that the WSOLA emits 1024-sample grains and the vocoder
512-sample hops against a 128-frame render quantum.

No ring buffer was needed. Both engines already keep their own internal FIFOs
(`WsolaProcessor::fifo_l`, the vocoder's overlap buffers) and are explicitly
chunk-size agnostic — `process()` takes `out_frames` and maintains its read
cursor across calls. The desktop already proves this: it calls `process()` with
whatever cpal's block size happens to be, anywhere from 512 to 2048 frames, and
gets a continuous stream. Asking for 128 works the same way.

The processor's scratch is sized well above 128 regardless, so a mid-block
engine swap or fader change always has somewhere to land without reallocating.

### Three bugs that only a browser could find

Every one of these passed `cargo test`, `tsc`, `npm test`, and `vite build`.
They were found by running the app and measuring its actual output. Worth
recording, because the failure modes are all maximally misleading.

**1. `self` does not exist in `AudioWorkletGlobalScope`.**

The polyfill attached to `self.TextDecoder`. `AudioWorkletGlobalScope` has no
`self` binding — measured directly: `typeof self === "undefined"` there, while
`globalThis` is an object. That threw a `ReferenceError` during module
evaluation, *before* `registerProcessor` ran.

The symptom is a lie. `addModule()` resolves successfully, and only a later
`new AudioWorkletNode(...)` fails with:

> `InvalidStateError: The node name 'dsp-processor' is not defined in AudioWorkletGlobalScope`

which reads like a missing worklet file. It is actually a `ReferenceError` in a
polyfill three files away. Bisecting with purpose-built modules was the only way
to see it. Fixed by attaching to `globalThis`.

The worklet is now bundled to a single import-free file regardless — not
because imports were the cause, but because a module-scope throw in a worklet
is this hard to diagnose, and bundling removes a whole class of them.

**2. `import initSync from "dsp-glue"` silently wires up the *async*
initializer.**

The wasm-bindgen glue's default export is `__wbg_init`, the async one; the sync
entry point is the *named* `initSync` export. A default import bundles to the
async path, which returns a promise. The worklet constructor ran straight past
it, the WASM instance was never assigned, and the first DSP call died on
`Cannot read properties of undefined (reading 'dspprocessor_new')`.

Correct form: `import { initSync, DspProcessor } from "dsp-glue"`.

**3. wasm-bindgen `&mut [f32]` out-parameters do not write back to JS.**

This was the big one, and it caused both reported symptoms at once. For
`process(&mut self, out: &mut [f32])` the generated JS is:

```js
process(out) {
    var ptr0 = passArrayF32ToWasm0(out, wasm.__wbindgen_malloc);
    wasm.dspprocessor_process(this.__wbg_ptr, ptr0, WASM_VECTOR_LEN, out);
}
```

`out` is passed as a *return pointer*. WASM writes into its own freshly
allocated buffer; the JS array is never updated. So:

- the worklet played a **stale buffer** — reported as "metallic, sounds like bit
  reduction";
- `spectrum_into` / `waveform_into` were **permanently silent** — reported as
  "no pink waveform visualizer".

Fixed by returning values (`-> Vec<f32>`), which makes wasm-bindgen copy the
data into a real `Float32Array`. Cost: one ~1 KB allocation per 128-frame
quantum (~375/s), far below the worklet's budget. `spectrum()` and `waveform()`
changed the same way.

**Lesson worth keeping:** the build was green and every test passed while the
player emitted garbage. Rust-level unit tests cannot see a broken JS↔WASM
boundary. Anything crossing that boundary needs an end-to-end check that
measures real samples.

### Robustness added as a result

- `ensureContext()` (bare `AudioContext`, for decoding) is now separate from
  `ensureEngine()` (worklet + WASM). Decoding no longer depends on the DSP
  starting, so a worklet failure can never again stop files from appearing in
  the playlist.
- The worklet posts `ready` from its constructor and the main thread waits for
  it with a 5 s timeout, plus `onprocessorerror`. A dead worklet now reports
  itself instead of producing silence.

## Phase 2 — file playback + playlist — **complete**

- `<input type=file multiple>` + whole-window drag/drop → `File[]` → `decodeAudioData`.
- Play/pause/stop/next/prev/play_index/toggle_play through the transport.
- Playlist rows with durations from `AudioBuffer.duration`.
- Keyboard transport (space, Z/X, arrows, 0–9 cue) — shared with desktop.
- Position tracked inside the worklet via the stretcher's own play position, and
  reported every ~33 ms. Never derived from `AudioContext.currentTime`, which is
  wall time and lies at any speed other than 1.0.

**Verified in-browser:** all 10 tracks in `music/` decode with correct durations
(1:20 … 4:30). Playback advances the position, output is a smooth stereo signal
(peak ≈ 0.24, no clipping, L and R distinct), the playlist renders with the
active row highlighted, and the taps arrive at ~30 fps.

## Phase 3 — time/pitch — **complete**

The WSOLA and phase vocoder run in the worklet through the shared `Stretcher`
selector, with the same `stretch = speed / pitch <= 1.0` crossover as desktop.
See the ring-buffer note above — no extra buffering layer was required.

**Verified in-browser:** engine swap across the `speed = 1.0` line at non-zero
pitch produces no non-finite output. Position advances under time-stretch. The
Rust side pins the same behaviour in `stretched_playback_stays_finite_and_advances`
and `engine_crossover_at_speed_one_is_glitch_free`.

## Phase 4 — visualizer taps — **complete**

48-bin FFT spectrum and 226-point echo scope, computed in the worklet post-EQ
(same tap point as desktop), throttled to ~30 fps so the message port is not
flooded at the 375 Hz quantum rate.

**Verified in-browser:** ~35 taps/sec received, non-zero spectrum (0.071) and
waveform (0.153) peaks during playback; both settle to zero when paused
(`paused_processor_reports_zero_spectrum`).

## Phase 5 — hosted MP3s — **complete**

`fetch` → `arrayBuffer()` → `decodeAudioData()`, same pipeline as a local file.
Reached through `window.misima.loadUrl(url, title)`.

**Caveat, confirmed by testing:** a cross-origin track needs
`Access-Control-Allow-Origin` or the fetch fails outright — `Failed to fetch`
with no useful detail. That is the same constraint that rules radio out (§7).
`scripts/serve-music.mjs` is a dev-only CORS server for exercising this path
against the local `music/` library.

## Phase 6 — ship — **in progress**

- [x] Conditional base path. `base` is `/misima-hybrid-player/` only when
      `GH_PAGES=1`; the desktop Tauri build stays at `/`. The spec's §1 said to
      set the subpath unconditionally, which would have broken every asset
      reference in the shipped desktop app — Tauri serves `dist/` from the root
      of its own protocol.
- [x] `.nojekyll`
- [x] `.github/workflows/pages.yml`
- [ ] Configure Pages → Source → GitHub Actions (repository setting, needs a human)
- [ ] First deploy + subpath verification

**Verified:** `npm run build:pages` produces a `dist/` whose HTML references
`/misima-hybrid-player/assets/…`, with `dist/wasm/` (353 KB wasm + 13 KB
bundled worklet) and the skin in place. All four assets return 200 from the
subpath under `vite preview`.
