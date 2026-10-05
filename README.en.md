# MI$IM∆ hybrid player

[русский](README.md) · english

it all started with Winamp aesthetics. 65,000 skins. digital folklore. creative freedom in its purest form. MI$IM∆ lived in someone else's skin for a while, then built a player of its own. no boring window frames. no html widgets. hand-drawn plates, glowing wireframes, and rust doing real-time dsp underneath the skin. it plays mp3, flac, wav, ogg. it has visualizers. MI$IM∆ lives inside the art.

**[play it in a browser](https://schrab.github.io/misima-hybrid-player/)** — same skin, same DSP, no install.

prefer it native? **[download the latest release](https://github.com/schrab/misima-hybrid-player/releases/latest)** — `.msi` / `.exe` for windows, one universal `.dmg` for macos, and `.deb` / `.rpm` / `.AppImage` for linux. (the macos build is unsigned — the first launch wants a right-click → open; the two steps live in [DEVELOPMENT.md](DEVELOPMENT.md).)

![MI$IM∆ hybrid player — annotated skin legend](docs/misima-hybrid-player-ui-legend.png)

### credits

- the ten tracks bundled with the web build are by **[Wit Chu](https://witchu.bandcamp.com)**, used with his permission. thank you, Anton.
- skin, icons, and everything else on screen: MI$IM∆.
- the reverb is a port of [Mutable Instruments Clouds](https://github.com/pichenettes/eurorack) by Emilie Gillet, who sped off into the sunrise with a capybara on the back seat of her vespa (MIT, © 2014).

[![Rust 2021](https://img.shields.io/badge/Rust-2021-orange.svg)](https://www.rust-lang.org/)
[![Tauri 2](https://img.shields.io/badge/Tauri-2.0-blue.svg)](https://tauri.app/)
[![TypeScript](https://img.shields.io/badge/TypeScript-5.8-blue.svg)](https://www.typescriptlang.org/)
[![License](https://img.shields.io/badge/license-MIT-green.svg)](LICENSE)

MI$IM∆ elsewhere: [Instagram](https://www.instagram.com/misima.gibrid/) · [Telegram](https://t.me/misimahybrid). updates land here first, and skin experiments get posted before they reach the repo.

---

## What it does

- **The skin is the interface**: no os chrome anywhere. the whole player is hand-drawn transparent plates, working knobs and buttons, and a custom bitmap glyph font. every pixel drawn by MI$IM∆.
- **Plays MP3, FLAC, WAV, OGG**: decoded natively, resampled to whatever the output device wants.
- **Bit-Perfect Bypass**: at 1.0x speed and 0 st the dsp steps aside completely. 100% original master. MI$IM∆ respects the master.
- **Speed & Pitch, Separate Faders**: tempo 0.05x–2.0x, pitch ±12 st. below 1.0x the tempo fader hands the stretch to Paulstretch — up to 20x with the pitch kept, in that signature washed-out sound that never goes granular. above 1.0x the two previous engines pick themselves per fader position: a phase vocoder where it runs smooth, a WSOLA time-stretcher where it stays clean. no hollow flanging. no comb filtering. MI$IM∆ tested. MI$IM∆ approves.
- **8-Band Equalizer**: reshape the tone of the sound while it plays — eight bands, from warm lows to airy highs. the two lowest faders it used to have are gone; they are the reverb's shift and tone now.
- **Resonant Master Lowpass**: the volume fader's slot now hosts a 4-pole lowpass (24 dB/oct, a hint of resonance). top of the fader = fully open and bit-transparent; from there the fader glides the signal from clean to fully processed with no loudness dips across the whole range, down to an almost-closed 30 Hz. volume lives in your OS mixer now — MI$IM∆ only shapes tone.
- **Shimmer Reverb**: the reverb is a [Mutable Instruments Clouds](https://github.com/pichenettes/eurorack) FDN with a pitch-shifted feedback cascade wrapped around it, the way Brian Eno and Daniel Lanois built theirs — the tail is fed through a delay-line shifter and back into the reverb's input, so it re-enters an octave up and keeps climbing. the **shift** fader picks the interval (detents at −12 / +7 / +12 / +19 / +24 semitones, +12 = one octave) and the **tone** fader damps the cascade from dark to bright; the reverb mix fader is both the wet/dry balance and how deep the cascade runs, and it sweeps from dry to full wet at matched loudness — no volume dips on the way. this is the whole reverb now — there is no plain-reverb mode.
- **Dynamic Visualizers**: a 10-band spectrum lighting hand-drawn segment chips, sprite-sheet animations, warm beads of light drifting along the wires, and a 226-point echo scope with a fading trail.
- **Glitch-Free Track Switching**: click through the playlist as fast as the mouse can go; decodes never overlap, audio never stutters. (MI$IM∆ learned this one the hard way. see the changelog.)
- **One DSP, Two Runtimes**: the browser build runs the same rust dsp, compiled to wasm.
- **Windows, Linux, macOS — and the browser** (table below).

---

## Controls & Interaction

- **Fader Drag**: click and drag any vertical fader.
- **Mouse Wheel on Faders**: scroll to adjust (`Shift` + scroll for fine adjustment).
- **UI Zoom / Scaling**:
  - `Ctrl` / `Cmd` + `+` / `=`: zoom in (presets: 75% [562×772], 100% [750×1030], 150% [1125×1545], 200% [1500×2060]).
  - `Ctrl` / `Cmd` + `-` / `_`: zoom out.
  - `Ctrl` / `Cmd` + `0`: reset to 100% standard size (750×1030).
  - `Ctrl` / `Cmd` + `D`: toggle double size (200% native 1:1 artboard) vs standard (100%).
  - `Ctrl` + **Mouse Wheel**: zoom across scale presets.
  - **Auto-Fit**: checks available display height on launch; screens under 1050px (e.g. 1080p scaled laptops) open in compact 75% mode so the player does not fall off the screen. (MI$IM∆ has fallen off screens. it is not dignified.)
- **Playlist Navigation**: single-click or double-click any track row to play immediately.
- **Playlist Scrolling**: mouse wheel over the playlist for libraries with more than 10 tracks.
- **Right-Click**: suppressed — the canvas is artwork, so the browser's "Save image as / Inspect" menu is never what you want. Credit links keep their own menu, so they can still be copied or opened in a new tab.
- **Keyboard Cue & Transport** (player window focused, bare keys):
  - `1`…`9`: jump to 10 %…90 % of the current track and start playing from there.
  - `0`: jump back to the start.
  - `Space`: play/pause.
  - `←` / `→`: seek ∓10 seconds.
  - `z` / `x`: previous / next track.
- **FX Enable / Bypass**: toggles master EQ, reverb and pitch processing.
- **FX Reset**: EQ to 0 dB, reverb to 0%, pitch to 0 st, speed to 1.0x, shimmer back to the +12 st / tone detent it ships with.
- **Power Button**: clean application shutdown.

![MI$IM∆ hybrid player — keyboard layout](docs/keyboard_layout.png)

Every key the player listens for, on one board. The transport and cue keys
(`1`…`9`, `0`, `Space`, `←`/`→`, `z`/`x`) are bare; the zoom keys take `Ctrl`
(`Cmd` on macOS) with `-`, `+`, `0` or `D`.

---

## Platforms

| Platform | Status |
|---|---|
| **Windows 11** | **Tested & Verified** |
| **Linux** (X11 & Wayland) | Verified |
| **macOS** | Verified |
| **Browser** | **Shipped** |

---

## Paperwork

- [`DEVELOPMENT.md`](DEVELOPMENT.md) — the technical companion to this page: stack, audio pipeline diagrams, skin coordinate system, build & dev environment, platform notes. everything too heavy for a readme lives there.
- [`AGENTS.md`](AGENTS.md) — architectural guidelines and the agent roster (`coder`, `reviewer`, `tester`, `debugger`, `research`, `documenter`).
- [`docs/DSP.md`](docs/DSP.md) — deep-dive on the audio engines: signal chain, WSOLA vs phase vocoder, reverb topology and loudness policy. read before touching `src/audio/`. MI$IM∆ means it.
- [`docs/WEB-PORT.md`](docs/WEB-PORT.md) — the browser port, phase by phase, including three bugs that passed every automated check.
- [`CHANGELOG.md`](CHANGELOG.md) — milestone history. (scars, documented.)
- [`docs/compose/spec/`](docs/compose/spec/) — historical feature specifications and design decisions.

sleep is for the compiled.
