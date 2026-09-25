# Misima Hybrid Player

A multiplatform skinnable music player featuring a custom organic sprite-based UI, high-performance Rust audio engine with real-time DSP, and live dynamic visualizers.

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-blue.svg)](https://tauri.app/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.8-blue.svg)](https://www.typescriptlang.org/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

---

## Key Features

- **Custom Organic Sprite UI**: No generic OS window frames or HTML form widgets. The interface is composed entirely of hand-drawn transparent PNG plates, interactive knobs, button overlays, and a custom bitmap glyph font engine.
- **Real-Time DSP Engine**:
  - **Bit-Perfect Studio Bypass**: Zero-DSP passthrough at 1.0x speed and 0 semitones pitch shift for 100% original master fidelity.
  - **High-Fidelity WSOLA Time-Stretcher**: Waveform Similarity Overlap-Add with mono-sum cross-correlation phase alignment, preserving natural timbre and stereo coherence without hollow flanging or comb filtering.
  - **Cubic Hermite Pitch-Shifter**: 4-point Catmull-Rom interpolation for smooth, artifact-free pitch adjustments with dynamic 2-pole Butterworth anti-aliasing filter during upward pitch shifts.
  - **10-Band Peaking Equalizer**: High-precision RBJ biquad filters with in-place coefficient updates for click-free adjustment during playback.
  - **Sample-Rate-Scaled Schroeder Reverb**: 4 comb filters and 2 allpass filters tuned to automatically scale with output hardware sample rates (44.1 kHz, 48 kHz, 96 kHz).
  - **Master FX Toggle & Reset**: One-click bypass and reset for all effects.
- **Dynamic Visualizers**:
  - **Organic Raster Spectrum**: 10-band energy visualizer where bands light up irregularly shaped segment chips rather than standard rectangular bars.
  - **Waveform Echo Scope**: 226-point real-time polyline oscilloscope with an 8-frame fading trail and Hann envelope windowing.
- **Multi-Format Playback**: Native decoding of MP3, FLAC, WAV, and OGG via Symphonia with interleaved resampler to hardware output rates.
- **Asynchronous, Glitch-Free Track Switching**: Generation-indexed decode queue (`load_gen`) and DSP buffer flush (`seek_gen`) immediately halt previous playback and eliminate buffer boundary clicks.
- **Multiplatform Architecture**: Built on Tauri 2 for Windows 11, Linux (X11 & Wayland), and macOS.

---

## Technical Stack

| Layer | Technologies | Role |
|---|---|---|
| **Desktop Shell** | [Tauri 2](https://tauri.app/) | Native windowing (transparent, frameless), custom drag regions, system dialogs |
| **Backend Core** | Rust 2021 | Audio pipeline, multi-format decoding, real-time DSP, IPC command handlers |
| **Audio I/O** | [cpal](https://crates.io/crates/cpal) | Cross-platform hardware audio stream management |
| **Audio Decoding** | [Symphonia](https://crates.io/crates/symphonia) | Pure-Rust decoding for MP3, FLAC, WAV, OGG/Vorbis, PCM |
| **DSP & Analysis** | [rustfft](https://crates.io/crates/rustfft), custom biquads | 1024-point FFT spectrum analysis, 10-band peaking EQ, Schroeder reverb |
| **Frontend UI** | TypeScript, Canvas 2D, Vite | 60 FPS sprite compositor, bitmap glyph font, pointer capture fader math |

---

## UI & Coordinate System

- **Artboard Reference**: Coordinates and origins in `skin.json` are expressed in **Photoshop 2x artboard pixels (1500×2060)**.
- **Display Target**: The window renders pixel-perfect at **750×1030 device pixels** on 1080p and 4K displays.
- **Knob Sizing**: Knob and button sprites always render at their natural pixel size to preserve pixel art fidelity.
- **Display Resolution Scaling**: Window resizing dynamically accounts for `window.devicePixelRatio`. DPI changes trigger a canvas refit without page reloads to prevent resetting active audio FX.

### Layout Reference
- **Playlist Area**: `(1028, 1490)`, size `378×310`, 10 visible rows with mouse-wheel scrolling.
- **Status Indicator**: `(1153, 1817)`, width `220`.
- **Echo Scope**: `(911, 180)`, size `226×142`.
- **Bitmap Font Atlas**:
  - Digits (24×24) at origin `(0, 0)`
  - Symbols (24×18) at origin `(0, 72)`
  - Letters (36×18) at origin `(0, 120)`, rows 0–3

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
WSOLA Time-Stretcher (Phase-Aligned Overlap-Add, Speed: 0.5x – 2.0x)
      │
      ▼
Cubic Hermite Resampler + Anti-Alias Lowpass (Pitch: -24.0 – +24.0 st)
      │
      ▼
10-Band Peaking Biquad EQ (60 Hz – 16 kHz)
      │
      ▼
Schroeder Reverb (4 Combs + 2 Allpass, Soft-Clipped Wet Mix)
      │
      ▼
Hardware Output Stream (cpal) ──► Spectrum Analyzer (rustfft) ──► Canvas Visuals
```

### Audio Performance Invariants
1. **Zero Steady-State Allocations**: Processing vectors (`bl`, `br`, `mono_scratch`, `fifo_l`, `fifo_r`) are pre-allocated and reused in the audio callback.
2. **Hoisted Mutex Locks**: Mutex locks (`eq`, `reverb`, `reverb_mix`) are acquired once per callback block instead of per-sample, dropping lock contention from ~3,000 acquisitions/buffer to 2.
3. **Click-Free EQ**: `EqState::set_gains` modifies biquad coefficients in-place while preserving filter delay registers (`z1`, `z2`, `z1r`, `z2r`), preventing pops or clicks when dragging sliders.
4. **Async Race-Free Loading**: `player::prepare_load()` bumps generation counters (`load_gen`, `seek_gen`) and silences the previous track instantly, ensuring rapid track skips never overlap or stutter.

---

## Multiplatform Support

| Platform | Status | Audio Backend | Window / Compositing Notes |
|---|---|---|---|
| **Windows 11** | **Tested & Verified** | WASAPI | Frameless, transparent window, DPI scaling supported |
| **Linux** | **In Progress** | ALSA / PipeWire / PulseAudio | Requires compositing window manager for transparency. Wayland uses `xdg_toplevel.move()` for dragging. |
| **macOS** | **Planned** | CoreAudio | Frameless window, Retina display backing-store handling |

For platform-specific troubleshooting and development guidelines, refer to [`agents.md`](agents.md).

---

## Controls & Interaction

- **Fader Drag**: Click and drag any vertical fader to adjust values.
- **Mouse Wheel on Faders**: Scroll over a fader to adjust (`Shift` + scroll for fine adjustment).
- **UI Zoom / Scaling**:
  - `Ctrl` / `Cmd` + `+` / `=`: Zoom In (Presets: 75% [562×772], 100% [750×1030], 150% [1125×1545], 200% [1500×2060]).
  - `Ctrl` / `Cmd` + `-` / `_`: Zoom Out.
  - `Ctrl` / `Cmd` + `0`: Reset to 100% standard size (750×1030).
  - `Ctrl` / `Cmd` + `D`: Toggle Double Size (200% native 1:1 artboard) vs Standard (100%).
  - `Ctrl` + **Mouse Wheel**: Zoom in / zoom out across scale presets.
  - **Auto-Fit**: Automatically checks available display height on launch; screens under 1050px (e.g. 1080p scaled laptops) automatically open in Compact 75% mode to avoid overflowing off-screen.
- **Playlist Navigation**: Single-click or double-click any track row to play immediately.
- **Playlist Scrolling**: Mouse wheel over the playlist area scrolls through libraries with more than 10 tracks.
- **FX Enable / Bypass**: Toggle master EQ, reverb, and pitch processing on or off.
- **FX Reset**: Resets EQ to 0 dB, reverb to 0%, pitch to 0 semitones, and speed to 1.0x.
- **Power Button**: Clean application shutdown.

---

## Development & Build

### Prerequisites
- Node.js (v20+) and npm
- Rust toolchain (stable) with Cargo (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)
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
```

### Running Tests

```bash
# Execute Rust backend unit and DSP tests (26/26 tests)
cd app/src-tauri
cargo test -- --nocapture

# Typecheck frontend code
cd ../app
npx tsc --noEmit
```

### Building for Release

```bash
cd app
npm run tauri build
```

Installable bundles (MSI/NSIS on Windows, DEB/AppImage on Linux, DMG on macOS) will be generated under `app/src-tauri/target/release/bundle/`.

---

## Project Structure & Agents

For architectural guidelines, subagent definitions, and development invariants, see:
- [`agents.md`](agents.md) — Comprehensive guide for AI subagents (`coder`, `reviewer`, `tester`, `debugger`, `research`, `documenter`) and architectural invariants.
- [`docs/compose/spec/`](docs/compose/spec/) — Historical feature specifications and design decisions.
