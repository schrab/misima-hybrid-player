---
feature: sprite-skin-ui
status: delivered
updated: 2026-09-17
branch: feature/skinnable-player-mvp
commits: pending
---

# Sprite Skin UI (Organic Sprite UI)

## Report

**What was built** — Canvas sprite compositor driven by master BG PNGs and `skin.json` v2 anchors (14 faders: volume/pitch/reverb/eq0–9/speed; transport buttons; 10-band raster spectrum; phase3D waterfall; bitmap-font playlist). Rust `set_params` wires volume, pitch+speed rate, Schroeder reverb, and 10-band EQ. Placeholder kit under `app/public/sprite/` + `scripts/make_sprite_kit.py`. Packed `.mskin` validates v1+v2.

**Verification** — `cargo test --lib` 19/19 PASS; `tsc --noEmit` + `vite build` PASS. Independent review found majors (v2 load, font fallback, reverb allpass, linear spectrum collapse, missing-sprite fail-hard) — fixed in follow-up.

**Journey log** —
1. HTML form UI was the wrong model; art must define layout via absolute anchors.
2. Raster glyph atlas is enough — TTF optional only.
3. load_skin must accept formatVersion 2 with required `blocks`+`faders`.
4. Schroeder allpass is `y=-g*x+d; d'=x+g*y` — not `(1-g)out+buf`.
5. Collapse 48 spectrum bins to 10 with log-aligned edges to match EQ centers.

## [S1] Problem

The player UI must be a **fully custom sprite composition** driven by the artist’s hand-drawn plates — not HTML form widgets. No OS window decorations. Faders, buttons, spectrum strips, and text sit at **fixed positions defined by the background art**. The interface is organic/non-rectangular (transparent PNG silhouettes).

Blocks (from the sketch):

1. **Visualizer** — 10-band spectrum; each band is a vertical strip of **individual raster segments** (various shapes). Also a generative **phase / 3D waterfall** layer.
2. **Transport** — play / stop / prev / next (and open) in the bottom-left green area attached to the visualizer; button **overlay sprites**.
3. **EQ / FX / Master** — vertical faders L→R: **volume, pitch (tone), reverb FX, 10× EQ, playback speed**. Knob sprites slide on tracks baked into the background.
4. **Playlist** — 10 lines: **track # · name · duration**, custom **raster glyph font** (not system UI type).

User can produce: background PNGs, fader knobs, button overlays, font.

## [S2] Design

### 2.1 Principles

- **Art is layout.** Background PNGs encode structure; `skin.json` only names anchors and IDs in **canvas pixels** (1:1 with art export).
- **No CSS chrome.** One transparent OS window; a single canvas (or per-block canvases) composites sprites every frame.
- **Everything hot is a sprite** (knob, button state, spectrum cell). Static ornament is baked into the BG plate.
- **Generative layers** (spectrum fill, waterfall/phase, optional EQ curve stroke) draw **only** inside their JSON rects so they never overwrite art.

### 2.2 Asset kit (what the artist supplies)

| Asset | Format | Notes |
|-------|--------|-------|
| Block background | **PNG + alpha** | Organic silhouette; export at **1× canvas** or 2× with `scale` in JSON. Transparent outside the plate. |
| Fader knob | **PNG + alpha** | One per family (magenta cap) is enough; optional second for pressed. |
| Button frames | **PNG + alpha** | `normal`, `pressed` (optional `disabled`). Hit size = JSON `size`. |
| Spectrum cell | **PNG strip or sheet** | Each band is stacked cells (empty→full). Prefer one strip per energy step or a grid sheet. |
| Glyph atlas | **PNG + JSON map** | Fixed cell grid (e.g. 10×18). `map` char → `[col,row]`. No TTF required. |
| Optional | TTF | Dev/debug only; production text uses atlas. |

Recommended folders (native skin zip or `skins/misima-hybrid/sprites/`):

