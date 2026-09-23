# HANDOFF — Misima Hybrid Player

Branch: feature/skinnable-player-mvp
Workspace: .worktrees/skinnable-player-mvp
Skin file (only one): app/public/sprite/skin.json

## What
Cross-platform Tauri 2 + Rust player. Full custom sprite UI (organic PNG plates, knobs, buttons, bitmap font, spectrum chips). No Winamp formats.

## Run
cd app; npm install; npm run tauri dev
cd app/src-tauri; cargo test --lib

## Skin
Edit app/public/sprite/skin.json only.
- origin = top-left of knob at MAX
- travel = Y to MIN
- defaults: volume=1 max, pitch/eq/tempo mid, reverb=0 min
- Art: public/sprite/{bg,ui,font,spectrum}

## Architecture
app/src/sprite/* compositor; app/src-tauri/src/audio/* decode/EQ/reverb/spectrum/transport
set_params: volume, pitch, reverb, eq[10], speed

## Do not
- Do not regenerate faders/buttons in skin.json
- Do not use bg slot X for knobs (use UI_elements / manual X)
- Do not add second skin.json
