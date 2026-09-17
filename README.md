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

## Native skin format (v1)

A skin is a ZIP (`.mskin`) containing `skin.json` + PNG/WebP assets with alpha.

```json
{
  "formatVersion": 1,
  "id": "misima-hybrid",
  "name": "Misima Hybrid",
  "panels": {
    "main": {
      "rect": { "x": 0, "y": 0, "w": 920, "h": 280 },
      "image": "assets/panel_main.png",
      "clip": "auto-alpha"
    }
  }
}
```

- `clip: "auto-alpha"` derives the window/hit silhouette from PNG alpha.
- Rejects zip path traversal and archives over 64 MiB uncompressed.
- Pack with `python scripts/make_skins.py`.

## Winamp deliverable note

Classic Winamp 2 `.wsz` skins are BMP + color-key and **cannot** express freeform alpha shapes. Freeform skins from the sketch are authored as **Winamp 5 modern `.wal`** (`skin.xml` + PNG). See `skins/winamp5-misima/README.md`.

## Graphics formats needed

| Target | Format | Notes |
|--------|--------|-------|
| Native player skin | PNG/WebP + alpha | Organic silhouettes, 1x or 2x |
| Winamp 5 freeform | PNG + alpha in `.wal` | No color key |
| Sketch reference | PNG | Source art only |

Export plate art at 2× then downscale; keep control hit targets ≥ 28×28 px.
