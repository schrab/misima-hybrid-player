# DSP Engine Documentation

This is the review guide for everything under `app/src-tauri/src/audio/`. It
covers the signal chain, each engine's design and measured trade-offs, the
invariants that keep the real-time thread safe, and how the test suite verifies
behaviour. Read this before modifying any DSP code; the module headers repeat
the critical parts, but this document explains the reasoning.

**Eight of these modules are shared with the browser build.** `app/wasm-dsp/`
`#[path]`-includes them and compiles them to WebAssembly, so the WSOLA, the
vocoder, the EQ, the spectrum analyzer and the reverb are the same Rust on both
platforms. See §8, *Shared vs desktop-only*, for the list and the rules that
follow from it.

---

## 1. Signal chain

```
decoded source buffer (device rate, f32 interleaved)
        │  ← anti-alias Butterworth lowpass when downsampling (resample_interleaved)
        ▼
bit-perfect bypass ──or──► Stretcher (stretcher.rs)
        │                    ├─ stretch = speed/pitch ≤ 1 → PhaseVocoder
        │                    └─ stretch > 1              → WsolaProcessor
        │                    then Cubic Hermite resample by pitch ratio
        │                    (cubic_hermite lives in dsp_utils.rs, shared by both engines)
        ▼
10-band peaking EQ (eq.rs, RBJ biquads, in-place coefficient updates)
        ├─► spectrum tap (48 log-spaced bins, rustfft)  ← post-EQ
        ├─► waveform tap (226-pt decimated, echo scope) ← post-EQ
        ▼
stereo reverb (clouds_reverb.rs) + dry/wet balance + envelope gain (reverb_mix.rs::Reverb)
        ▼
master lowpass (lpf.rs, 24 dB/oct) — identity (bypass) at the fader's top
        ▼
soft clip, cpal output
```

Every stage runs on the cpal audio callback thread (see AGENTS.md §3.1 for the
real-time rules: no allocation, no per-sample locks, no delay-line resets).
`SharedPlay` is the only state shared with the UI thread; parameters are
hoisted once per callback buffer, never per sample.  `play_pos` is locked once
at the start of each callback and written back once at the end; `wet_gain()` is
likewise computed once per buffer and passed to each per-sample reverb call via
`process_with_gain`.

**Engine selection** (`stretcher.rs::Stretcher`): the vocoder handles
`stretch ≤ 1`, the WSOLA everything else. The split is not arbitrary — see
§3.3. Both engines are constructed up front; switching re-seeds the newly
active engine at the live position so a fader crossing never resumes from
stale state.

---

## 2. Source of truth for parameters

| Parameter | Range | Where clamped |
|---|---|---|
| Cutoff | 30 – 20000 Hz (≥ 20000 = bypass) | `player::set_params` |
| Speed (tempo) | 0.5 – 2.0 | `player::set_params` |
| Pitch | ±12 st (±1 octave) | `player::set_params`, both engines |
| Reverb mix | 0 – 1 | `player::set_params` |
| EQ | ±12 dB per band | `eq.rs` |

Pitch is clamped to ±1 octave in the Rust layer to match the fader range in
`skin.json` (`range: [-12, 12]`). At 2 octaves the WSOLA runs at 8x incoherent
grain overlap — audibly granular — so the clamp is a design boundary, not a
safety net. Do not widen it without changing §3.

The pitch fader's range lives in `skin.json`, which is user-owned art config:
**never regenerate `skin.json`**, only hand-edit it.

---

## 3. The time/pitch engines

Both engines have the same outward contract (see `Stretcher::process`): consume
a source slice, produce `out_frames` stereo frames, report `finished`, and
answer `get_play_pos(speed, pitch)` — the source frame currently being *heard*,
which must compensate for the engine's internal latency and buffering. A new
engine must implement all four or the playlist progress bar and seeking break.

