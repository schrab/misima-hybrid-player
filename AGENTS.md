# Agent System & Engineering Guidelines

This document defines the architecture, subagent roles, technical invariants, and operational guidelines for AI agents and human developers collaborating on the **Misima Hybrid Player**.

---

## 1. Project Overview & Architecture

Misima Hybrid Player is a high-performance, skinnable, multiplatform (Windows, macOS, Linux) audio player built with **Tauri 2**, **Rust** (audio DSP & system integration), and **TypeScript / Canvas 2D** (organic sprite-based UI).

### Architecture Diagram

```
┌────────────────────────────────────────────────────────────────────────┐
│                        Tauri 2 Desktop Shell                           │
│  (Transparent window, decorations disabled, custom hit-testing/drag)  │
├────────────────────────────────────────────────────────────────────────┤
│                     Frontend (TypeScript / Canvas)                     │
│  - Canvas 2D Compositor: 1500×2060 2x artboard rendered at 750×1030     │
│  - Sprite Renderer: background plates, faders, button states           │
│  - Visuals: 10-band raster spectrum strips, 226-pt echo scope, waterfall│
│  - Text: Custom bitmap glyph atlas font engine (digits, symbols, chars)│
│  - Input: Custom pointer capture, fader travel math, playlist scrolling│
├────────────────────────────────────┬───────────────────────────────────┤
│          IPC Commands              │          Events / Taps            │
│  (play, pause, next, prev,         │  (spectrum: 48-bin FFT,           │
│   seek, set_params, open_files)    │   waveform: 226-pt PCM scope,     │
│                                    │   track_ended, play_started)      │
├────────────────────────────────────┴───────────────────────────────────┤
│                     Backend Core (Rust / CPAL / Symphonia)             │
│  - Symphonia: Multi-format synchronous & streaming decoding            │
│  - Resampler: Interleaved SRC to device rate + anti-alias lowpass      │
│  - DSP Pipeline:                                                       │
│      1) Bit-perfect bypass (1.0x speed, 0 st pitch)                     │
│      2) Time/pitch engine, split by stretch = speed/pitch:              │
│         stretch <= 1 → stereo phase vocoder (pitch-up; smooth)          │
│         stretch  > 1 → WSOLA time-stretch (expansion; WSOLA's good side)│
│         then Cubic Hermite resample by the pitch ratio                  │
│      3) 10-Band Peaking EQ (RBJ biquad filters, seamless updates)      │
│      4) Post-EQ visualizer taps (spectrum + waveform)                  │
│      5) Stereo FDN Reverb (Dattorro/Griesinger, Clouds port)            │
│      6) Master 4-pole lowpass (cutoff fader), then output soft-clip    │
│  - Real-time Visualizer Taps (post-EQ):                                │
│      * rustfft 1024-point FFT analyzer → 48 log-spaced energy bins     │
│      * Decimated 226-point mono PCM buffer for echo scope              │
│  - CPAL Output: Low-latency audio stream to hardware device             │
└────────────────────────────────────────────────────────────────────────┘
```

The diagram above is the **desktop** runtime. There is a second one, running
the *same* DSP modules:

```
┌────────────────────────────────────────────────────────────────────────┐
│                    Web Runtime (static, GitHub Pages)                   │
│                                                                        │
│  <File> or bundled MP3                                                 │
│        │  decodeAudioData — browser-native; resamples to the context    │
│        ▼                                                               │
│  AudioWorkletNode "dsp-processor"                                      │
│        │  ← app/wasm-dsp, the nine platform-free modules compiled to  │
│        │    WASM (+simd128). Same bypass / stretcher / EQ / taps /     │
│        │    reverb / clip chain, same order, same tap points.          │
│        ▼                                                               │
│  ctx.destination                                                       │
│        ▲  port.postMessage: params in │ spectrum, waveform, pos out    │
│        │                                                               │
│  transport.ts ←─ WebTransport (browser) │ TauriTransport (desktop)      │
└────────────────────────────────────────────────────────────────────────┘
```

