# Shimmer Reverb (0.6.0)

**Status:** design, awaiting review
**Replaces:** the Clouds reverb as the player's only reverb effect
**Reference:** Valhalla Shimmer architecture (pitch shifter in a reverb feedback
loop); Elysiera's *algorithms* (LFO-decorrelated shifting, in-loop filtering) —
ideas only, its GPL-3.0 code is not used.

## 1. Goal

Replace the plain Clouds reverb with a Valhalla-style shimmer reverb: the wet
tail is fed through a pitch shifter and back into the reverb input, so the tail
cascades upward and blooms. The user chose the control layout; there is no
plain-reverb mode afterwards — the single reverb effect *is* the shimmer.

## 2. Control map (no new art, no labels)

The UI has no free knobs. Three existing faders are repurposed; the two lowest
EQ faders are freed by shrinking the EQ to 8 bands. No text labels exist in the
skin and none are added — the background glyphs are decorative.

| Fader (left → right) | Today | Becomes | Range |
|---|---|---|---|
| reverb (x=628) | reverb wet/dry | **Shimmer amount** | 0..1, 0 = effect off |
| eq1 (x=664) | 60 Hz band | **Shift interval** | stepped: −12 / +7 / +12 / +19 / +24 st, default +12 |
| eq2 (x=714) | 170 Hz band | **Tone** (loop damping) | 0..1 → loop LP 500 Hz..16 kHz, log, default 0.65 (≈5 kHz) |
| eq3..eq10 | 310 Hz..16 kHz | 8-band EQ | ±12 dB, bands below |

Knob sprites (`knob_eq_1.png`, `knob_eq_2.png`) are reused unchanged; the artist
may redraw them later without any code impact.

**EQ bands (8):** `[310, 600, 1000, 3000, 6000, 12000, 14000, 16000]` Hz — the
top eight of the current ten, verbatim. Q rules: 1.0, except 0.9 above 10 kHz;
the sub-100 Hz Q=0.7 rule dies with the bass bands. The 10-band visualizer
spectrum is independent and untouched.

## 3. Signal flow

The proven Griesinger engine (`clouds_reverb.rs`) stays as the cascade core —
its internals do not change. The shimmer loop is new code around it, living in
`reverb_mix.rs` plus a new shared module `shimmer.rs`:

```
dry [L,R] ────────────────────────────────┐ (dry reference for the mix)
   │                                      │
   ▼                                      │
reverb input = (L+R)·input_gain + loop_return   (mono-summed, as today)
   │                                      │
   ▼                                      │
CloudsReverb::process ── raw wet tail ──► mix stage
   │                                      │
   ▼                                      │
tap = cross-tap (0.7·own + 0.3·other)      │
   ▼                                      │
Shimmer::process  (pitch shift → tone LP → safety HP)
   │                                      │
   └── × g(mix) = loop_return ────────────┘  (next frame)
```

Output stage unchanged: `dry·(1−mix) + clip(env-normalized wet)·mix`. The
envelope normalizer and its soft clip stay exactly as they are — see §5.

**Amount mapping.** The reverb fader is the dry/wet mix (loudness-stable thanks
to the envelope policy), and it also scales the cascade depth:
`g = G_MAX · mix^1.5`. At low mix the tail is a subtly shimmering verb; at full
it blooms. `mix ≤ 0.001` skips the whole reverb lock, as today.

## 4. The shifter (`src-tauri/src/audio/shimmer.rs`, shared module)

Dual delay-line pitch shifter — the engine family Valhalla and Elysiera actually
use in this loop. Eleventh `#[path]`-shared module; declared at the wasm-dsp
crate root (not inside an inline `mod audio`), unit tests run in both crates.

- **Two crossfaded read heads** per channel on one ring per channel, offset by
  half the ring. Reading faster than the write pointer shifts up.
- **Complementary raised-cosine windows over half the ring each**, with
  `a1 + a2 = 1` at every position. This is the invariant the textbook
  implementations get wrong: unity-gain heads with a short fade region sum to
  2 (+6 dB) and snap periodically — audible as a ~12 Hz thump. Constant sum is
  unit-tested directly.
- **Rate scaling (AGENTS.md 3.1.4):** ring length = 4096 at 48 kHz, scaled by
  `rate/48000` and rounded up to a power of two, heads and fade region scaling
  with it — the `layout()` pattern from `clouds_reverb.rs`.