The tempo/pitch algebra both engines share: the engine produces a stream with
tempo `stretch = speed / pitch` (pitch unchanged), and a Cubic Hermite
resampler reads that stream with `step = pitch`, which multiplies tempo by
`step` and pitch by `step`. Net: tempo `speed`, pitch `pitch`. This is
verified by test (`pitch_is_accurate_and_independent_of_speed`).

### 3.1 WSOLA (`wsola.rs`) — tempo, and pitch-down

Waveform-Similarity Overlap-Add: 1024-sample grains (periodic Hann, 50%
overlap, exact unity reconstruction), advanced 512 output samples per grain
while consuming `512 · stretch` source samples. A ±256-sample cross-correlation
search (256-sample template, mono-summed so L and R share one shift and the
stereo image is preserved) aligns each grain before overlap-add, which
suppresses the comb-filtering naive OLA produces.

Strengths: pitch-accurate (FFT-verified to <1 cent), cheap (3% of one core at
+1 octave, 0.5% at −1), and in its *expansion* direction (`stretch > 1`) the
grains don't overlap incoherently, so quality is high.

Weakness: in *compression* (`stretch < 1`, i.e. pitch-up) consecutive grains
overlap `WIN / (HOP · stretch)` times with related but phase-incoherent
content — 4x at +1 octave. The ear hears that as "granular". This is the
reason the vocoder exists.

Known past panic: `fifo_read_pos` underflow near track end (audio-thread death
→ silent output). All `usize` arithmetic there is saturating/clamped; keep it
that way (AGENTS.md §3.1.7).

### 3.2 Phase vocoder (`phase_vocoder.rs`) — pitch-up

STFT engine: 2048-point frames, periodic Hann analysis and synthesis windows.
For each frame: FFT both channels, estimate per-bin instantaneous frequencies
(expected advance `ω·ha` plus the wrapped deviation `princarg`), resynthesise
with locked phases, inverse FFT, weighted overlap-add (output divided by the
running sum of squared synthesis windows).

The structure that must not change:

1. **Synthesis hop is FIXED at `n/4` (512).** The analysis hop carries the
   ratio: `ha = 512 · stretch`, so stream tempo is `ha/hs = stretch`. The
   synthesis hop cannot vary, because the Hann window's squares sum to exactly
   1.5 at hop n/4 — constant — making the weighted overlap-add exact at every
   ratio. (An earlier version varied the synthesis hop instead: level was
   accurate at stretch 1, +2.3 dB by stretch 2, divergent at stretch 4 where
   consecutive frames stop overlapping. Table measured, test
   `tail_decays_in_a_musical_time` analog: `unity_ratio_reconstructs_the_input`
   guards level, `output_is_stable_and_finite` guards divergence.)

2. **The tempo convention inverts relative to the WSOLA.** A vocoder emitting
   `hs` samples per `ha` input samples runs at tempo `ha/hs`, so
   `hs = ha / (speed/pitch)`. The WSOLA's grain advances *source* position by
   `HOP · stretch` per `HOP` outputs, so its tempo is `stretch` directly. The
   inverted form is invisible to pitch tests (pitch depends only on the
   resampler `step`) — it manifests as a wrong tempo. Verified via output
   duration, not just frequency.

3. **Strict identity phase locking (Laroche & Dolson 1999), seeded.** Only
   *peak* bins accumulate phase (advanced by their own instantaneous frequency
   over the synthesis hop); every other bin is rebuilt each frame as
   `peak_phase + (its own measured analysis phase − the peak's)`. Two
   failed formulations are recorded so they are not retried:
   - per-bin accumulators for *all* bins diverge when a bin's owning peak
     changes between frames (−5.4 dB measured);
   - anchoring to the previous frame's *analysis* phase only works when
     analysis and synthesis hops are equal (+143 cents at stretch 0.5
     otherwise).
   The accumulator is seeded from the measured analysis phases at the first
   frame that carries signal (`frame_max > 1e-6`); starting from zero leaves
   every bin missing its constant phase term and partials sum destructively.
   The seed is cleared on `reset` (seek).

