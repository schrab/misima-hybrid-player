# Misima Hybrid Player

Cross-platform skinnable music player with a fully custom organic sprite UI (PNG plates, knobs, buttons, bitmap font).

Repo: https://github.com/schrab/misima-hybrid-player
Branch: feature/skinnable-player-mvp

## Stack

- Shell: Tauri 2
- UI: Canvas sprite compositor + Vite (TypeScript / dev server)
- Audio: Symphonia + cpal
- DSP: 10-band EQ, Schroeder reverb, pitch (tone-only OLA), tempo (time-stretch only)

## Display

Artboard 1500x2060 (2x) shown at 750x1030 device pixels on FHD and 4K.

## Run

    cd app
    npm install
    npm run tauri dev
    npm run tauri build

Tests: cargo test --lib (app/src-tauri), npx tsc --noEmit (app).

## Skin

Only file: app/public/sprite/skin.json

- Units: Photoshop 2x artboard
- Knob origin: top-left at MAX
- travel: Y to MIN
- Defaults: volume max, EQ/pitch mid, reverb min, tempo 1x at mid (log scale)
- Playlist: 1028,1490 size 378x310, 10 rows
- Status: 1153,1817 width 220
- Echo scope: 911,180 226x142, magenta + 8 echoes
- Font: digit 24x24 at 0,0; symbol 24x18 at 0,72; letter 36x18 at 0,120 rows 0-3

Art source: skins/misima-hybrid/sprites/ then copy to app/public/sprite/
(or python scripts/make_sprite_kit.py).

## DSP

- pitch = tone only (OLA), semitones -12..+12. Does NOT change speed.
- tempo = speed only (OLA time-stretch) 0.5..2x. Does NOT change pitch.
- reverb = Schroeder wet, soft-clipped
- eq1-eq10 = peaking 60..16000 Hz

## Controls

- Wheel on fader (Shift = fine)
- Click playlist row to play
- Wheel on playlist scrolls if more than 10 tracks
- fx_enable: master EQ + reverb + pitch (default on)
- fx_reset: EQ 0, reverb 0, pitch 0, tempo 1
- power: quit

Icon: gfx/misima-gibrid-icon.png
