---
feature: skinnable-player-mvp
status: in-progress
updated: 2026-09-17
branch: feature/skinnable-player-mvp
commits: 26e2411..pending
---

# Skinnable Multiplatform Music Player MVP

## Report

## [S1] Problem

Ship a cross-platform (Windows/macOS/Linux) music player whose UI can be heavily customized like classic Winamp skins, matching an organic non-rectangular sketch (top spectrum, middle EQ, bottom playlist). Also produce a separate Winamp 5 freeform modern skin package from the same aesthetic for Winamp/WACUP users.

Constraints from product decisions:
- Toolchain: **Tauri 2 + Rust**
- Native skins: **ZIP + PNG/WebP + skin.json** (not classic .wsz)
- Winamp deliverable: **Winamp 5 freeform modern** (`.wal` / modern skin package), not classic WA2 BMP skins
- Audio MVP: **local files + real 10-band EQ + live FFT spectrum**

## [S2] Design

### Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Tauri 2 shell (window: transparent, decorations off)       │
│  ┌───────────────────────────────────────────────────────┐  │
│  │  WebView UI (HTML/CSS/Canvas)                         │  │
│  │  - spectrum canvas (FFT bins from Rust)               │  │
│  │  - EQ faders → IPC → Rust DSP                         │  │
│  │  - playlist (drag reorder, double-click play)         │  │
│  │  - non-rect hit regions via CSS clip-path             │  │
│  └───────────────────────┬───────────────────────────────┘  │
│                          │ Tauri commands / events          │
│  ┌───────────────────────▼───────────────────────────────┐  │
│  │  Rust core                                            │  │
│  │  - decode: Symphonia (mp3/flac/wav/ogg)               │  │
│  │  - output: cpal                                       │  │
│  │  - EQ: 10-band peaking biquads                        │  │
│  │  - spectrum: rustfft on windowed PCM → bins           │  │
│  │  - playlist state, transport, volume                  │  │
│  └───────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Window & non-rectangular UI

- Main window: `decorations: false`, `transparent: true`.
- Panels use CSS `clip-path` polygons matching the sketch silhouette.
- Drag bars use Tauri `-webkit-app-region: drag` / start-dragging permission.

### Native skin package format

A skin is a `.zip` (extension `.mskin`) containing `skin.json` + PNG assets.

Loader rules:
- Reject zip with path traversal (`../`), max uncompressed size 64 MiB.
- `formatVersion` must be 1; unknown fields ignored.
- Path traversal and missing-manifest covered by unit tests.

### Audio pipeline

1. Open file → Symphonia probe/decode to f32 interleaved.
2. Playback thread: cpal stream callback pulls from shared buffer.
3. DSP chain: source → 10-band biquad EQ → volume → output.
4. Parallel tap: mono mix → Hann window → rustfft → 48 log-spaced bins → `spectrum` event ~30 Hz.

Supported formats MVP: `.mp3`, `.flac`, `.wav`, `.ogg`.

### IPC surface

| Command | Effect |
|---------|--------|
| `open_files` | enqueue paths |
| `play` / `pause` / `stop` / `next` / `prev` / `play_index` | transport |
| `seek` / `get_position` | position |
| `set_volume` / `set_eq` | volume + 10-band gains |
| `get_playlist` / `reorder_playlist` / `clear_playlist` | playlist |
| `load_skin` | parse `.mskin` zip |

Events: `spectrum`, `track_changed`.

### Winamp 5 freeform modern skin deliverable

- Source: `skins/winamp5-misima/` (`skin.xml` + PNG alpha plates)
- Packed: `skins/winamp5-misima.wal`
- Classic `.wsz` cannot express freeform alpha; `.wal` is the correct modern format
- Placeholder plates generated; art direction documented for final painting

### Repository layout

```
app/                      # Tauri 2 project
  src-tauri/              # Rust core
  src/                    # frontend
skins/
  misima-hybrid/          # native skin source
  misima-hybrid.mskin
  winamp5-misima/
  winamp5-misima.wal
scripts/make_skins.py
gfx/UI_sketch.PNG
docs/compose/spec/skinnable-player-mvp.md
```

### Error behavior

- Unreadable/unsupported file: command returns error string; UI status shows message.
- Skin invalid: `load_skin` rejects; unit tests cover traversal/missing manifest.
- Audio device missing: stream start logs error; playback buffer still loads.
- Empty playlist play: no-op.

### Testing boundaries

- Unit (13 tests, all passing): skin zip safety, EQ energy on 1 kHz boost/cut, spectrum sine vs silence, WAV decode, playlist reorder, player load/stop.
- Frontend: `tsc --noEmit` clean; `vite build` succeeds.
- Manual (not automated in this pass): live device playback + visual spectrum in `tauri dev`.

## [S3] Out of Scope

- Streaming services, internet radio, CD rip
- Library database / tags editor UI
- Classic Winamp 2 `.wsz` BMP/color-key skins
- Winamp 5 skin *loader* runtime (we author one modern skin)
- Plugin API, AVS visualizations, media keys
- Mobile
- Installer signing / auto-update

## Tasks

- [x] T1: Scaffold Tauri 2 app in `app/` with transparent undecorated window — acceptance: project structure, config, capabilities, icons present; frontend builds.
- [x] T2: Implement Rust audio engine (Symphonia decode, cpal output, transport, volume, playlist state) — acceptance: unit tests load WAV into shared buffer and stop cleanly.
- [x] T3: Implement 10-band EQ biquad chain and wire `set_eq` — acceptance: unit test shows +12 dB boost increases RMS near 1 kHz, cut decreases it.
- [x] T4: Implement FFT spectrum tap and emit `spectrum` events — acceptance: sine fixture yields non-zero bins; silence is zero; frontend canvas draws bars.
- [x] T5: Define and implement native skin loader (ZIP + skin.json) — acceptance: rejects path traversal; loads valid zip; formatVersion rename tested.
- [x] T6: Build frontend panels: spectrum, EQ, playlist, transport; non-rect clip-path — acceptance: UI matches sketch structure; playlist double-click plays; dialog opens files.
- [x] T7: Author Winamp 5 freeform modern skin source + pack `.wal` — acceptance: skin.xml + PNGs + packed archive; format documented.
- [x] T8: Pack native `misima-hybrid.mskin` and document skin authoring format in README — acceptance: archive exists; README describes skin.json schema.
- [x] T9: Verify build (`cargo test --lib` 13/13 pass, `tsc` + `vite build` pass) and prepare for review.