4. **Peak detection needs a magnitude floor: −28 dB relative to the frame's
   strongest bin.** A Hann-windowed partial sprouts sidelobe local maxima at
   −31.5 dB, three bins either side of its main lobe; without the floor those
   sidelobes win nearest-peak assignment for the main lobe's edge bins and the
   partial is split against itself. Level loss from this depended on where the
   partial sat relative to the bin grid (300 Hz lost 4.4 dB while 440 Hz was
   exact) — if you see frequency-dependent level loss, look at region
   assignment first.

5. **Stereo without mono sums.** Instantaneous frequencies are estimated once
   from the *mid* spectrum — the complex average of the two channels' spectra,
   costing no extra transform — so both channels are warped identically and
   cannot drift apart. Peak picking and locking then run per channel, so each
   channel keeps its own bin phases and its stereo image.
   `stereo_image_survives` fails if channels collapse (|correlation| ≥ 0.9).

`identity_phases_reconstruct_exactly` is the structural test: with the phase
processing disabled via the `identity_phases` flag the engine is a plain
analysis→synthesis STFT and must reconstruct within ±1%. If that test fails,
the overlap-add plumbing is broken; if it passes but the phase tests fail, the
phase logic is broken. This split is the fastest way to localise a fault — use
it before instrumenting anything else.

Known limits: transients smear more than WSOLA (inherent to STFT methods);
CPU is higher than the WSOLA and *increases* as stretch decreases (analysis
hop shrinks → more frames per second). At stretch 0.25 the analysis hop is 128
samples, i.e. 16× overlap and 4× the unity-ratio frame rate. Not yet
benchmarked in release; the WSOLA numbers above are the reference point.

### 3.3 Why the split at `stretch ≤ 1`

`stretch = speed/pitch`, so `stretch ≤ 1` is exactly "pitch-up faster than
tempo-up" — the region where the WSOLA compresses time and sounds granular.
For `stretch > 1` the WSOLA expands, which is its good direction, and the
vocoder would gain nothing (its advantage is independence from compression
ratio, which only matters when compressing). If the vocoder is later extended
past stretch 1, re-verify level linearity first — the analysis hop grows with
stretch and the phase-propagation reliability degrades once `ha` exceeds `n/4`.

---

## 4. Reverb (`clouds_reverb.rs`, `reverb_mix.rs::Reverb`)

Stereo feedback-delay network ported from Mutable Instruments Clouds (MIT,
© 2014 Emilie Gillet — attribution is in the module header and must stay).
Dattorro/Griesinger topology: four allpass input diffusers, two cross-coupled
feedback loops (2 diffusers + long delay each) with separate output taps, so
the L/R tails decorrelate — the reason the old mono-summed Schroeder reverb
was replaced. LFOs at 0.5 Hz and 0.3 Hz modulate the first diffuser and the
long delays (shimmer/smear).

Deviations from the original, all deliberate:
- `f32` delay storage instead of 12-bit packed `u16` (that was a Cortex-M4 RAM
  constraint; here it is only noise and headroom loss).
- Layout recomputed per sample rate: **both lengths and offsets** scale by
  `sample_rate / 32000`. Scaling lengths without offsets makes `del1` overrun
  `del2` and the loop diverges (regression test:
  `lines_do_not_overlap_at_any_rate`).
- Tail length is a fixed `REVERB_TIME = 0.55` (RT60 ≈ 2.8 s, consistent
  within 0.15 s across 32k/44.1k/48k/96k), NOT tied to the fader. The loop has
  a stability cliff: musical to ~0.6, 10 s ringing at 0.65, build-up past 0.9.
  The original hardware caps its internal amount at 0.54, so its `krt` never
  exceeds 0.69 — do not map a 0..1 fader onto the original's
  `0.35 + 0.63·amount` (reaches 0.98, diverges). Guarded by
  `tail_decays_in_a_musical_time`.

