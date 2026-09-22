# HANDOFF — Misima Hybrid Player

Last updated: 2026-09-17  
Branch: `feature/skinnable-player-mvp`  
Workspace: `C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp`  
Head (at last polish): `3a7bc7d`  
Feature spec: `docs/compose/spec/skinnable-player-mvp.md` (status: delivered)

---

## 1. What this is

A cross-platform (Windows/macOS/Linux) **skinnable music player MVP** inspired by Winamp, built for an organic non-rectangular UI sketch (`gfx/UI_sketch.PNG`):

| Region | Role |
|--------|------|
| Top | Spectrum visualizer |
| Middle | 10-band EQ |
| Bottom | Playlist |

### Product decisions (locked)

| Axis | Decision |
|------|----------|
| Toolchain | **Tauri 2 + Rust** + HTML/CSS/Canvas frontend |
| Audio MVP | Local playback (mp3/flac/wav/ogg) + **real 10-band EQ** + **live FFT spectrum** |
| Native skin format | **ZIP + PNG/WebP + `skin.json`** (`.mskin`) — not classic `.wsz` |
| Winamp deliverable | **Winamp 5 freeform modern** (`.wal`) — classic `.wsz` cannot express freeform alpha |
| Worktree | Isolated at `.worktrees/skinnable-player-mvp` on branch `feature/skinnable-player-mvp` |

### Why not classic `.wsz`

Classic Winamp 2 skins are fixed-size BMP + color-key (main 275×232). The sketch needs freeform alpha silhouettes → authored as **Winamp 5 modern `.wal`**. Documented in `skins/winamp5-misima/README.md` and project README.

---

## 1b. Sprite UI (current)

The form HTML UI is **replaced** by a canvas compositor (`app/src/main.ts` + `app/src/sprite/*`):

- Layout: **master BG PNGs + `skin.json` v2 absolute anchors** (artist pixels = canvas px)
- Controls: fader knobs & button frames as sprites; 14 faders L→R: volume, pitch, reverb, eq0–9, speed
- Text: **raster glyph atlas** (no TTF required)
- Generative: 10-band raster spectrum sheet + phase/3D waterfall
- Default kit: `app/public/sprite/` + source `skins/misima-hybrid/sprites/`
- Spec: `docs/compose/spec/sprite-skin-ui.md`
- Generate placeholders: `python scripts/make_sprite_kit.py`

DSP: `set_params` → volume, pitch (st), reverb mix (Schroeder), eq[10], speed (tape rate).

## 2. Layout

```
misima-hybrid-winamp/                 # main checkout (main branch)
  gfx/UI_sketch.PNG
  .worktrees/skinnable-player-mvp/    # THIS workspace
    docs/compose/spec/
      skinnable-player-mvp.md
      sprite-skin-ui.md               # current UI system
    app/src/sprite/                   # compositor, font, visuals, layout
    app/public/sprite/                # default runtime kit
    skins/misima-hybrid/sprites/      # art source + skin.json v2
    scripts/make_sprite_kit.py
```

```
misima-hybrid-winamp/                 # main checkout (main branch)
  gfx/UI_sketch.PNG                   # original UI sketch
  .worktrees/skinnable-player-mvp/    # THIS workspace (feature branch)
    docs/compose/spec/skinnable-player-mvp.md
    README.md
    HANDOFF.md                        # this file
    app/                              # Tauri 2 project
      package.json, vite.config.ts, index.html
      public/skin/                    # default panel PNGs (served by Vite)
      src/main.ts, styles.css, ui/*
      src-tauri/
        Cargo.toml, tauri.conf.json, capabilities/default.json
        src/{main,lib,commands,playlist,skin}.rs
        src/audio/{mod,decoder,eq,spectrum,player}.rs
        icons/
    skins/
      misima-hybrid/                  # native skin source
      misima-hybrid.mskin             # packed ZIP
      winamp5-misima/                 # Winamp 5 freeform source (skin.xml + png/)
      winamp5-misima.wal              # packed modern skin
    scripts/make_skins.py             # generate plates + pack archives
```

---

## 3. Architecture

```
WebView UI (HTML/CSS/Canvas)
  · spectrum canvas (bins from Rust)
  · EQ sliders → set_eq
  · playlist (drag reorder, dblclick play)
  · clip-path silhouettes; content in .panel-inner
        │  Tauri invoke / events
        ▼
Rust core
  · Symphonia decode → f32 PCM
  · resample_interleaved → device sample rate
  · SharedPlay: buffer, cursor, volume, EQ, spectrum, ended flag
  · cpal output callback: read lock → EQ → volume → device
  · rustfft → 48 log-spaced bins → emit "spectrum" ~30 Hz
  · skin zip loader (path-traversal + size caps)
```

### Key modules

| Module | Responsibility |
|--------|----------------|
| `audio/decoder.rs` | Symphonia open/decode; test WAV writer |
| `audio/eq.rs` | RBJ 10-band peaking biquads; `set_gains` rebuilds coeffs |
| `audio/spectrum.rs` | Hann + rustfft; magnitudes scaled for display |
| `audio/player.rs` | SharedPlay, resample-to-device, cpal stream, EOS → `track_ended` |
| `playlist.rs` | id/path/title list, reorder, current index |
| `skin.rs` | `formatVersion` 1, zip safety, size limits |
| `commands.rs` | All `#[tauri::command]` IPC |
| `lib.rs` | Builder, state, handler registration |

### IPC (invoke)

`open_files`, `play`, `pause`, `stop`, `next`, `prev`, `play_index`, `seek`, `get_position`, `set_volume`, `set_eq` (10 dB), `get_playlist`, `reorder_playlist`, `clear_playlist`, `load_skin` → `{manifest, assets(base64)}`