```
bg/visualizer.png
bg/eq.png
bg/playlist.png
sprites/knob_magenta.png
sprites/btn_play_normal.png
sprites/btn_play_pressed.png
...
sprites/spectrum_cell.png      # vertical strip of N states, or
sprites/spectrum_sheet.png     # grid
font/glyphs.png
font/glyphs.json
skin.json
```

**Export rule:** artboard pixels == `canvas.width` × `canvas.height` (or 2× with `"scale": 2`). Fader travel and button hits are integers on that grid.

### 2.3 `skin.json` v2 contract

```json
{
  "formatVersion": 2,
  "id": "misima-hybrid",
  "name": "Misima Hybrid",
  "canvas": { "width": 1280, "height": 980, "scale": 1 },
  "blocks": {
    "visualizer": {
      "image": "bg/visualizer.png",
      "origin": { "x": 0, "y": 0 },
      "size": { "w": 1280, "h": 420 },
      "hit": "auto-alpha"
    },
    "eq": {
      "image": "bg/eq.png",
      "origin": { "x": 0, "y": 430 },
      "size": { "w": 1280, "h": 220 },
      "hit": "auto-alpha"
    },
    "playlist": {
      "image": "bg/playlist.png",
      "origin": { "x": 640, "y": 660 },
      "size": { "w": 560, "h": 300 },
      "hit": "auto-alpha"
    }
  },

  "faders": [
    {
      "id": "volume",
      "param": "volume",
      "orientation": "vertical",
      "origin": { "x": 260, "y": 720 },
      "travel": 120,
      "knob": "sprites/knob_magenta.png",
      "knobHotspot": "center",
      "range": [0, 1],
      "value": 0.8
    },
    { "id": "pitch",   "param": "pitch",   "range": [-12, 12], "unit": "st" },
    { "id": "reverb",  "param": "reverb",  "range": [0, 1] },
    { "id": "eq0",     "param": "eq0",     "range": [-12, 12], "unit": "db" },
    { "id": "eq1",     "param": "eq1",     "range": [-12, 12], "unit": "db" },
    { "id": "eq2",     "param": "eq2",     "range": [-12, 12], "unit": "db" },
    { "id": "eq3",     "param": "eq3",     "range": [-12, 12], "unit": "db" },
    { "id": "eq4",     "param": "eq4",     "range": [-12, 12], "unit": "db" },
    { "id": "eq5",     "param": "eq5",     "range": [-12, 12], "unit": "db" },
    { "id": "eq6",     "param": "eq6",     "range": [-12, 12], "unit": "db" },
    { "id": "eq7",     "param": "eq7",     "range": [-12, 12], "unit": "db" },
    { "id": "eq8",     "param": "eq8",     "range": [-12, 12], "unit": "db" },
    { "id": "eq9",     "param": "eq9",     "range": [-12, 12], "unit": "db" },
    { "id": "speed",   "param": "speed",   "range": [0.5, 2],  "unit": "x", "value": 1 }
  ],

  "buttons": [
    {
      "id": "prev",
      "action": "prev",
      "origin": { "x": 40, "y": 340 },
      "size": { "w": 48, "h": 36 },
      "frames": {
        "normal": "sprites/btn_prev_normal.png",
        "pressed": "sprites/btn_prev_pressed.png"
      }
    }
  ],

  "visuals": {
    "spectrum": {
      "origin": { "x": 300, "y": 80 },
      "size": { "w": 720, "h": 200 },
      "bands": 10,
      "cell": { "w": 64, "h": 12 },
      "frames": 12,
      "sheet": "sprites/spectrum_sheet.png",
      "align": "bottom",
      "gapPx": 4
    },
    "waterfall": {
      "origin": { "x": 80, "y": 80 },
      "size": { "w": 200, "h": 160 },
      "mode": "phase3d",
      "color": "3dffb5"
    }
  },

  "text": {
    "font": {
      "atlas": "font/glyphs.png",
      "cell": { "w": 10, "h": 18 },
      "map": {
        "A": [0, 0], "B": [1, 0], "0": [2, 0], "1": [3, 0],
        " ": [26, 0], "-": [27, 0], ".": [28, 0], ":": [29, 0]
      },
      "fallback": "?"
    },
    "playlist": {
      "origin": { "x": 700, "y": 720 },
      "rows": 10,
      "rowHeight": 20,
      "columns": [
        { "id": "index", "width": 28 },
        { "id": "title", "width": 360 },
        { "id": "duration", "width": 56, "align": "right" }
      ]
    },
    "status": {
      "origin": { "x": 40, "y": 400 }
    }
  }
}
```