`main.ts` talks to `transport` and never learns which one it got. The worklet
has **no input node**: it reads from a decoded track buffer sent over the
message port, exactly as the desktop reads from `SharedPlay::samples`, so
source position is tracked inside the DSP and never derived from
`AudioContext.currentTime` (which is wall time, and lies at any speed ≠ 1.0).

The real-time lock discipline of §3.1 mostly dissolves in the worklet — it is
single-threaded, so there are no locks to hoist. The allocation rules and the
saturating-arithmetic rules around the WSOLA FIFO apply **verbatim**.

---

## 2. Specialized Subagent Roster

When orchestrating or delegating tasks in this repository, agents operate according to these defined roles:

| Agent Role | Subagent Name | Model / Persona | Primary Scope & Responsibilities |
|---|---|---|---|
| **Coder** | `coder` | Gemini 3.8 Flash | High-volume code generation, DSP implementation, refactoring, and frontend canvas changes. |
| **Reviewer** | `reviewer` | Claude Opus / Gemini | Static analysis, verifying audio thread safety, catching lock contention, linting, and architecture sanity checks. |
| **Tester** | `tester` | Gemini 3.8 Flash | Writing and executing unit/integration tests (`cargo test`), generating test audio signals, validating edge cases. |
| **Debugger** | `debugger` | Gemini 3.8 Flash | Investigating audio artifacts (glitches, clicks, phase distortion), IPC failures, thread deadlocks, and cross-platform issues. |
| **Research** | `research` | Gemini 3.8 Flash | Read-only codebase indexing, searching documentation, exploring external audio/DSP crates. |
| **Documenter** | `documenter` | Gemini 3.8 Flash | Keeping specifications, READMEs, architecture logs, and API definitions up to date. |

---

## 3. Core Invariants & Engineering Rules

All agents modifying code in this codebase **must** adhere to the following strict technical invariants:

### 3.1 Audio Thread & DSP Rules (CRITICAL)

The audio callback runs on a high-priority, real-time thread driven by the OS audio server (CPAL/ALSA/CoreAudio/WASAPI). Any delay or allocation causes audible stuttering, crackling, or dropouts.

1. **Zero Allocations in Hot Paths**:
   - Never call `Vec::new()`, `Vec::reserve()`, or reallocating operations inside the per-frame loop (`for f in 0..frames`).
   - Pre-allocate scratch vectors outside the loop or reuse persistent buffers.
2. **Minimal Mutex Contention (Hoisted Locks)**:
   - **Never acquire or release mutexes per sample**.
   - Acquire locks (`shared.eq.lock()`, `shared.reverb.lock()`, `shared.reverb_mix.lock()`) once per callback buffer, process the block, and release them.
3. **Glitch-Free Filter Updates**:
   - When updating DSP parameters (such as EQ gains or the cutoff fader), update the filter coefficients in-place.
   - **Never wipe delay-line registers (`z1`, `z2`, `z1r`, `z2r`)** during gain adjustments. Doing so causes sharp discontinuities (clicks and pops).
