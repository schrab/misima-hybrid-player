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
│  - Resampler: Interleaved sample-rate conversion to device rate        │
│  - DSP Pipeline:                                                       │
│      1) OLA Time-Stretch (speed/tempo independent of pitch)            │
│      2) OLA Pitch-Shift (tone transposition independent of tempo)      │
│      3) 10-Band Peaking EQ (RBJ biquad filters, seamless updates)      │
│      4) Schroeder Reverb (4 comb + 2 allpass, sample-rate scaled)      │
│      5) Output soft-clipping & master volume attenuation               │
│  - Real-time Visualizer Taps:                                          │
│      * rustfft 1024-point FFT analyzer → 48 log-spaced energy bins     │
│      * Decimated 226-point mono PCM buffer for echo scope              │
│  - CPAL Output: Low-latency audio stream to hardware device             │
└────────────────────────────────────────────────────────────────────────┘
```

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
   - When updating DSP parameters (such as EQ gains or volume), update the filter coefficients in-place.
   - **Never wipe delay-line registers (`z1`, `z2`, `z1r`, `z2r`)** during gain adjustments. Doing so causes sharp discontinuities (clicks and pops).
4. **Sample-Rate Scaling**:
   - Reverb delay lines (comb filter lengths and allpass buffers) must scale proportionally with `device_sample_rate / 44,100.0`. Never assume fixed 44.1 kHz.
5. **Asynchronous Load & Race-Free Track Changing**:
   - Always call `player::prepare_load()` on the caller thread before initiating a background decode.
   - `prepare_load()` immediately stops previous audio and increments `load_gen`.
   - Background decoder threads must verify that `shared.load_gen == expected_gen` before touching playback buffers; stale decodes must be discarded.

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
# 1. Rust Audio Core & DSP Unit Tests (Must pass 20/20)
cd app/src-tauri
export PATH="$HOME/.cargo/bin:$PATH"
cargo test -- --nocapture

# 2. Rust Lints & Compilation Check (Zero warnings)
cargo check

# 3. TypeScript Typecheck & Frontend Build
cd ../app
npm run build
```

---

## 6. Directory Map

```
misima-hybrid-player/
├── .agents/                    # Agent specifications and team settings
│   ├── settings.json           # Model selection and subagent configuration
│   └── agents/                 # Role definitions (coder, reviewer, tester, etc.)
├── agents.md                   # This instruction manual
├── README.md                   # User-facing and developer documentation
├── docs/                       # Specifications and architectural history
│   └── compose/spec/           # Feature specifications (MVP, sprite UI)
├── app/
│   ├── index.html              # HTML shell containing the main canvas element
│   ├── package.json            # Node.js dependencies (Tauri 2, Vite, TypeScript)
│   ├── src/                    # Frontend source code
│   │   ├── main.ts             # Main event loop, input handling, IPC bindings
│   │   └── sprite/             # Sprite compositor, font engine, layout math
│   │       ├── font.ts         # Bitmap glyph font renderer
│   │       ├── layout.ts       # Fader hit-testing and travel calculation
│   │       ├── spectrumLayout.ts # Organic spectrum segment stacker
│   │       ├── types.ts        # Skin and layout TypeScript interfaces
│   │       └── visuals.ts      # Spectrum segments and waterfall drawing
│   └── src-tauri/              # Rust backend core
│       ├── Cargo.toml          # Rust dependencies (cpal, symphonia, rustfft, tauri)
│       ├── tauri.conf.json     # Window, bundle, and capability configuration
│       └── src/
│           ├── lib.rs          # Tauri application entry point and command registration
│           ├── commands.rs     # IPC command handlers (play, pause, next, seek, etc.)
│           ├── playlist.rs     # Playlist state, track metadata, and reordering
│           ├── skin.rs         # Skin zip archive parser and validator
│           └── audio/          # Real-time audio engine
│               ├── decoder.rs  # Symphonia multi-format audio decoder
│               ├── eq.rs       # 10-band peaking biquad EQ
│               ├── player.rs   # Playback state, cpal audio callback, OLA, Reverb
│               └── spectrum.rs # FFT spectrum analyzer (rustfft)
└── skins/                      # Raw skin sprite source assets
```
