# HANDOFF - Misima Hybrid Player

Remote: https://github.com/schrab/misima-hybrid-player
Branch: main (the only branch)
Skin (single folder): app/public/sprite/ (skin.json + bg/ ui/ font/ spectrum/ anim/)

## Do not

1. Regenerate faders/buttons in skin.json
2. Use bg slot X for knobs
3. Add a second skin folder (app/public/sprite is the only copy; dist/ is build output only)
4. Reload page on DPI change (resets EQ)
5. Mix up pitch vs tempo (they are independent)
6. Put fixed gain on the reverb wet path (raw tail varies ~20 dB by material — it is envelope-normalized, see agents.md §3.1.8)
7. Use unchecked usize subtraction around WSOLA fifo_read_pos (underflow kills the audio thread)
8. Scale reverb delay offsets as well as lengths when changing sample rate (agents.md §3.1.4)
9. Map the reverb fader onto loop gain as `0.35 + 0.63 * amount` (reaches 0.98, diverges the loop — agents.md §3.1.9)
10. Let the vocoder's synthesis hop vary (must stay fixed at n/4; varying it makes the WOLA normalisation ripple and diverge — docs/DSP.md §3.2)
11. Expect the vocoder's tempo convention to match the WSOLA's (it inverts: hs = ha / stretch — docs/DSP.md §3.2)
12. Drop the −28 dB peak floor in the vocoder's peak picking (Hann sidelobes at −31.5 dB then split partials against themselves — docs/DSP.md §3.2)
13. Diagnose DSP faults without the identity test first (phase_vocoder `identity_phases_reconstruct_exactly` splits overlap-add faults from phase-logic faults — docs/DSP.md §5)

## Audio

- tempo = OLA time-stretch (speed only)
- pitch = OLA pitch-shift (tone only), clamped to ±1 octave (matches the fader range in skin.json)
- then EQ, reverb, volume
- reverb: dry→wet crossfade of an envelope-normalized tail; 100% fader = full wet at ~dry loudness
- reverb DSP is a stereo FDN (Dattorro/Griesinger) ported from Mutable Instruments Clouds —
  MIT, © 2014 Emilie Gillet — in `audio/clouds_reverb.rs`; `player.rs::Reverb` only owns the
  mix and the envelopes. Tail length is a fixed `REVERB_TIME`, deliberately NOT tied to the fader.
- pitch-up runs through a stereo phase vocoder (`audio/phase_vocoder.rs`) because WSOLA
  time-compression is audibly granular there; stretch > 1 stays on the WSOLA. The engine
  split lives in `player.rs::Stretcher`. Vocoder invariants: synthesis hop FIXED at n/4
  (exact WOLA at every ratio), strict identity locking with a -28 dB peak floor, phases
  seeded from the first signal-bearing frame.

## Display

Window 750x1030 device px; art 1500x2060 at 50%.
Playlist 1028,1490 378x310; status 1153,1817 w=220; scope 911,180 226x142.

## Font

digit 24x24 (0,0), symbol 24x18 (0,72) (band empty in atlas), letter 36x18 (0,120) rows 0-3.
Playlist row: 2-digit number + up to 6 title glyphs (filename index prefixes stripped) + minutes.

## Skin layers

- overlays[]: still layers above bg (e.g. UI_highlights.png); artist erases animated areas from the art, engine draws unmasked
- animations[]: uniform-grid sprite sheets, screen blend (black drops out), origin = frame-0 top-left, fps per entry
- spectrum bands[].xShift: whole-column X nudge; energies have a display tilt (BAND_GAIN in main.ts) — tune it there, not in Rust
- spectrum chip sets: bands may use different chip PNG sets (default chip_*, chip_1_*, chip_2_*); currently cycled 1-2-3 across columns. Tune one band, then clone anchor-relative (keep each column's left edge/xShift, copy Y-stack + X jitter)

## Art

Edit app/public/sprite/ directly (single folder), then press F5 in the player window
(Vite does not watch public/). gfx/ holds untracked artboard PSD sources.

## Docs

README.md, agents.md, docs/compose/spec/sprite-skin-ui.md
