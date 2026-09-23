# Misima Hybrid Player

Cross-platform skinnable music player (Windows / macOS / Linux) with a fully custom organic sprite UI (non-rectangular plates, knobs, buttons, bitmap font).

## Stack

| Layer | Choice |
|-------|--------|
| Shell | Tauri 2 |
| Audio | Symphonia decode + cpal output |
| DSP | 10-band EQ, Schroeder reverb, pitch + speed |
| UI | Canvas sprite compositor (PNG plates + `skin.json` anchors + bitmap font) |

## Layout

```
app/                 Tauri player
  public/sprite/     runtime kit (bg, ui, font, spectrum, skin.json)
  src/sprite/        compositor, font, layout, visuals
skins/misima-hybrid/sprites/   art source (same PNGs)
gfx/UI_sketch.PNG, UI_elements.png
docs/compose/spec/
```

## Run

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

## Skin (single file)

**Edit only:** `app/public/sprite/skin.json`

| | |
|--|--|
| Units | Photoshop **2× artboard** px (1500×2060) |
| Knob `origin` | top-left at **MAX** value |
| Knob `travel` | Y down to **MIN** |
| Defaults | volume max · EQ/pitch/tempo mid · reverb min |
| Buttons | `origin` top-left; idle art in `bg.png`; `pressed` overlay |

```json
{
  "id": "volume",
  "origin": { "x": 515, "y": 930 },
  "travel": 110,
  "value": 1,
  "knob": "ui/knob_volume.png",
  "range": [0, 1]
}
```

PNG kit: `bg/bg.png`, `ui/knob_*.png`, `ui/button_*.png`, `font/glyphs.png` (digits 24×24, letters 36×18), `spectrum/chip_*.png`.
