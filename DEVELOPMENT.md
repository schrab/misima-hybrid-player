# MI$IM∆ hybrid player — development & technical notes

the user-facing readme is [README.md](README.md) ([english version](README.en.md)); it keeps the download links, the hotkeys and the pictures. everything with diagrams, numbers and build steps lives here. MI$IM∆ keeps the paperwork too.

---

## The Web Build

**[schrab.github.io/misima-hybrid-player](https://schrab.github.io/misima-hybrid-player/)** — the whole player, statically hosted, no install.

```
<File> or bundled MP3
        │  decodeAudioData (browser-native; resamples to the context rate)
        ▼
  AudioWorkletNode "dsp-processor"      ← app/wasm-dsp, compiled to WASM + simd128
        │    1. bypass (speed=1.0, pitch=0)   bit-perfect passthrough
        │    2. WSOLA / phase vocoder         stretch = speed / pitch
        │    3. 10-band peaking EQ
        │    4. taps → 48-bin FFT + 226-pt scope
        │    5. FDN reverb                    envelope-normalized wet gain
        │    6. hard-clip + master volume
        ▼
  ctx.destination
       ▲  port.postMessage: params in | spectrum, waveform, position out
       │
  transport.ts ←→ WebTransport | TauriTransport      (one interface, two platforms)
```

- **Same source, not a fork.** `app/wasm-dsp/` includes the eight platform-free modules from `src-tauri/src/audio/` directly. `player.rs` and `decoder.rs` stay desktop-only; the browser equivalents are `AudioContext` and `decodeAudioData`.
- **Why not native Web Audio nodes?** The chain was tuned by ear — the reverb's `TARGET` above 1.0, the `REVERB_TIME` ceiling, the vocoder/WSOLA split at `stretch <= 1`. A rewrite in native nodes would be a second implementation that does not sound the same, and the two would drift apart over time.
- **SIMD is not optional.** `rustfft` falls back to scalar kernels 3–4× slower without it, which is the difference between fitting the 128-sample render quantum and dropping buffers.
- **Autoplay is gated on a user gesture**, as the browser requires. The page renders immediately, loads its bundled track, and starts on the first click where the platform allows it — Chrome blocks it outright on a low-engagement site, and the player says "press play" rather than pretending.
- Full implementation log, including three bugs that passed every automated check: [`docs/WEB-PORT.md`](docs/WEB-PORT.md).

---

## Technical Stack

| Layer | Technologies | Role |
|---|---|---|
| **Desktop Shell** | [Tauri 2](https://tauri.app/) | Native windowing (transparent, frameless), custom drag regions, system dialogs |
| **Backend Core** | Rust 2021 | Audio pipeline, multi-format decoding, real-time DSP, IPC command handlers |
| **Audio I/O** | [cpal](https://crates.io/crates/cpal) | Cross-platform hardware audio stream management |
| **Audio Decoding** | [Symphonia](https://crates.io/crates/symphonia) | Pure-Rust decoding for MP3, FLAC, WAV, OGG/Vorbis, PCM |
| **DSP & Analysis** | [rustfft](https://crates.io/crates/rustfft), custom biquads | 1024-point FFT spectrum analysis, 10-band peaking EQ, Clouds-style feedback-delay reverb |
| **Frontend UI** | TypeScript, Canvas 2D, Vite | 60 FPS sprite compositor, bitmap glyph font, pointer capture fader math |
| **Web Runtime** | [wasm-bindgen](https://rustwasm.github.io/wasm-bindgen/), AudioWorklet | The same DSP modules compiled to WASM (+`simd128`), running on the audio thread of the browser |

---

## UI & Coordinate System

- **Artboard Reference**: everything in `skin.json` is measured in **Photoshop 2x artboard pixels (1500×2060)**. measure twice. place once.
- **Display Target**: pixel-perfect at **750×1030 device pixels** on 1080p and 4K.
- **Knob Sizing**: knob and button sprites render at their natural pixel size. always. the art is pixel art; scaling it is disrespect.
- **Display Resolution Scaling**: resizing accounts for `window.devicePixelRatio`. DPI changes trigger a canvas refit without a page reload (a reload resets active audio fx — MI$IM∆ does not reload).

### Layout Reference
- **Playlist Area**: `(1028, 1490)`, size `378×310`, 10 visible rows with mouse-wheel scrolling. rows read `NN · NAME···· · MM` (2-digit index, up to 6 title glyphs with filename index prefixes stripped, minutes-only duration).
- **Status Indicator**: `(1153, 1817)`, width `220`.
- **Echo Scope**: `(911, 180)`, size `226×142`.
- **Bitmap Font Atlas**:
  - Digits (24×24) at origin `(0, 0)`
  - Symbols (24×18) at origin `(0, 72)` — the symbol band is currently empty art; symbol glyphs render invisible
  - Letters (36×18) at origin `(0, 120)`, rows 0–3

### Skin Layers & Animation (skin.json)
- **`background.overlays[]`**: still png layers composited above the background plate, below all controls (e.g. `bg/UI_highlights.png`). animated regions are erased from the layer art by the artist; the engine draws overlays unmasked. (no engine-side masking. the artist owns the mask.)
- **`animations[]`**: sprite-sheet loops. uniform grid (`grid.cols/rows`), real `frames` count (trailing empty cells allowed), `origin` = top-left of frame 0 on the 2× artboard, optional `size` to scale cells in code (omit = native), `fps`, `blend: "screen"` (default — drops solid black sheet backgrounds), `playback: "always" | "on-playing"`.
- **`visuals.spectrum.bands[].xShift`**: whole-column X nudge (artboard px) applied to every segment of that band at draw time.
- **`visuals.rails`**: `true` (default) runs the wire-rail particles; `false` switches them off for a skin whose artwork has no wires. the rail geometry itself is **not** a skin asset — the 21 paths traced from the artwork are embedded in `app/src/sprite/rails.ts`, so there is nothing to add to the skin folder and no manifest asset list to extend.
- **Spectrum chip set variations**: bands don't share one chip pool — each band's 10 segments can reference any freeform-size chip png (`spectrum/chip_*.png`). the current layout cycles three tuned variants across the columns (`chip_1_*` / `chip_2_*` / default `chip_*`). to retune: adjust one band's segment origins, then clone to the others **anchor-relative** (keep each column's own left edge and `xShift`, copy the variant's Y-stack and X jitter). display energy tilt lives in `BAND_GAIN` (`main.ts`), not in rust. MI$IM∆ checked twice.

---

## Audio Pipeline & Performance

```
Decoded PCM (Symphonia)
      │
      ▼
Interleaved Resampler (Device Rate: 44.1k / 48k / 96k)
      │
      ▼
[Bypass Check: Speed == 1.0 && Pitch == 0.0] ──► (Bit-perfect direct PCM transfer)
      │ (if FX active)
      ▼
Stretcher (Speed: 0.5x – 2.0x, Pitch: ±12 st)
      ├─ stretch ≤ 1 → Stereo Phase Vocoder (pitch-up, smooth partials)
      └─ stretch > 1 → WSOLA Time-Stretcher (tempo + pitch-down)
      │
      ▼
Cubic Hermite Resampler + Anti-Alias Lowpass
      │
      ▼
10-Band Peaking Biquad EQ (60 Hz – 16 kHz)
      │
      ▼
Clouds-Style Stereo Reverb (FDN, Modulated, Envelope-Normalized Wet, Dry→Wet Crossfade)
      │
      ▼
Hardware Output Stream (cpal) ──► Spectrum Analyzer (rustfft) ──► Canvas Visuals
```

the horse said one time-stretcher was enough. the barn overruled. (the barn IS the horse. denial is structural.) both engines have a job: the vocoder is smooth where the wsola goes granular (pitch-up, 4x grain overlap at +1 octave — MI$IM∆ measured), and the wsola is cheap and clean where it expands (tempo, pitch-down). the full reasoning, the failure modes and the numbers live in [`docs/DSP.md`](docs/DSP.md). read it before touching `src/audio/`. MI$IM∆ means it.

### Audio Performance Invariants
1. **Zero Steady-State Allocations**: processing vectors (`bl`, `br`, `mono_scratch`, `fifo_l`, `fifo_r`) are pre-allocated and reused. allocation on the audio thread makes the bones creak. the bones do not creak here.
2. **Hoisted Mutex Locks**: `eq`, `reverb`, `reverb_mix` are acquired once per callback block, not per sample — lock contention drops from ~3,000 acquisitions/buffer to 2.
3. **Click-Free EQ**: `EqState::set_gains` modifies biquad coefficients in-place while preserving the delay registers (`z1`, `z2`, `z1r`, `z2r`). no pops. no clicks. MALLOC SAYS NOTHING, FOR ONCE.
4. **Async Race-Free Loading**: `player::prepare_load()` bumps the generation counters (`load_gen`, `seek_gen`) and silences the previous track instantly. rapid track skips never overlap or stutter.
5. **Saturating WSOLA FIFO Bookkeeping**: the resampler read position can legally run past the FIFO length near track end (overreads are zero-padded); all length arithmetic around `fifo_read_pos` must stay saturating/clamped. an unchecked `usize` underflow here panics the audio thread and kills output. MI$IM∆ warned you. MI$IM∆ always warns you.

---

## Multiplatform Support

| Platform | Status | Audio Backend | Window / Compositing Notes |
|---|---|---|---|
| **Windows 11** | **Tested & Verified** | WASAPI | Frameless, transparent window, DPI scaling supported |
| **Linux** | **In Progress** | ALSA / PipeWire / PulseAudio | Requires compositing window manager for transparency. Wayland uses `xdg_toplevel.move()` for dragging. |
| **macOS** | **Verified (dev)** | CoreAudio | Frameless transparent window (requires `macOSPrivateApi`), Cmd+scroll zoom, 44.1/48 kHz playback |
| **Browser** | **Shipped** | Web Audio + AudioWorklet | Static site on GitHub Pages. No window: zoom is CSS, and the page scales itself to fit the viewport. |

platform-specific troubleshooting and development guidelines: [`AGENTS.md`](AGENTS.md).

---

## Development & Build

### Prerequisites
- Node.js (v20+) and npm
- Rust toolchain (stable) with Cargo (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
- **For the web build only:** the `wasm32-unknown-unknown` target and `wasm-pack`
  ```bash
  rustup target add wasm32-unknown-unknown
  cargo install wasm-pack --locked
  ```
- OS dependencies:
  - **Linux (Debian/Ubuntu)**: `sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev libasound2-dev`
  - **Windows**: WebView2 (pre-installed on Windows 10/11), C++ build tools

### Running in Development

```bash
# Clone the repository
git clone https://github.com/schrab/misima-hybrid-player.git
cd misima-hybrid-player/app

# Install frontend dependencies
npm install

# Launch Tauri development mode (hot-reload for frontend and backend)
npm run tauri dev

# Or serve the web build alone (no Tauri, no wasm rebuild needed after the
# first `npm run build:wasm`)
npm run build:wasm
GH_PAGES=1 npx vite dev
```

### Running Tests

```bash
# Rust desktop backend: unit + DSP tests (43 passed, 1 #[ignore]d smoke test)
cd app/src-tauri
cargo test -- --nocapture

# Live CoreAudio smoke test (requires a real output device; #[ignore]d by default)
cargo test coreaudio_smoke -- --ignored --nocapture

# The web DSP crate — runs the same eight shared modules, no browser needed
cd ../wasm-dsp
cargo test
cargo clippy --target wasm32-unknown-unknown

# Frontend: typecheck + unit tests
cd ../app
npx tsc --noEmit
npm test
```

### Building for Release

```bash
# Desktop installers (MSI/NSIS, DEB/RPM/AppImage, DMG)
cd app
npm run tauri build

# Static site for GitHub Pages: WASM (+simd128) → worklet bundle → typecheck
# → tests → vite build, all under the /misima-hybrid-player/ base path
npm run build:pages
```

Installable bundles land under `app/src-tauri/target/release/bundle/`.

The Pages build is deployed automatically on every push to `main` by
`.github/workflows/pages.yml`. The desktop installers are untouched by that
workflow; they are cut by tagging `v*` (see `release.yml`).

> **Arch Linux:** take the `.pkg.tar.zst`, not the AppImage. The AppImage
> aborts on startup there — it carries support libraries built on Ubuntu 22.04
> that shadow the host's on a rolling distro, and the web process dies on a
> JavaScriptCore assertion before a window appears. The Arch package is
> converted from the `.deb`, which bundles nothing and declares webkit2gtk as a
> dependency, so everything resolves to the host's own libraries.

---

## Installing on macOS (unsigned build)

GitHub Releases carry an unsigned, unnotarized DMG (`Misima Hybrid Player_*_aarch64.dmg`, Apple Silicon, macOS 11+). no Developer ID signature, so Gatekeeper blocks the first launch. to install:

1. Open the DMG and drag `Misima Hybrid Player.app` to Applications.
2. **Right-click (Ctrl-click) the app → Open → Open** in the dialog. this whitelists it permanently; double-click works from then on.

Alternative (Terminal): `xattr -d com.apple.quarantine "/Applications/Misima Hybrid Player.app"`.
