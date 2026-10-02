# Changelog

All notable changes to the Misima Hybrid Player. Versions follow semver
loosely; the version lives in `app/package.json`, `app/src-tauri/tauri.conf.json`
and `app/src-tauri/Cargo.toml` and must be bumped together.

## [0.2.0] — 2026-10-03

### Added
- **Stereo phase vocoder** (`audio/phase_vocoder.rs`) handles the pitch-up
  region (engine `stretch ≤ 1`), replacing the WSOLA there because WSOLA
  time-compression is audibly granular (4x incoherent grain overlap at +1
  octave). Fixed synthesis hop at n/4 for exact overlap-add at every ratio;
  strict identity phase locking with a −28 dB peak floor; instantaneous
  frequencies estimated from the mid spectrum so both channels warp
  identically (no mono sums in the output path). Engine selection in
  `player.rs::Stretcher`; WSOLA remains for tempo and pitch-down, where it is
  in its good expansion regime.
- **`docs/DSP.md`** — deep-dive documentation of the audio engines for
  reviewers and future maintainers.
- **`head_sheet` skin animation** — 8 frames @ 110 px, two instances
  (230,300) and (1300,1380), 3 fps.

### Changed
- Pitch clamped to **±1 octave** (±12 st) in the Rust layer, matching the
  fader range in `skin.json`; the old ±24 st clamp was unreachable and hid
  where the WSOLA degrades.
- Test suite 33 → 39 tests; `agents.md` verification protocol updated.

### Fixed
- A seek now flushes the reverb tail instead of carrying the previous
  position's reverb across the jump.

## [0.1.0] — 2026-10-02

### Changed
- **Reverb replaced** with a stereo feedback-delay network ported from
  Mutable Instruments Clouds (MIT, © 2014 Emilie Gillet), Dattorro/Griesinger
  topology, in `audio/clouds_reverb.rs` — the old Schroeder reverb summed its
  input to mono and collapsed the tail to a dead-centre image. The dry/wet
  crossfade and envelope-normalised wet gain (100% fader = full wet at matched
  loudness) were kept in `player.rs::Reverb`; wet-gain `TARGET` retuned 0.9 →
  1.8 for the new tail.
- Spectrum display: tilt and per-band xShift (f044b9f); per-band chip-set
  variations (796ad51).
- Animation sprite-sheet engine (`skin.json animations[]`): 5 sheets, screen
  blend, user-tuned fps and origins (4d9ca2b/6f3ed51).
- Single skin folder: `app/public/sprite` is the only skin copy; `skin.json`
  is user-owned and never regenerated (df566fb).

### Fixed
- WSOLA FIFO read-position underflow panic near track end (silent output),
  504c39f.
- Playlist titles stripped of filename index prefixes; duration minutes-only
  (1fec13b).
- Reverb wet loudness: −16 dB drop at high mix fixed by envelope
  normalisation (0449be1).
