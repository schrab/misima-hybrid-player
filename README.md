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
| UI | Canvas sprite compositor (PNG plates + JSON anchors + bitmap font) |

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

### Coordinate system (2× artboard)

| Rule | Value |
|------|--------|
| Artboard | `bg/bg.png` = **1500 × 2060** (Photoshop pixels) |
| JSON units | **Same as Photoshop Info** — use the numbers you see, **do not divide by 2** |
| `canvas` | `{ "width": 1500, "height": 2060, "scale": 2 }` |
| Origin of canvas | **Top-left** of `bg.png` = (0, 0) |

**What to measure (Photoshop → layer bounds top-left):**

| Control | `origin` means | `travel` / `size` |
|---------|----------------|-------------------|
| **Knob** (vertical fader) | **Top-left of the knob PNG at MAXIMUM value** (cap at the top of its slot) | `travel` = Y of top-left at **MINIMUM** value − Y at max (always positive; slides downward as value falls) |
| **Button** | **Top-left of the button PNG** — same rectangle as the idle art already in `bg.png` | `size` = that PNG’s W×H (auto-read from file) |
| **Spectrum / waterfall / text** | **Top-left** of the draw rectangle / first baseline row | `size` / `rowHeight` in artboard px |

Draw math (vertical):

```text
knobY(value) = origin.y + (1 - normalized) * travel
x stays origin.x
```

Per-control **knob sizes and travels are independent** (your PNGs already differ; JSON allows different `travel` per fader).

Buttons: **idle state is only in `bg.png`**. Separate `button_*.png` are **active/pressed overlays** at the same `origin`.

### Spectrum (irregular segments)

Not a rectangular grid. Each of **10 bands** is a stack of **hand-drawn PNG segments** (any silhouette):

```json
"visuals": {
  "spectrum": {
    "mode": "segments",
    "bands": [
      {
        "id": 0,
        "segments": [
          { "image": "spectrum/band0_0.png", "origin": { "x": 420, "y": 520 } },
          { "image": "spectrum/band0_1.png", "origin": { "x": 418, "y": 500 } }
        ]
      }
    ]
  }
}
```

- `segments[0]` = **bottom** piece (first to light).
- `origin` = **top-left of that segment PNG** on the 2× artboard.
- Band **heights can differ** — just use more/fewer segments per band.
- Engine reveals count ≈ `energy * segments.length` from the bottom.

### Font (variable metrics)

| Class | Typical | Aspect |
|-------|---------|--------|
| `digit` | 18×18 | ~1:1 square |
| `letter` | 36×14 | **2:1**, **shorter than digits** |
| `symbol` | 18×14 | shorter |

- Glyphs are **bottom-aligned** on a shared baseline (shorter letters sit with digits).
- `map` entries: `{ "col", "row", "class": "letter" }` index that class’s atlas grid.
- Optional `atlasOrigin` per class so one PNG can hold all classes.

```json
"font": {
  "atlas": "font/glyphs.png",
  "cell": { "w": 18, "h": 18 },
  "classes": {
    "digit":  { "cell": { "w": 18, "h": 18 }, "atlasOrigin": { "x": 0, "y": 0 } },
    "letter": { "cell": { "w": 36, "h": 14 }, "atlasOrigin": { "x": 0, "y": 80 } }
  },
  "map": {
    "0": { "col": 0, "row": 0, "class": "digit" },
    "A": { "col": 0, "row": 0, "class": "letter" }
  }
}
```

### Fader row (left → right)

`volume` · `pitch` · `reverb` · `eq1`…`eq10` · `tempo` (speed)

Filnames: `knob_volume.png`, `knob_pitch.png`, `knob_reverb.png`, `knob_eq_1.png`…`knob_eq_10.png`, `knob_tempo.png`.


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
