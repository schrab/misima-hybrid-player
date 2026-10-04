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
- [x] Pages → Source → GitHub Actions
- [x] Deployed and verified: **https://schrab.github.io/misima-hybrid-player/**

**Live at 0.4.0.** `npm run build:pages` emits a `dist/` whose HTML references
`/misima-hybrid-player/assets/…`, with `dist/wasm/` (353 KB wasm + 12 KB
bundled worklet), `dist/music/` (28 MB of bundled tracks) and the skin in
place. Every asset returns 200 from the subpath.

---

## After the phases — production hardening

Everything below happened after the first deploy, driven by using the live
site. All four are the same lesson wearing different hats: **the failure mode
was invisible to every automated check**, because the bug lived in the space
between JavaScript and WebAssembly, or between the UI and the network.

### 1. The Linux CI `#[path]` failure

The first Pages build failed on Ubuntu while passing on Windows:

```text
couldn't find file `src/audio/../../../src-tauri/src/audio/clouds_reverb.rs`
```

The shared modules were included through an inline `mod audio { … }` block. An
inline module anchors a nested `#[path]` at `src/audio/` rather than at the
directory of `lib.rs`, so the `..` count is only right under one of the two
plausible readings — and the other points at a path that does not exist.
Windows happened to take the working one.

Each module now sits at the crate root, where the anchor is unambiguous and the
path is two segments instead of three, with a `pub mod audio` shim restoring the
`crate::audio::…` paths. CI now runs `cargo check --target wasm32-unknown-unknown`
in `wasm-dsp/` *before* installing wasm-pack, so this fails in seconds rather
than after a multi-minute toolchain install.

### 2. Blank page in Chrome when autoplay is blocked

Chrome showed no player at all, with only this warning:

> The AudioContext was not allowed to start. It must be resumed (or created)
> after a user gesture on the page.

The warning is a decoy. The real behaviour is that when the autoplay policy
blocks a resume, **Chrome returns a promise that never settles** — not
rejected, pending forever. `ensureContext()` awaited it unconditionally, so
`init()` hung before `requestAnimationFrame(render)` and the canvas was never
drawn. A browser lenient enough to allow autoplay hid the bug completely.

Two independent fixes:

- `resume()` is raced against a 1 s timer, and outside the autoplay probe it is
  only attempted after a real user gesture (a capture-phase listener in
  `main.ts` sets the flag before any click handler can reach `play()`).
- All startup audio work moved to *after* `requestAnimationFrame(render)`.
  Nothing that fetches, decodes or touches an `AudioContext` may sit between
  the canvas becoming visible and the loop that draws it.

### 3. ~30 second blank page

Every sprite was awaited one at a time — 135 references in nested loops, each
waiting on the last. Invisible on localhost; over the network it was the
difference between half a second and half a minute. All 64 distinct files now
race in one `Promise.all`.

Separately, the render loop only started once every image had loaded, so the
canvas was empty for that entire window — and since the track load ran after
it, it looked like the drawing was waiting on the audio. It now starts as soon
as the skin manifest parses and fills in progressively.

A 2 px progress hairline was added, because the background plate alone is
2.24 MB of the skin's 3.6 MB: the wait is real, and without a signal the page
reads as broken rather than loading.

### 4. The bundled music

Four Wit Chu tracks (used with his permission) ship in `app/public/music/` and
are served same-origin. Only the first is fetched on arrival; the rest are
registered as playlist rows carrying a `sourceUrl` and downloaded by
`playIndex` when the listener reaches them — decoding all four up front would
make every page load pay for 28 MB.

Credit line under the player links to the Misima Telegram first, then to Wit
Chu's Bandcamp. Hidden on the desktop build, which ships no bundled music.

### 5. The page backdrop

The tab used to be flat black. It now sits on the Misima artwork at cover fit.

The first version of this shipped a different, much brighter wallpaper and
treated it hard: a 50% black scrim and a 26 px blur, on a `html::before`
pseudo-element, with the box inflated to `inset: -80px` because a blur samples
past its own edge and would otherwise fade the whole border out. Getting there
cost three silent failures worth remembering, none of which produced an error —
the page just looked quietly wrong:

- **Layer order is top-first in CSS**, the opposite of canvas. The scrim was
  listed after the image, and since the image was opaque it buried the scrim
  completely: the page rendered at full brightness.
- **`z-index: -1` is not "behind everything."** It drops behind the *body's own
  background box*, so the fixed pseudo-element carrying the backdrop rendered
  nothing at all. Ordering has to be explicit.
- **`filter: blur()` cannot go on `body`** — it would blur the player canvas
  along with the page, which is what forced the pseudo-element in the first
  place.

The artwork was then swapped for the dark concrete plate, which needs none of
that. It is already dark on its own — mean luminance ~44/255, with a p5-p95
spread of only 41-48 — so a scrim would only crush it and the texture is the
whole point of showing it. Blur went with it. What is left is a plain
`background-image` on `html.web`, and the artist's PNG shipped untouched:
no re-encode, no quality ladder, nothing between the file in `gfx/` and the
page. The three CSS traps above are kept in AGENTS.md 3.2 in case a filter ever
comes back.

Stripped from the desktop build in `vite.config.ts` alongside `wasm/` and
`music/`. The dangling CSS reference is harmless because the rule is gated
behind `html.web`, which the desktop never sets.

### 6. Animated sprites bleeding past the silhouette

Not web-specific — this affected the desktop build equally — but it only became
visible once the page stopped being black.

The `anim/*_sheet.png` cells have an opaque black backdrop (some have no alpha
channel at all). `screen` blending erases black, but only where something is
already painted. Over the plate's transparent gaps there is nothing to lift, so
the black cell survived compositing as a hard square: four of them, around the
top panel's notch, the middle panel's left arm, and twice in the gap above the
bottom panel. Invisible against a dark desktop; glaring against a light one.

The fix is the plate's own alpha. The background and its still overlays are now
flattened once into a `plate` canvas, which doubles as the render blit and as a
mask: each cell is cut to the plate's alpha with `destination-in` before the
blend. Identical wherever the player is solid, absent everywhere it is not.
Worst-case leaked pixels per frame fell from 4,137 to 49 (`ring`), 1,075 to 8
(`form`), 636 to 15 (`cones`).

What is left is the plate's own antialiased rim, where the sprite shows at up to
double the plate's alpha — a few hundred pixels at ≤12% opacity, which reads as
a soft fade along the edge. That is the floor, not a defect.

Clipping to the artboard rect would *not* have worked, and it is the obvious
wrong fix: the silhouette is an irregular shape inside the 1500×2060 canvas, so
a rect clip passes the leak straight through.

### 7. The magenta faders

The render loop starts on the manifest, well before any sprite has loaded, so
anything drawn from `skin.json` alone paints a placeholder into that window.
`drawFader()` fell back to a magenta `#ff4fd8` block when its knob was missing:
every fader flashed a pink slab over the dark page for about half a second and
then vanished. It read as a glitch, not as progress.

It now draws nothing until the knob arrives. An absent fader reads as "not ready
yet"; a coloured one never will.

### Known limits

- **`bg/bg.png` is 2.24 MB.** It is the remaining bottleneck for first paint.
  A WebP would cut it to ~200–300 KB, but that is the artist's asset and it is
  shared with the desktop build, so it has been left alone deliberately.
  (The page backdrop *was* converted — it is web-only, so it does not have that
  constraint.) It adds a further 174 KB, which is not counted by the load
  progress bar and downloads in parallel with the skin.
- **Autoplay depends on the browser's policy.** Chrome blocks it on a
  low-engagement site; the page renders immediately, loads its track, and shows
  "press play" rather than claiming success over silence.
- **Radio remains out of scope.** It needs `Access-Control-Allow-Origin` on the
  stream, and there is no Pages function to add it — see §7 of the spec.
- **`npx tauri build` does not work** on this machine (`could not determine
  executable to run`); use `npm run tauri build`. CI is unaffected — it uses
  `tauri-apps/tauri-action`, not `npx`.
