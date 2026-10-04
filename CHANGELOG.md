# Changelog

All notable changes to the Misima Hybrid Player. Versions follow semver
loosely; the version lives in `app/package.json`, `app/src-tauri/tauri.conf.json`,
`app/src-tauri/Cargo.toml` and `app/wasm-dsp/Cargo.toml` and must be bumped
together.

> **Gap:** 0.3.0–0.3.2 shipped and are tagged in git but were never recorded
> here. Reconstruct them from `git log v0.2.1..v0.3.2` if the detail is ever
> needed; nothing below depends on them.

## [Unreleased]

### Fixed
- **Arch Linux gets a working package** — the AppImage aborts on startup there.
  It carries support libraries built on Ubuntu 22.04 which shadow the host's on
  a rolling distro, and the WebKit web process dies on a JavaScriptCore
  assertion before a window ever appears. `release.yml` now converts the `.deb`
  into a native `.pkg.tar.zst` with `debtap` and attaches it to the release.
  The `.deb` is the right source: it bundles nothing and declares webkit2gtk as
  a dependency, so the whole stack resolves to the host's own libraries.
  Tauri's bundler has no pacman target and no option to exclude individual
  libraries from the AppImage, so converting is cleaner than patching the
  AppImage.

## [0.4.1] — 2026-10-04

First tagged build of the desktop app: `release.yml` builds Windows, macOS
(universal) and Linux installers on the `v*` tag. 0.4.0 was never tagged, so
this is also the first time the installers exist for the web-build era.

### Fixed
- **Animated sprites no longer bleed past the player** — the `anim/*_sheet.png`
  cells carry an opaque black backdrop, and `screen` blending only erases black
  where something is already painted. Over the plate's transparent gaps the
  black cell survived compositing as a hard square: four of them, invisible
  against a dark desktop and glaring against a light one. The background and
  its still overlays are now flattened once into a `plate` canvas that serves as
  both the render blit and a mask, and each cell is cut to that alpha with
  `destination-in` before it is blended. Worst-case leaked pixels per frame fell
  from 4,137 to 49. Affects both runtimes.
- **Right-click is no longer an interaction** — it opened the browser context
  menu ("Save image as / Inspect") over what is a picture of a player, and
  because `pointerdown` fires for every button it was also grabbing faders and
  calling `startDragging()`, which would drag the window across the screen.
  The menu is suppressed except on credit links, and `pointerdown` now ignores
  everything but the primary button.
- **No magenta faders during load** — the render loop starts on the skin
  manifest, well before the sprites, and `drawFader()` fell back to a `#ff4fd8`
  block in that window. Every fader flashed a pink slab over the dark page for
  about half a second. It now draws nothing until its knob arrives.

### Added
- **Page backdrop on the web build** — the tab was flat black; it now carries the
  artwork at cover fit, shipped as the artist's PNG untouched with no scrim and
  no blur. Web only, stripped from the desktop bundle.

### Changed
- **`agents.md` renamed to `AGENTS.md`**, matching how the repo already referred
  to it. The lowercase references in `README.md`, `docs/DSP.md`,
  `clouds_reverb.rs` and `player.rs` were fixed in the same commit, since they
  would not resolve on a case-sensitive filesystem.
- **README** — the Pages link and the credits (Wit Chu's *Once* album, used with
  his permission) moved to the top of the file instead of living in a footer
  section.
- **Pitch fader travel** shortened from 228 to 200 artboard pixels, so the knob
  stops short of the panel edge.

## [0.4.0] — 2026-10-04

### Added
- **Web build** — the whole player as a static site, deployed at
  <https://schrab.github.io/misima-hybrid-player/>. Same skin, same DSP, no
  install. Deployed by a new `.github/workflows/pages.yml` on every push to
  `main`; `release.yml` and the desktop installers are untouched by it.
