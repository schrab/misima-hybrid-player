# Misima Hybrid Player

Cross-platform skinnable music player (Windows / macOS / Linux) inspired by classic Winamp skinning, built for a non-rectangular organic UI.

## Stack

| Layer | Choice |
|-------|--------|
| Shell | Tauri 2 |
| Audio decode | Symphonia (mp3/flac/wav/ogg) |
| Output | cpal |
| EQ | 10-band RBJ biquads |
| Spectrum | rustfft → log-spaced bins |
| UI | HTML/CSS/Canvas (clip-path silhouettes) |

## Repository layout

```
app/                 Tauri player
skins/
  misima-hybrid/     native skin source (ZIP + PNG + skin.json)
  misima-hybrid.mskin
  winamp5-misima/    Winamp 5 freeform modern skin source
  winamp5-misima.wal
gfx/UI_sketch.PNG
docs/compose/spec/
```

## Prerequisites

- Node.js 20+
- Rust stable (MSVC on Windows)
- WebView2 runtime (preinstalled on Win 10/11)

## Develop

```powershell
cd app
npm install
npm run tauri dev
```

## Test

```powershell
cd app/src-tauri
cargo test --lib
```

## Build

```powershell
cd app
npm run tauri build
```

## Native skin format (v2 — sprite UI)

A skin is a ZIP (`.mskin`) containing master **background PNGs**, **knob/button sprites**, **spectrum cell sheet**, **glyph atlas**, and `skin.json` with **absolute canvas coordinates**.

### Artist workflow

1. Paint a **master artboard** at `canvas.width × canvas.height` (e.g. 1280×980). Transparent outside organic plates.
2. Export plates: `bg/visualizer.png`, `bg/eq.png`, `bg/playlist.png` (or one full plate).
3. Export sprites: `ui/knob_magenta.png`, `ui/btn_normal.png`, `ui/btn_pressed.png`.
4. Export **spectrum sheet**: 10 columns × 12 energy rows of cell rasters (`cell` 64×12 in placeholder).
5. Export **glyph atlas** PNG + map (10×18 cells). **No TTF required** — raster only is supported and preferred.
6. Measure pixel **origins** (fader knob top of travel, button top-left, text baselines) and fill `skin.json`.
7. `python scripts/make_sprite_kit.py` (placeholders) or your pack script → `.mskin`.
8. Drag faders in the player to verify hit boxes; tweak JSON integers.

### Fader row (left → right)

`volume` · `pitch` · `reverb` · `eq0`…`eq9` · `speed`

Positions are defined **only** by your background art + JSON `origin`/`travel`.

### DSP

| Param | Control | Engine |
|-------|---------|--------|
| volume | fader | gain |
| pitch | fader ±12 st | rate = 2^(st/12) × speed |
| reverb | fader 0..1 | Schroeder mix |
| eq0..9 | faders ±12 dB | RBJ peaking |
| speed | fader 0.5..2× | playback rate (tape-style: pitch+tempo together) |

Generative UI: 10-band raster spectrum + phase/3D waterfall (canvas, clipped to JSON rects).

```json
{
  "formatVersion": 2,
  "canvas": { "width": 1280, "height": 980 },
  "faders": [
    { "id": "volume", "param": "volume", "origin": { "x": 220, "y": 520 }, "travel": 120, "range": [0, 1], "value": 0.8, "knob": "ui/knob_magenta.png" }
  ]
}
```

See `docs/compose/spec/sprite-skin-ui.md` for the full schema.


## Winamp deliverable note

Classic Winamp 2 `.wsz` skins are BMP + color-key and **cannot** express freeform alpha shapes. Freeform skins from the sketch are authored as **Winamp 5 modern `.wal`** (`skin.xml` + PNG). See `skins/winamp5-misima/README.md`.

## Graphics formats needed

| Target | Format | Notes |
|--------|--------|-------|
| Native player skin | PNG/WebP + alpha | Organic silhouettes, 1x or 2x |
| Winamp 5 freeform | PNG + alpha in `.wal` | No color key |
| Sketch reference | PNG | Source art only |

Export plate art at 2× then downscale; keep control hit targets ≥ 28×28 px.
