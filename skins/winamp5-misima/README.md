# Winamp 5 freeform skin — Misima Hybrid

## Format note

Classic Winamp 2 skins use the `.wsz` extension and **BMP + color-key** graphics with hard window sizes (main 275×232). They cannot express freeform alpha silhouettes.

**Winamp 5 modern / freeform skins use `.wal`** (ZIP of `skin.xml` + PNG assets with alpha). This directory is authored for that engine. Pack as:

```powershell
Compress-Archive -Path skins\winamp5-misima\* -DestinationPath skins\winamp5-misima.wal -Force
```

Rename to `.wsz` only if a host accepts it; Winamp 5/WACUP expect `.wal` for modern skins.

## Required files (simplified skeleton)

| File | Role |
|------|------|
| `skin.xml` | Freeform layout, groups, containers, elements |
| `png/player_bg.png` | Main window plate (alpha) ~600×420 |
| `png/eq_bg.png` | EQ plate ~600×240 |
| `png/playlist_bg.png` | Playlist plate ~480×520 |
| `png/cbutton_*.png` | Transport buttons |
| `png/volume_*.png` | Volume slider parts |

## Art direction (from UI sketch)

- Dark slate `#0a1214` plates with teal metal rims
- Neon: cyan `#5ec8ff`, mint `#3dffb5`, magenta `#ff4fd8`, violet `#b388ff`
- Motifs: wireframe orbs, alien glyphs, waveform traces, organic cut corners
- True alpha PNG (not color key)

## Production checklist

1. Export plates at 2× then downscale for crispness.
2. Keep interactive regions clear of extreme clip-path notches.
3. Test in WACUP or Winamp 5.666+ with modern skin support.