- **`app/wasm-dsp/`** — the DSP chain compiled to WebAssembly. It contains no
  DSP of its own: eight platform-free modules are `#[path]`-included from
  `src-tauri/src/audio/`, so the browser and desktop run the same Rust. No
  cpal, tauri, symphonia or parking_lot in its dependency graph. Compiled with
  `+simd128`, without which rustfft falls back 3–4x slower than the 128-sample
  render quantum allows.
- **`src/worklet/dspWorklet.js`** — the `dsp-processor` AudioWorklet, bundled by
  esbuild into a single import-free file with the wasm-bindgen glue inlined.
- **`src/transport.ts`** — a `Transport` interface with `TauriTransport` and
  `WebTransport` implementations, so `main.ts` keeps one code path and no longer
  imports `@tauri-apps/api` directly.
- **Bundled demo music** — four tracks by Wit Chu, used with his permission, in
  `app/public/music/`. Only the first is fetched on page load; the rest load
  when the listener reaches them.

### Changed
- **DSP modules split** — `Reverb` (with `follow` and `mix_reverb_frame`) moved
  to `audio/reverb_mix.rs`, and `Stretcher` to `audio/stretcher.rs`, so both
  crates can share them. Behaviour unchanged; the desktop plays identically.
- **Verification gate widened** — `cargo test` in `app/wasm-dsp` is now part of
  the required checks, since the shared modules' tests run there too.
- **Web build sizing** — the canvas fits the viewport and refits on resize;
  zoom no longer applies twice (it was setting both a CSS size and a
  `transform: scale()`).

### Fixed
- **Web: blank page in Chrome.** `AudioContext.resume()` never settles when the
  autoplay policy blocks it — pending forever, not rejected. Awaiting it hung
  initialisation before the first frame. Now raced against a timeout and only
  attempted after a user gesture.
- **Web: ~30 s blank page.** Sprites were awaited one at a time, ~135 serial
  requests. They now load in parallel, and the render loop starts as soon as the
  skin manifest parses instead of after the last image.
- **Desktop installers no longer carry web-only assets** — `dist/wasm` and
  `dist/music` are stripped from non-Pages builds.

## [0.2.1] — 2026-10-03

### Added
- **`audio/dsp_utils.rs`** — shared DSP utility module: `cubic_hermite`,
  `read_stereo_isize`, `read_stereo_f64`, `read_mono`. Eliminates duplicate
  implementations that lived in `phase_vocoder.rs` and `wsola.rs`.
- **`EqState::process_frame`** — dedicated single-frame EQ method that avoids
  the `chunks_mut` iterator overhead of `process_interleaved` in the audio
  callback's per-sample loop.
- **`Reverb::process_with_gain`** — accepts a pre-computed `wet_gain`, enabling
  per-buffer hoisting instead of per-sample computation.

### Changed
- **Visualizer taps moved post-EQ** — spectrum and waveform displays now show
  the signal after EQ processing, matching user expectations when boosting or
  cutting bands.
- **Anti-alias filter on resampler** — `resample_interleaved` now applies a
  2nd-order Butterworth lowpass at 90% of the target Nyquist when downsampling
  (e.g. 96 kHz → 44.1 kHz), preventing aliased high-frequency content.
- **Reverb envelope is sample-rate-independent** — `env_attack` and
  `env_release` are now computed from the actual device rate
  (`exp(-1 / (sr × 0.3))`) instead of being baked as constants for 44.1 kHz.
  Fixes pump artifacts at 96 kHz where the release was 2× faster.
- **`wet_gain()` hoisted per-buffer** — computed once per callback buffer and
  passed to `process_with_gain()`, matching the code's documented intent.
- **`play_pos` lock consolidated** — locked once at the start of each callback,
  used as a local variable, written back once at the end (was 2–3 lock/unlock
  cycles per callback).
- **`Reverb::clear()` resets `env_dry`** — prevents stale envelope state from
  leaking across seeks.
- `docs/DSP.md` updated: signal chain diagram, reverb envelope docs, module map.
- `AGENTS.md` architecture diagram updated for post-EQ taps and anti-alias SRC.

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
- Test suite 33 → 39 tests; `AGENTS.md` verification protocol updated.

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