**Semantics**

- `origin` of a fader = **top** of travel for vertical; value maps to travel: `y = origin.y + (1-n)*travel`.
- `hit: "auto-alpha"` — window/input hit region from BG alpha (organic silhouette).
- Spectrum: for band *b*, energy `e∈[0,1]` picks frame `floor(e*(frames-1))` from the sheet; blit `cell.w×cell.h` stack at band column.
- Glyph map is optional for missing chars (`fallback`).
- Unknown keys ignored (forward compatible). Loader rejects zip traversal / >64 MiB (same as v1).

### 2.4 Render pipeline (per frame)

```
clear transparent
  → draw block backgrounds (fixed)
  → generative (clipped to visuals.*):
       waterfall/phase (scroll + project)
       spectrum raster cells (10 bands)
       optional EQ response polyline
  → fader knobs (position from live params)
  → buttons (pressed state)
  → bitmap text (playlist rows, status, optional time)
```

Compositor: **single `<canvas>`** sized to `canvas.width/height`, CSS-scaled to window. Pointer events map through scale.

### 2.5 Interaction

| Input | Behavior |
|-------|----------|
| LMB drag on fader | Set param along travel; clamp to `range` |
| LMB on button | `pressed` frame → fire `action` on pointerup inside |
| LMB drag on title strip / non-hit | Move window (`startDragging`) — optional `dragZones` |
| Double-click playlist row | `play_index` |
| Scroll on playlist | Scroll if rows > visible (MVP: fixed 10, no scroll) |

### 2.6 Sound / DSP capabilities

Pipeline (offline buffer or live callback; target real-time):

```
PCM → [speed / pitch] → EQ 10-band peaking → [reverb] → volume → device
         ↘ spectrum tap (post-EQ, pre-volume) → UI
```

| Param | Range | Implementation (MVP) | Later |
|-------|-------|----------------------|--------|
| **volume** | 0..1 | Linear gain | Soft clip |
| **pitch** | ±12 st | Playback rate on resample / `rubato`-style ratio = 2^(st/12) **while preserving duration optional** | Time-stretch (phase vocoder) if speed must stay 1 |
| **speed** | 0.5..2× | Resample ratio (changes pitch + duration together) | Independent via WSOLA |
| **reverb** | 0..1 | Feedback delay / simple Schroeder (4 combs + 2 allpass) mix | Convolution IR |
| **eq0..9** | ±12 dB | RBJ peaking @ 60,170,310,600,1k,3k,6k,12k,14k,16k | Shelf options |

**Relationship:** MVP treats **speed** as sample-rate style playback rate (affects pitch like tape). **pitch** is an extra semitone trim on the same resampler (product with speed). Document clearly in UI later.

Already implemented in engine: decode, device resample, 10-band EQ, FFT bins. **Add:** pitch/ratio combine, Schroeder reverb, 10-band spectrum display from 10 log bands (not 48 generic bins).

### 2.7 Bitmap font

- Atlas PNG + `cell` + `map`.
- Draw: `drawImage(atlas, col*cw, row*ch, cw, ch, x, y, cw, ch)`.
- **You do not need TTF.** Raster is preferred for hand-drawn / alien glyphs. Optional TTF only if you want crisp debug labels.