4. **Sample-Rate Scaling**:
   - Reverb delay lengths must scale proportionally with `device_sample_rate / 32,000.0` (the rate Clouds' table is written for). Never assume a fixed rate.
   - Scale the delay-line **offsets as well as the lengths**. `clouds_reverb::layout()` recomputes both per rate; leaving offsets at their 32 kHz positions while lengths grow makes `del1` overrun `del2`, and the loop diverges.
5. **Asynchronous Load & Race-Free Track Changing**:
   - Always call `player::prepare_load()` on the caller thread before initiating a background decode.
   - `prepare_load()` immediately stops previous audio and increments `load_gen`.
   - Background decoder threads must verify that `shared.load_gen == expected_gen` before touching playback buffers; stale decodes must be discarded.
6. **Stream Ownership (cpal::Stream is !Send + !Sync)**:
   - Only the `misima-stream-owner` thread may create, hold, pause, or drop the output stream. Never move it into shared state or statics.
   - Other threads request `Start` / `Shutdown` via the owner channel (`OWNER_TX`); `shutdown()` waits (bounded, 2 s) for `STREAM_LIVE` to clear so CoreAudio HAL teardown completes before process exit.
7. **Saturating WSOLA FIFO Arithmetic**:
   - The WSOLA resampler read position may legally advance past the FIFO length near track end (overreads are zero-padded); all `usize` arithmetic around `fifo_read_pos` must use `saturating_sub` / clamps. An unchecked underflow panics the audio callback thread and silences output (debug builds).
8. **Envelope-Normalized Reverb Wet**:
   - Reverb wet path is envelope-normalized (`Reverb::wet_gain`, level-follower state inside `Reverb`); never replace it with a fixed wet gain — raw tail loudness varies ~20 dB between tonal and broadband material.
   - `TARGET` in `wet_gain()` sits above 1.0 because `mix_reverb_frame`'s `w/√(1+w²)` soft clip costs ~3 dB at `w = 1`; the envelope has to aim above unity to land on the dry level.
9. **Reverb Loop Gain**:
   - `CloudsReverb::write` stores the accumulator **unscaled** and returns it scaled; the scale applies to the running accumulator only. Scaling the stored sample as well doubles the feedback gain and diverges the loop to NaN.
   - `REVERB_TIME` (loop gain) sets the tail's RT60 and has a stability cliff: ~0.6 is the practical ceiling, 0.65 already rings for 10 s, and past 0.9 the loop builds up instead of decaying. The original caps its internal reverb amount at 0.54 so `krt` never exceeds 0.69 — do not map a 0..1 fader onto `0.35 + 0.63 * amount` (that reaches 0.98 and diverges). `tail_decays_in_a_musical_time` guards this.
10. **Master Lowpass Bypass & Flush** (`lpf.rs`, the cutoff fader):
   - At `cutoff ≥ 20 kHz` the `Lowpass4` becomes identity **and clears its registers**, entered exactly once — the fader's top position must stay bit-transparent, and a real 20 kHz lowpass would not be. Never clear per buffer while open.
   - `set_cutoff` is an epsilon no-op when cutoff and rate are unchanged; `set_params` fires on every pointermove of any fader drag. The desktop callback re-applies it per buffer with the live device rate (a device change rebuilds the coefficients); the worklet applies it in `set_params`.
   - A seek or track change flushes the filter alongside the stretcher and the reverb on the `seek_gen` path, or a closed filter rings across the gap.
11. **Never Force a Binned Coefficient's Sign** (`phase_vocoder.rs`):
   - The resynthesised DC bin may only have its imaginary rounding cleared. Writing `spec[0].re = spec[0].norm()` takes the magnitude, so a negative analysis DC (ordinary in music) flips by π and injects a constant `2·|X0|` into the whole frame. Steady state buries it; the first frame after a `reset()` divides by a partial sum of squared windows instead of 1.5, multiplying that constant by `1/(N·w[i])` where a Hann window approaches zero — a full-scale burst on every vocoder reset, which the reverb tail then rings for seconds. Sines never showed it (their windowed DC is ~0), so the regression tests must use broadband material.

### 3.2 Frontend & UI Compositor Rules

1. **Coordinate System**:
   - The UI is designed on a **1500×2060 2x Photoshop artboard**, rendered at **750×1030 device pixels**.
   - All positions in `skin.json` are in 2x artboard pixels.
2. **Knob & Sprite Sizing**:
   - Knobs and sprites must use natural pixel dimensions; never scale knob images in code.
   - Fader movement is strictly vertical along defined travel distances.
3. **No Reload on DPI Change**:
   - Never reload the webview on DPI/resolution changes (this wipes dynamic UI/FX state). Refit canvas dimensions via `fitPixelPerfect()`.
4. **Clamping & Visual Feedback**:
   - Spectrum segments light up organically based on energy thresholds (`reveal: 0.0..1.0`). If energy is near zero (`<= 0.01`), do not render idle visualizer noise.
5. **Per-Band Spectrum Chip Sets**:
   - Each band may use its own chip set (`spectrum/chip_*.png`, freeform sizes); never normalize bands onto one shared pool. Clone a tuned band's layout anchor-relative: keep every column's own left edge / `xShift`, copy only the variant's Y-stack and per-chip X jitter.
6. **Web build: nothing may block the first paint**:
   - `AudioContext`, `fetch`, `decodeAudioData` and WASM compilation must never run between `requestAnimationFrame(render)` and the canvas being drawn. `init()` starts the render loop as soon as the skin manifest parses and does its audio work afterwards.
   - Chrome's `AudioContext.resume()` **never settles** when the autoplay policy blocks it — not rejected, just pending forever. Always race it against a timeout, and only call it after a real user gesture (tracked by a capture-phase listener in `main.ts`).
   - Load sprites in one `Promise.all`. Awaiting them in turn was ~135 serialised requests; invisible on localhost, half a minute over the network.
7. **wasm-bindgen out-parameters do not write back**:
   - A `&mut [f32]` argument is compiled as a **return pointer**: the generated JS hands the array to WASM and never copies the result back, so the caller's array is untouched. Return `Vec<f32>` instead. Getting this wrong plays a stale buffer (audible as metallic, bit-reduced audio) and silently kills the visualizer taps.
8. **Blend-mode sprites must be masked by the plate's alpha**:
   - The `anim/*_sheet.png` cells carry an **opaque black backdrop** — some have no alpha channel at all. `screen` erases black, but only where something is already painted: over the plate's transparent gaps the blend has nothing to lift and the cell survives as a hard black square. Invisible against a dark wallpaper, glaring against a light one.
   - Therefore every cell is cut to the plate's alpha with `destination-in` before it is composited, and the background + still overlays are flattened once into a `plate` canvas that serves as both the render blit and the mask source. The mask is a no-op wherever the player is solid, so this costs one `clearRect` plus two small `drawImage` calls per animation per frame.
   - Do **not** mask by clipping to the artboard rect. The player's silhouette is an irregular shape *inside* the 1500×2060 canvas, so a rect clip passes the leak straight through.
   - The residual is the plate's own antialiased rim (alpha 1–32), where the sprite shows at up to double the plate's alpha — a few hundred pixels at ≤12% opacity, which reads as a soft fade. That is the expected floor, not a bug to chase.
9. **No debug placeholder may render before its sprite arrives**:
   - The render loop starts on the manifest, long before the art. Anything drawn from `skin.json` alone — fader knobs above all — paints a placeholder into that window. `drawFader()` used to fall back to a magenta block, which flashed a pink slab over the dark page for half a second and read as a glitch. Draw nothing until the image lands; absence reads as "not ready", a coloured block does not.
10. **Web-only page furniture**:
   - The backdrop is a plain `background-image` on `html.web` at cover fit, shipped as the artist's PNG untouched — no scrim, no blur, no re-encode. The artwork is already dark (mean luminance ~44/255); anything laid over it only crushes it.
   - Keep it that way if it ever needs a `filter` back: `blur()` is a filter, so on `body` it would blur the player canvas too, and it forces the pseudo-element plus the `inset` inflation — a blur samples past its own edge and fades the border out. CSS also paints the **first** background layer on *top*, the opposite of canvas, so a scrim listed after an opaque image is buried by it.
   - Web-only assets go in the desktop strip in `vite.config.ts` alongside `wasm/` and `music/`. Referencing a stripped file from CSS is safe only while the rule stays gated behind `html.web`, which the desktop never sets.

11. **Wire-rail particles are code, not skin assets** (`app/src/sprite/rails.ts`):
   - The 21 rail paths traced from `gfx/bg_wires.svg` are embedded in TypeScript, not shipped in `public/sprite/`. They belong to one piece of artwork and need no zip entry or manifest asset list. `skin.json` carries only a `visuals.rails` boolean so a skin without wires can switch the flow off.
   - **They deliberately skip the plate-alpha mask of rule 8.** Every rail was checked against the plate alpha over 3,793 sample points and all 21 lie *entirely* inside the player silhouette, so a plain `screen` draw over the plate cannot leak past the edge. If a future skin's rails ever cross the silhouette, that mask has to be added — a bead over a transparent gap is exactly the hard bright square rule 8 exists to prevent.
   - Rails are flattened to polylines and addressed by **arc length**, never by raw curve parameter `t`. `t` is not proportional to distance, so sampling it makes a bead surge through every tight corner. `railAt` binary-searches the cumulative-length table for this reason.
   - The fade envelope derives alpha from **position**, not from a spawn timer. That is what makes a bead fade in while already moving and reach zero opacity strictly before the rail end, instead of arriving, stopping, and dissolving. Both sides of the wrap are transparent, so the seam is invisible.
   - Beads share one pre-rendered radial-gradient sprite and differ only by `globalAlpha` and size; never build a gradient per particle per frame. The profile is a true **Gaussian**, sampled into stops — a hand-picked stop list kinks, and one that still carries alpha where the sprite ends shows as a hard disc. Widening the blur therefore means growing `SPOT_SCALE` *and* `SPOT_SIGMA` together, never one alone. The tint is warm off-white (255,222,150): a pure white bead reads as a speck of dust rather than a light, and `gain` stays low (0.6) for the same reason. Note that `screen` compounds where neighbouring beads' halos overlap, so bundle corridors do reach pure white even though a single isolated bead peaks near 190.

12. **Never use `ctx.filter` — canvas filters are not Baseline.**
   - MDN's compatibility data has `ctx.filter` in Chrome 52 and Firefox 49, but in Safari only behind the "Canvas Filters" preference, which WebKitGTK does not enable. Assigning it there is a **silent no-op**, not a throw — so a build that depends on it looks correct on Windows and macOS and silently does nothing on Linux. This bit the selected playlist row, which dimmed its glyphs with `brightness(0.12)`.
   - `sprite/font.ts` pre-builds a darkened atlas with `source-atop` instead. That is the operation to reach for, not a plain `fillRect`: `source-atop` confines the fill to pixels that already exist and **preserves their alpha**, which is exactly what `brightness()` does (scale colour, leave shape). A plain fill would also make the glyphs translucent, and `multiply` cannot express this without also darkening the backdrop. Cost is one canvas at load time and an extra `drawImage` source, not per-frame work.
   - Verified equivalent on Chromium: max channel difference 1/255, mean 0.29, against the filter it replaced.

---

## 4. Multiplatform Guidelines (Windows / Linux / macOS)

The player is designed for cross-platform deployment. Agents must verify platform-specific considerations:

### Windows (Tested)
- DPI awareness handles physical window sizing cleanly.
- Native dialog and WASAPI output work out-of-the-box.
- Decorations are disabled (`decorations: false`), transparency is supported.

### Linux (In Progress)
- **Audio Servers**: Linux environments may use ALSA, PulseAudio, or PipeWire + WirePlumber. PipeWire may default to 48 kHz, 96 kHz, or 192 kHz. Resampling must handle non-44.1k cleanly.
- **Window Transparency**: Transparent windows require a running compositor (Picom, Wayland compositor, etc.). On X11 without a compositor, the window background will appear black.
- **Window Dragging**: Under Wayland, `startDragging()` relies on `xdg_toplevel.move()` and must strictly originate from an active pointer event with a valid serial.
- **Display Scaling**: Watch for fractional scaling (125%, 150%) differences between logical and physical pixels.

### macOS (Future)
- **CoreAudio**: Devices typically operate at 44.1 kHz or 48 kHz.
- **High-DPI / Retina**: Window size represents points (logical), but canvas requires proper backing store multiplier.
- **Teardown**: Proper stream teardown is essential to prevent CoreAudio HAL resource leaks.

---

## 5. Verification & Testing Protocol

Before committing or completing any task, agents must run and pass the following checks:

```bash
# 1. Rust Audio Core & DSP Unit Tests (Must pass 51/51, 1 ignored smoke test)
cd app/src-tauri
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -- --nocapture

# 2. Rust Lints & Compilation Check (Zero warnings)
cargo check

# 3. Web DSP Crate — same shared modules, wasm32 target
#    The unit tests of the nine shared DSP modules run here too, so a
#    regression that only shows up in the WASM build is caught without a
#    browser. This is mandatory for any change under `src/audio/`.
cd ../wasm-dsp
cargo test
cargo clippy --target wasm32-unknown-unknown

# 4. Frontend Tests, Typecheck & Production Build
#    `npm run build` = `tsc` (typecheck, noEmit) → `npm test` → `vite build`.
#    A failing frontend test fails the build, so this step is not optional.
cd ../app
npm test
npm run build

# 5. Pages Build (only when touching the web build)
#    Builds WASM with simd128, bundles the worklet, then runs the same gate as
#    step 4. Requires `wasm-pack` on PATH (`cargo install wasm-pack --locked`).
npm run build:pages
```

Frontend tests are plain `.ts` files under `src/` run by `tsx` (no test-runner
framework). They are typechecked by `tsc` and executed by `npm test`, but are
never bundled into `dist/` — Vite only follows imports reachable from
`index.html`. Add new tests as `src/**/*.test.ts` and register them in the
`test` script in `app/package.json`.

---

## 6. Directory Map

```
misima-hybrid-player/
├── .agents/                    # Agent specifications and team settings
│   ├── settings.json           # Model selection and subagent configuration
│   └── agents/                 # Role definitions (coder, reviewer, tester, etc.)
├── AGENTS.md                   # This instruction manual
├── README.md                   # User-facing and developer documentation
├── .nojekyll                   # GitHub Pages: serve dist/ verbatim, no Jekyll
├── docs/                       # Specifications and architectural history
│   ├── DSP.md                  # Audio engine deep-dive (read before src/audio/)
│   ├── WEB-PORT.md             # Web port implementation log, phase by phase
│   └── compose/spec/           # Feature specifications (MVP, sprite UI, web app)
├── app/
│   ├── index.html              # HTML shell containing the main canvas element
│   ├── package.json            # Node.js dependencies (Tauri 2, Vite, TypeScript, esbuild)
│   ├── vite.config.ts          # Base path, dev server, and the web-only asset strip
│   ├── scripts/                # Build helpers (wasm-pack, worklet bundler, dev servers)
│   ├── src/                    # Frontend source code
│   │   ├── main.ts             # Main event loop, input handling, transport calls
│   │   ├── transport.ts        # Transport interface shared by both platforms
│   │   ├── transportTauri.ts   # Desktop implementation (wraps invoke/listen)
│   │   ├── transportWeb.ts     # Browser implementation (wraps WebPlayer)
│   │   ├── web/                # Browser-only engine
│   │   │   ├── player.ts       # AudioContext lifecycle, decoded-track playlist, worklet wiring
│   │   │   ├── files.ts        # <input type=file> + whole-window drag-and-drop
│   │   │   └── base.ts         # import.meta.env.BASE_URL with a non-Vite fallback
│   │   ├── worklet/            # AudioWorklet source (bundled, not shipped raw)
│   │   │   ├── dspWorklet.js   # The "dsp-processor" node and its message protocol
│   │   │   └── polyfill.js     # TextDecoder/TextEncoder for AudioWorkletGlobalScope
│   │   └── sprite/             # Sprite compositor, font engine, layout math
│   │       ├── font.ts         # Bitmap glyph font renderer
│   │       ├── layout.ts       # Fader hit-testing and travel calculation
│   │       ├── rails.ts        # Wire-rail light particles: path parser + flow
│   │       ├── spectrumLayout.ts # Organic spectrum segment stacker
│   │       ├── types.ts        # Skin and layout TypeScript interfaces
│   │       └── visuals.ts      # Spectrum segments and waterfall drawing
│   ├── public/
│   │   ├── sprite/             # THE skin folder (single source of truth):
│   │   │                       #   skin.json + bg/ ui/ font/ spectrum/ anim/
│   │   ├── music/              # Bundled demo tracks (web build only, stripped from desktop)
│   │   ├── misima-background.webp # Page backdrop (web only, stripped from desktop)
│   │   └── wasm/               # GENERATED: wasm-pack output + bundled worklet
│   ├── wasm-dsp/               # The DSP chain compiled to WASM for the browser
│   │   ├── Cargo.toml          # wasm-bindgen, js-sys, rustfft — no cpal/tauri/symphonia
│   │   └── src/
│   │       ├── lib.rs          # #[path]-includes the nine shared audio modules
│   │       ├── processor.rs    # DspProcessor: the whole worklet-side engine
│   │       └── bindings.rs     # wasm_bindgen surface (wasm32 only)
│   └── src-tauri/              # Rust backend core (desktop)
│       ├── Cargo.toml          # Rust dependencies (cpal, symphonia, rustfft, tauri)
│       ├── tauri.conf.json     # Window, bundle, and capability configuration
│       └── src/
│           ├── lib.rs          # Tauri application entry point and command registration
│           ├── commands.rs     # IPC command handlers (play, pause, next, seek, etc.)
│           ├── playlist.rs     # Playlist state, track metadata, and reordering
│           ├── skin.rs         # Skin zip archive parser and validator
│           └── audio/          # Real-time audio engine
│               ├── clouds_reverb.rs # Stereo FDN reverb (Clouds port, MIT © Emilie Gillet)
│               ├── decoder.rs  # Symphonia multi-format audio decoder
│               ├── dsp_utils.rs # Shared interpolation and buffer readers
│               ├── eq.rs       # 10-band peaking biquad EQ & anti-aliasing lowpass
│               ├── lpf.rs      # 4-pole resonant master lowpass (cutoff fader)
│               ├── phase_vocoder.rs # Stereo phase vocoder (pitch-up engine)
│               ├── reverb_mix.rs # Reverb dry/wet balance + envelope gain policy
│               ├── spectrum.rs # FFT spectrum analyzer (rustfft)
│               ├── stretcher.rs # Engine selector between vocoder and WSOLA
│               ├── player.rs   # Playback state, cpal callback, SharedPlay (desktop-only)
│               └── wsola.rs    # Real-time WSOLA time-stretcher (tempo + pitch-down)
```

### Shared DSP modules

Eight modules under `src-tauri/src/audio/` are `#[path]`-included by
`wasm-dsp`, so the browser and desktop run **the same Rust**:

`clouds_reverb`, `dsp_utils`, `eq`, `phase_vocoder`, `spectrum`, `wsola`,
`reverb_mix`, `stretcher`.

Their unit tests run in **both** crates. `player.rs` (cpal, tauri, parking_lot)
and `decoder.rs` (Symphonia) stay desktop-only — the browser equivalents are
the `AudioContext` and `decodeAudioData`.

When editing one of the nine, the shared module's `use crate::audio::…` paths
must keep resolving in both crates. Declare new modules at the crate root of
`wasm-dsp/src/lib.rs`, not inside an inline `mod audio { … }`: an inline
module anchors a nested `#[path]` at `src/audio/`, and the resulting `..` count
only resolves under one reading — which works on Windows and fails on the
Linux CI runner.

## 7. DSP Documentation

`docs/DSP.md` is the deep-dive for anyone reviewing or modifying the audio
engines: the full signal chain, both time-stretch engines (WSOLA and the phase
vocoder) with their measured trade-offs, the reverb topology and loudness
policy, and the test methodology (including the identity-bypass trick for
separating overlap-add faults from phase-logic faults). Read it before touching
anything under `src/audio/`.