### Events (emit → UI)

- `spectrum`: `f32[]` / float array ~48 bins  
- `track_changed`: playlist id  
- `track_ended`: auto-advance via UI `next`

### Native skin (`skin.json`)

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

Default art is **also** applied on init from `app/public/skin/*.png` via CSS `--panel-bg` (no disk zip required for first paint). `load_skin` can override from a packed `.mskin`.

---

## 4. How to run (Windows)

**PATH:** Rust was installed under `%USERPROFILE%\.cargo\bin`. Open a **new** PowerShell after install; old shells show `cargo: program not found`.

```powershell
cd C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp\app
cargo --version          # must succeed
npm install              # ignore esbuild postinstall warn unless vite fails
npm run tauri dev
```

**Vite note:** `vite.config.ts` ignores `**/src-tauri/target/**` — do not remove that (Windows `EBUSY` on watcher).

Rebuild default skin plates:

```powershell
python scripts/make_skins.py   # or $MIMO_PYTHON
```

---

## 5. Verification (run before claiming done)

```powershell
# Unit tests — last known: 15/15 PASS
cd app/src-tauri
cargo test --lib

# Frontend
cd app
npx tsc --noEmit
npx vite build
```

Known PASS history:

| Check | Result |
|-------|--------|
| `cargo test --lib` | 15 passed (EQ boost/cut, spectrum, decode, playlist, skin zip, resample, EQ persist, load/stop) |
| `tsc --noEmit` | PASS |
| `vite build` | PASS |
| Independent review | Criticals fixed in `25285fe`; re-review success |
| Manual `tauri dev` | Launches `target\debug\misima-player.exe` |

**Not automated:** live device audio quality, visual skin fidelity vs sketch, Winamp/WACUP load of `.wal`.

---

## 6. Review history (do not re-litigate)

Initial review criticals — **fixed**:

1. No sample-rate conversion → `resample_interleaved` on load to device rate  
2. EQ wiped on track load → persist `SharedPlay.eq_gains`  
3. EQ coeffs file-rate vs device-rate → always device rate  

Majors fixed: zip entry size before read, `data-tauri-drag-region` (not `-webkit-app-region`), `load_skin` returns assets, `samples.read()` in callback, `track_ended` + auto-next, on-disk skin file size check.

Residual non-blockers (accepted): linear resample (no AA filter), `applySkinFromPath` not wired to a picker UI, dual EQ store (`AppInner.eq_gains` vs `SharedPlay` — playback uses SharedPlay).

---

## 7. Current UX state (as of polish `3a7bc7d`)

- Three organic panels; clip-path on `::before`, content in `.panel-inner` so titles/buttons are not cut  
- Default neon plate PNGs visible on all panels  
- Spectrum: adaptive normalize + peak hold + backend gain boost  
- Status line updates on add/play/pause/stop/clear  
- Playlist: Wit Chu-style files load, double-click play, drag reorder  
- Window: decorations off, transparent, ~980×760 default  

User may still report: art flatter than original sketch, spectrum only when playing, no progress seek UI.

---

## 8. Out of scope (explicit)

- Streaming / radio / CD  
- Library DB / tag editor  
- Classic Winamp 2 `.wsz` BMP skins  
- Loading arbitrary WA5 skins in *this* player (we only *author* one `.wal`)  
- Mobile, plugins, media keys, auto-update, code signing  

---

## 9. Suggested next work (priority order)

1. **Art pass** — redraw `public/skin` plates from `gfx/UI_sketch.PNG` (organic cutouts, cyan/magenta/green, alien glyphs); keep PNG alpha; regenerate `.mskin` + `.wal` via `make_skins.py`.  
2. **Skin picker UI** — file dialog → `load_skin` → `applySkinFromPath` (command + helper already exist).  
3. **Seek / progress bar** — `seek` / `get_position` IPC exist; no UI.  
4. **Align EQ state** — drop unused `AppInner.eq_gains` or make it the only store.  
5. **Anti-alias downsample** — replace linear resample for quality.  
6. **Integrate default skin zip** — optionally load `.mskin` from resource dir in production builds (currently public PNG path).  
7. **Polish `.wal`** — replace dummy includes `xml/player-*.xml`; test in WACUP/Winamp 5.  

---

## 10. Commands cheat sheet

```powershell
# from worktree root
git log --oneline
git status

# tests
cd app/src-tauri; cargo test --lib
cd ..; npx tsc --noEmit; npx vite build

# run
cd app; npm run tauri dev

# pack skins
python scripts/make_skins.py

# stop leftovers if needed
Get-Process misima-player -ErrorAction SilentlyContinue | Stop-Process -Force
```

---

## 11. Closing / branch tips

- Do **not** commit from main worktree without checking `git status`.  
- Merge/PR target: `main` from `feature/skinnable-player-mvp`.  
- Feature doc finalization commit sits **outside** the reviewed range by Compose Next design (documentation-only).  
- When finishing: ask user for local merge / PR / keep branch; remove worktree only under `.worktrees/`.

---

## 12. Contacts / sources of truth

| Question | Read first |
|----------|------------|
| Requirements / contracts | `docs/compose/spec/skinnable-player-mvp.md` |
| How to build skins / formats | `README.md`, `skins/winamp5-misima/README.md` |
| Original UI intent | `gfx/UI_sketch.PNG` |
| Audio pipeline details | `app/src-tauri/src/audio/player.rs` |
| IPC surface | `app/src-tauri/src/commands.rs` + `app/src/main.ts` |
