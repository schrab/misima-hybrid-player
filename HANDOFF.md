# HANDOFF - Misima Hybrid Player

Remote: https://github.com/schrab/misima-hybrid-player
Branch: feature/skinnable-player-mvp
Skin (single file): app/public/sprite/skin.json

## Do not

1. Regenerate faders/buttons in skin.json
2. Use bg slot X for knobs
3. Add a second skin.json (dist/ is build output only)
4. Reload page on DPI change (resets EQ)
5. Mix up pitch vs tempo (they are independent)

## Audio

- tempo = OLA time-stretch (speed only)
- pitch = OLA pitch-shift (tone only)
- then EQ, reverb (soft clip), volume

## Display

Window 750x1030 device px; art 1500x2060 at 50%.
Playlist 1028,1490 378x310; status 1153,1817 w=220; scope 911,180 226x142.

## Font

digit 24x24 (0,0), symbol 24x18 (0,72), letter 36x18 (0,120) rows 0-3.
Playlist: number + 6 letters + duration.

## Art

Edit skins/misima-hybrid/sprites/ then copy to app/public/sprite/ then reload.

## Docs

README.md, docs/compose/spec/sprite-skin-ui.md
