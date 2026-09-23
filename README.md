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

### Spectrum — prefilled band geometry

Artist measured (2× artboard):

| Field | Value |
|-------|--------|
| Band **left-border X** | `370, 411, 451, 496, 546, 602, 655, 703, 750, 805` |
| Shared **bottom Y** | `585` |

You do **not** need 100 unique PNGs. Export **10 unique chips** — one per **stack level** (`chip_0` = bottom … `chip_9` = top). Each of the 10 bands reuses the same 10 pieces at its own X:

```json
"spectrum": {
  "mode": "segments",
  "auto": {
    "bandLeftX": [370, 411, 451, 496, 546, 602, 655, 703, 750, 805],
    "bottomY": 585,
    "maxHeight": 240,
    "segmentsPerBand": 10,
    "overlap": 0.4,
    "chips": ["spectrum/chip_0.png", "…", "spectrum/chip_9.png"]
  },
  "bands": []
}
```

Runtime (or `make_sprite_kit.py`) expands `auto` → `bands[]` with absolute origins and `reveal` thresholds. Pieces may overlap; sizes may differ.

**Manual mode:** fill `bands[].segments[]` yourself with `{ image, origin, reveal }` if you want full artistic control per piece.

### Playlist text box

| | |
|--|--|
| Top-left | **1024, 1490** |
| Bottom-right | **1402, 1800** |
| Size | **378 × 310** |
| Rows | **10** (`rowHeight` 31) |

Columns inside the box: `index` 48 · `title` 234 · `duration` 96 (right-aligned). Hit + clip stay inside this rect.

### Font (variable metrics)

Not a rectangular grid or non-overlapping stack. Each of **10 bands** is a set of **freeform PNGs** with absolute origins that **may overlap** and vary in size.

```json
"visuals": {
  "spectrum": {
    "mode": "segments",
    "bands": [{
      "id": 0,
      "segments": [
        { "image": "spectrum/band0_0.png", "origin": { "x": 420, "y": 540 }, "reveal": 0.0 },
        { "image": "spectrum/band0_1.png", "origin": { "x": 418, "y": 512 }, "reveal": 0.25 },
        { "image": "spectrum/band0_2.png", "origin": { "x": 430, "y": 490 }, "reveal": 0.55 }
      ]
    }]
  }
}
```

| Field | Meaning |
|-------|---------|
| `origin` | **Top-left of that PNG** (2× artboard) — free placement |
| `reveal` | Energy threshold `0..1` when the piece lights |
| Overlap | Allowed — no stack packing |
| Size | From each PNG (they may all differ) |

Draw: for each band, every segment with `reveal <= energy` is painted (sorted by `reveal`).

### Font (variable metrics)

| Class | Cell (artboard px) | Notes |
|-------|--------------------|--------|
| `digit` | **24×24** | square |
| `letter` | **36×18** | 2:1, **shorter** than digits |
| `symbol` | 24×18 | optional |

- **Bottom-aligned** on a shared baseline (short letters meet digits).
- `map`: `{ "col", "row", "class": "letter" }` indexes that class’s atlas grid.
- `atlasOrigin` positions that class’s grid inside one PNG.

**About “5 vertical rectangles” in glyphs.png:** that was **placeholder drawing only** (column rulers / spacing guides in the auto-generated atlas) — not production glyphs and not required by the engine. Replace `font/glyphs.png` with your real sheet; only `map` cells are sampled.

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