- **Interpolation:** `dsp_utils::cubic_hermite`, indices wrapped manually.
- **LFO decorrelation** (Elysiera's trick): read speed modulated ±0.15 % by a
  ~0.2 Hz sine on L and cosine on R, so the two channels' comb artifacts
  decorrelate instead of ringing in unison.
- **Ratio slew:** a stop change slews the speed ratio over ~5 ms; no clicks.
- **Tone:** per-channel `Biquad::lowpass` (registers outside the coefficient
  struct, per the EQ pattern — coefficient updates never wipe state, rule
  3.1.3), updated in place per buffer.
- **Safety HP:** one-pole ~30 Hz DC blocker inside the loop; the −12 stop must
  not build sub-bass/DC runaway.
- No allocation after `new(sample_rate)`; all parameters set once per buffer.

## 5. Stability and loudness

- **Loop-gain cap:** worst-case recirculation ≈ `REVERB_TIME (0.55) × shifter
  gain (≤1 by the window invariant) × tone LP × g`. `G_MAX` starts at 1.0 and is
  tuned by the bounded-tail test until the product stays below 1 with margin —
  the same cliff discipline as rule 3.1.9 (`REVERB_TIME`), not a
  `feedback.min(1.2)` fantasy. Feedback > 1 modes are out of scope.
- **NaN self-heal:** the soft clip turns a diverged loop into `inf/inf = NaN`,
  which would poison every delay line permanently. Once per buffer the loop
  return is checked; on non-finite, `reverb.clear()` + shifter reset. One
  branch per buffer, self-recovering from dry input.
- **Envelope normalizer stays** (contra the proposal's §4.1): with a capped
  loop the cascade reaches a finite equilibrium, `env_wet` follows it and `wg`
  ducks the output mix — a self-riding shimmer, which is the wanted behavior.
  Swell-pumping is a listening-test item, not a assumed bug.

## 6. Parameter plumbing

- Desktop: `set_params(cutoff, pitch_st, reverb, eq: [f32; 8], speed, shift,
  tone)`. Shimmer params are applied under the existing per-buffer
  `shared.reverb` lock — no new locks (rule 3.1.2).
- Worklet: `Params` gains `eq: [f32; 8]`, `shift`, `tone`; `dspWorklet.js`
  forwards `msg.shift` / `msg.tone`; applied in `set_params`.
- `transport.ts`: `AudioParamsInput.eq` length 8, plus `shift`/`tone`.
- `main.ts`: fader params `shift`/`tone`/`eq0..eq7`; drag on `shift` quantizes
  to the nearest stop; `fx_reset` zeroes 8 bands and restores shift/tone
  defaults.
- `skin.json`: faders `eq1`→`{id:"shift", param:"shift", stops:[-12,7,12,19,24],
  value:12}`, `eq2`→`{id:"tone", param:"tone", range:[0,1], value:0.65}`,
  `eq3..eq10` params → `eq0..eq7`. `sprite/types.ts` grows the optional
  `stops` field; `skin.rs` needs no change (it validates structure, not param
  names). Third-party skins still naming `eq8/eq9` degrade gracefully (unknown
  indices already read as 0).
- Nothing persists params across restarts today; no migration needed.

## 7. Testing

- `shimmer.rs` (both crates): window-sum invariant; `g = 0` output bit-equal to
  the plain engine path; 10 s impulse + broadband noise at max settings,
  finite and bounded (peak < 5) across 32k/44.1k/48k/96k; octave test (440 Hz
  sine → growing 880 Hz energy); stop-change continuity (bounded per-sample
  delta); 10M-sample random-ratio wrap fuzz. Broadband material per rule
  3.1.11 — sines alone never showed the DC-bin burst.
- Existing reverb tests: RT60/loudness bounds re-measured with the cascade
  engaged; EQ tests move to 8 bands.
- Gates (§5 of AGENTS.md): `cargo test` + zero-warning `cargo check` in
  src-tauri; `cargo test` + wasm32 clippy in wasm-dsp; `npm test` +
  `npm run build`; then a real-browser listen (fresh tab, bundled track,
  shimmer engaged, throttled-CPU smoke) — the JS↔WASM boundary has passed
  cargo while emitting garbage before, so the browser check is not optional.

## 8. Docs & release

During implementation: rewrite the reverb chapter of `docs/DSP.md`; add
AGENTS.md 3.1 invariants (12: complementary windows sum to unity; 13: shimmer
loop-gain cap + NaN self-heal); README effect description. Ship as **0.6.0** —
behavior change (reverb identity, EQ shape) — with the usual seven-site version
bump and changelog at tag time.

## 9. Out of scope (phase 2 candidates)

Cascaded second shifter (+12→+19 pitch clusters, Elysiera's series pair);
reverse-shift modes; a "size" knob (touches `layout()` offsets and the
`REVERB_TIME` cliff — needs its own design); parameter persistence.