### 2.8 Workflow (artist ↔ engine)

1. Paint **master artboards** at `canvas` size (or 2×).
2. Slice knobs/buttons/cells **or** export full plates + few interactive sprites.
3. Place elements in Photoshop/Krita; **read pixel coordinates** of knob top travel and button corners.
4. Fill `skin.json` origins/sizes (or generate via `scripts/measure_skin.py` later).
5. Pack: `python scripts/make_skins.py` → `.mskin`.
6. Run player → load skin → drag faders to verify hit boxes.
7. Iterate numbers in JSON; repack.

**We provide:** `skin.json` schema, placeholder art + coordinates, compositor, DSP wiring, pack script.  
**You provide:** production PNGs + atlas + numbers matching your art.

### 2.9 Architecture change vs HTML MVP

| Keep (Rust) | Replace (UI) |
|-------------|--------------|
| Symphonia decode, playlist, cpal out, EQ, FFT, skin zip | HTML/CSS panels & range inputs |
| Tauri shell, transparent, `decorations: false` | — |
| IPC commands | Canvas compositor + pointer → params |

New frontend: `app/src/sprite/{compositor,faders,buttons,font,spectrum,waterfall,layout}.ts`  
New params: `set_params { volume, pitch, reverb, eq[10], speed }`

### 2.10 Error behavior

- Missing sprite file: skip draw, log once.
- Atlas char missing: `fallback` glyph.
- Fader drag outside travel: clamp.
- Skin v2 without required `faders`/`blocks`: reject `load_skin`.
- No audio device: generative visuals still idle; transport no-ops.

### 2.11 Testing

- Unit: JSON v2 parse; fader value↔pixel map; glyph blit string length; 10-band energy from sine; reverb dry=identity.
- Manual: load skin, drag each fader (audio changes), buttons, playlist dblclick, spectrum+waterfall animate when playing.

## [S3] Out of Scope

- Visual skin editor GUI (JSON + art only)
- Time-stretch that keeps pitch fixed while changing speed (post-MVP)
- IR convolution reverb
- >10 playlist rows / scrolling in v2 (fixed 10 lines)
- Classic `.wsz`/`.wal` formats
- Desktop font rendering without atlas

## Tasks

- [x] T1: Author `skin.json` v2 schema + placeholder BG/knob/button/atlas (covers: S2.2, S2.3)
- [x] T2: Implement sprite compositor (covers: S2.4)
- [x] T3: Implement fader hit-test/drag → params; button press frames (covers: S2.5)
- [x] T4: Implement bitmap font + 10-row playlist + status (covers: S2.7)
- [x] T5: 10-band raster spectrum + phase3d waterfall (covers: S2.4, S2.1)
- [x] T6: Rust DSP: pitch+speed rate, Schroeder reverb, `set_params` (covers: S2.6)
- [x] T7: Replace HTML panel UI with full-window canvas (covers: S2.9)
- [x] T8: Pack script v2, README artist workflow (covers: S2.8)
- [x] T9: Unit tests + builds + review fix loop (covers: S2.11)

**Accepted residual:** UI loads default `/sprite/` kit at runtime (custom `.mskin` load_skin returns bundle for future picker UI); nearest-frame pitch/speed (no interpolation); click-through alpha hit not implemented (`hit: auto-alpha` reserved).

## [S4] Delivered notes (2026-09)

- Single skin file: app/public/sprite/skin.json (no Winamp packages).
- Window 750x1030 device px; art 1500x2060 at 50%; DPR-aware.
- Font: letter rows 0-3 at atlasOrigin (0,120) — not row 4+.
- Playlist 1028,1490; status 1153,1817 w=220; echo scope 911,180 226x142 (8 echoes + Hann).
- DSP: pitch = OLA tone-only; tempo = OLA time-stretch only; reverb soft-clipped.
- Fader hit width 48px; tempo log-centred at 1.0.