`reverb_mix.rs::Reverb` owns the policy the DSP must not: the dry/wet crossfade and
the envelope-normalised wet gain. The raw tail level varies ~20 dB between
tonal and broadband material, so no fixed wet gain stays balanced (AGENTS.md
§3.1.8). `TARGET = 1.8` sits above unity because the soft clip `w/√(1+w²)`
costs ~3 dB at `w = 1`; measured wet level lands at +0.1 dB (half mix) and
+1.3 dB (full mix) against dry on broadband material. Never calibrate reverb
gain on a sine — tones phase-cancel against their own tails.

The envelope's attack and release coefficients are stored in the `Reverb` struct
and computed from the actual device sample rate at construction:
`env_release = exp(-1 / (sr × 0.3))` gives a ~0.3 s release at any rate (the
old constants were correct only at 44.1 kHz — at 96 kHz the release halved to
~0.15 s and caused pump artifacts).  `wet_gain()` is hoisted once per callback
buffer and passed to `process_with_gain()` — per-sample computation was
redundant because the envelope moves slowly relative to individual samples.

Seeks flush the reverb (`shared.reverb.lock().clear()` on `seek_gen` change) so
the previous position's tail does not bleed across a jump.  `clear()` resets
both `env_wet` and `env_dry` to zero.

---

## 5. Master lowpass (`lpf.rs`)

The former volume fader is now a resonant master lowpass — the last stage
before the output clamp, after the reverb, so the tail darkens with everything
else while the reverb's envelope follower still sees the full-band signal.

- **Topology**: two cascaded RBJ lowpass biquads (`Biquad::lowpass`), 24 dB/oct.
  The Butterworth pole Qs (0.5412, 1.3066) are multiplied by `RESONANCE = 1.4`,
  which lifts a ~+4 dB peak at the cutoff. RBJ biquads are stable for any
  finite Q, so this sits far from self-oscillation.
- **Bypass at the top**: `cutoff ≥ 20 kHz` swaps in identity coefficients and
  clears the registers once, so the fader's top position is bit-transparent —
  a real 20 kHz lowpass would still shave the top octave. An epsilon no-op
  guard makes repeat applications free, which matters because `set_params`
  fires on every pointermove of *any* fader drag.
- **Glitch-free sweeps**: coefficients update in place; the per-stage,
  per-channel delay registers live outside the coefficient structs (the same
  discipline as `EqState`) and keep their memory across a sweep.
- **Flush on seek**: a seek or track change clears the filter together with
  the stretcher and the reverb, so a closed filter cannot ring across the gap.
- **Fader mapping**: `skin.json` range 30 Hz – 20 kHz with `curve: "log"`
  (knob travel and wheel steps move multiplicatively); the DSP clamps to the
  same bounds. 30 Hz is deliberately not silence — the sub-bass floor stays
  alive ("almost closed").

---

## 6. Test methodology

`cargo test` from `app/src-tauri` (51 passing, 1 ignored smoke test, zero
warnings is the bar — AGENTS.md §5). The tests are the specification; the
useful ones to understand before touching DSP:

| Test | Pins |
|---|---|
| `unity_ratio_reconstructs_the_input` | vocoder transparency at stretch 1, swept 300–2000 Hz (level ±10%, pitch ±3%) |
| `pitch_is_accurate_and_independent_of_speed` | ±1 octave at 0.5×/1×/2× speed, ±30 cents, measured per channel |
| `identity_phases_reconstruct_exactly` | pure STFT reconstructs ±1% — separates overlap-add faults from phase-logic faults |
| `output_is_stable_and_finite` | no divergence/NaN on noise across ratios |
| `stereo_image_survives` | channels stay decorrelated (|corr| < 0.9) |
| `lines_do_not_overlap_at_any_rate` | reverb layout scales both offsets and lengths |
| `tail_decays_in_a_musical_time` | reverb loop gain stays off the stability cliff |
| `reverb_mix_loudness_constant` | wet level within ±4 dB of dry at 0/50/100% mix |
| `open_is_bit_transparent` / `closing_then_reopening_is_transparent_again` | fader top = identity; state clears once on re-entry |
| `two_octaves_above_cutoff_lands_near_48db` | the 24 dB/oct slope |
| `resonance_bump_at_the_cutoff` | the resonant Q pair peaks at the cutoff |
| `sweep_stays_finite_and_bounded` | no divergence while the coefficients move |
| `reset_into_the_middle_of_a_track_does_not_explode` | a reset at a non-zero source position primes the overlap-add correctly |
| `pitch_gestures_with_reverb_stay_bounded` | crossing the pitch fader's midpoint and cueing while pitched up stay within headroom |

Measurement conventions that have bitten us:

- Measure **per channel**. A mono downmix of two differently-pitched channels
  peaks at whichever channel came out stronger — that once masqueraded as a
  +701-cent pitch error.
- Measure **level as well as frequency**. A phase error can be frequency-exact
  and still −4 dB.
- A level loss that **depends on fractional bin position** is a region-
  assignment bug, not a gain bug.
- The identity-bypass flag (`identity_phases`) splits overlap-add faults from
  phase-logic faults in one run. Try it before adding instrumentation.

---

## 7. Known limitations / future work

- Transient smear on percussive material through the vocoder (inherent to
  STFT); WSOLA remains better for pure tempo.
- Vocoder CPU is unbenchmarked in release. It scales as `1/stretch` (more
  frames per second at deep pitch-up); if it matters on weak hardware, the
  lever is FFT size or capping the analysis overlap.
- `Stretch ≤ 1` routing means extreme settings (speed 0.5 + pitch +12 st) run
  the vocoder at its deepest compression; re-verify level linearity if the
  routing range is ever widened.
- WSOLA remains the only tempo engine; Elastique-class quality would need
  multi-resolution vocoder + transient detection (Rubber Band is GPL-2.0+ —
  do not adopt without a licence decision).

## 8. Module map

| File | Purpose |
|---|---|
| `decoder.rs` | Symphonia decode to interleaved f32 PCM |
| `dsp_utils.rs` | Shared DSP utilities: `cubic_hermite`, `read_stereo_*`, `read_mono` |
| `eq.rs` | 10-band RBJ biquad EQ; `process_frame` for single stereo frames, `process_interleaved` for bulk |
| `lpf.rs` | 4-pole resonant master lowpass (cutoff fader); identity + cleared state at the top |
| `phase_vocoder.rs` | Stereo STFT pitch shifter with Laroche & Dolson phase locking |
| `wsola.rs` | WSOLA time-stretcher + Cubic Hermite resampler |
| `clouds_reverb.rs` | Dattorro/Griesinger FDN reverb (Clouds port) |
| `spectrum.rs` | 1024-point FFT → 48 log-spaced bins for the visualizer |
| `reverb_mix.rs` | Reverb dry/wet balance + envelope-normalized wet gain policy. **Platform-free.** |
| `stretcher.rs` | Engine selector between the phase vocoder and the WSOLA. **Platform-free.** |
| `player.rs` | Playback engine: cpal output, `SharedPlay`, stream ownership. **Desktop-only.** |
| `mod.rs` | Module declarations and re-exports |

### Shared vs desktop-only

The web port (`app/wasm-dsp/`) `#[path]`-includes the platform-free modules
directly from this directory, so a DSP fix lands on both platforms at once and
the two builds cannot silently diverge. Anything that touches cpal, tauri,
parking_lot, or crossbeam stays in `player.rs` and is reimplemented in the
worklet instead.

**Platform-free (shared):** `clouds_reverb`, `dsp_utils`, `eq`, `lpf`,
`phase_vocoder`, `spectrum`, `wsola`, `reverb_mix`, `stretcher`.

**Desktop-only:** `player.rs` (cpal stream + `SharedPlay`), `decoder.rs`
(Symphonia — the browser uses `decodeAudioData`).

CI runs `cargo check --target wasm32-unknown-unknown` in `wasm-dsp/` on every
PR that touches `src/audio/`.
