# Shimmer Reverb Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the Clouds reverb with a Valhalla-style shimmer reverb (pitch-shifted feedback cascade) controlled by the existing reverb fader plus two faders freed by shrinking the EQ to 8 bands.

**Architecture:** The proven Griesinger engine (`clouds_reverb.rs`) stays as the cascade core. A new shared module `shimmer.rs` (dual delay-line pitch shifter + tone lowpass + DC blocker) is wired into `reverb_mix.rs` as a feedback loop: tap wet tail → cross-tap → shift → damp → `g(mix)` → back into the reverb input. Params reach both runtimes through the existing `set_params` paths.

**Tech Stack:** Rust (src-tauri + wasm-dsp, `#[path]`-shared modules), TypeScript/Canvas frontend, skin.json manifest.

**Spec:** `docs/compose/spec/shimmer-reverb.md`

## Global Constraints

- Commit **straight to main** — no branch, no PR (standing user instruction). Stage precisely: `gfx/`, `music/`, `.zcode/` are always untracked, never stage them.
- Zero allocations in the per-frame audio path; no new locks; per-sample code takes no mutexes (AGENTS.md 3.1.1–3.1.2).
- Filter coefficient updates never wipe delay registers (AGENTS.md 3.1.3).
- Every delay length and offset scales with sample rate (AGENTS.md 3.1.4).
- The head crossfade gains must satisfy `a1 + a2 = 1` at every position (spec §4).
- Shimmer loop gain is hard-capped: worst-case recirculation `REVERB_TIME (0.55) × shifter (≤1) × tone (≤1) × g` must stay < 1 with margin; `G_MAX = 1.0` initial (spec §5).
- Per-buffer NaN self-heal on the loop return: non-finite → `inner.clear()` + `shimmer.clear()` + rets zeroed (spec §5).
- New shared module declared at the **crate root** of `wasm-dsp/src/lib.rs` (`#[path = "../../src-tauri/src/audio/shimmer.rs"]`), added to the `pub mod audio` shim — never inside an inline module (AGENTS.md §6).
- Tests use broadband LCG noise for divergence/cascade checks, never sine-only (AGENTS.md 3.1.11).
- Gates per task: `cargo test` + zero-warning `cargo check` in `app/src-tauri`; `cargo test` + `cargo clippy --target wasm32-unknown-unknown` in `app/wasm-dsp`; `npm test` + `npm run build` in `app`. Rust needs `export PATH="$HOME/.cargo/bin:$PATH"` in a fresh shell.
- No version bump / changelog in this plan — that happens at tag time.
- English replies, terse commit messages in the repo's lowercase `feat:`/`docs:` style.

---

### Task 1: The `shimmer.rs` shared module

**Files:**
- Create: `app/src-tauri/src/audio/shimmer.rs`
- Modify: `app/src-tauri/src/audio/mod.rs` (add `pub mod shimmer;` after `pub mod reverb_mix;`)
- Modify: `app/wasm-dsp/src/lib.rs` (add the `#[path]` declaration + shim re-export)

**Interfaces:**
- Consumes: `dsp_utils::cubic_hermite(y_m1: f32, y0: f32, y1: f32, y2: f32, t: f32) -> f32`, `eq::Biquad` (`Biquad::lowpass(sample_rate, freq, q) -> Biquad`, `tick(&self, x: f32, z1: &mut f32, z2: &mut f32) -> f32`).
- Produces (used by Task 2): `Shimmer::new(sample_rate: f32) -> Shimmer`, `set_shift(&mut self, semitones: f32)`, `set_tone(&mut self, t: f32)` (0..1), `clear(&mut self)`, `process(&mut self, in_l: f32, in_r: f32) -> (f32, f32)` (loop return, un-gained), and `#[cfg(test)] poison_for_test(&mut self)`.

- [ ] **Step 1: Write the module skeleton with failing tests** — create `shimmer.rs` with the full code below; the tests reference the API so the crate fails to compile until the impl is in (write both, then run).

```rust
//! Dual delay-line pitch shifter for the shimmer reverb loop.
//!
//! Two read heads half a ring apart sweep one ring per channel at the pitch
//! ratio, carrying complementary raised-cosine gains — `a1 + a2 = 1` at every
//! position, which is what keeps the crossfade free of the periodic 6 dB
//! thump a short fade region produces. Reads are cubic Hermite; the ring
//! length scales with the device rate the way `clouds_reverb::layout` scales
//! its lines (AGENTS.md 3.1.4). A slow sine modulates the left head and a
//! cosine the right, decorrelating the channels' comb structure (Elysiera's
//! trick — algorithm only, its GPL code is not used).
//!
//! Everything is allocated once in `new`; `process` allocates nothing and
//! takes no locks. Tone updates replace the `Biquad` coefficients in place
//! and never wipe the per-channel registers (AGENTS.md 3.1.3).

use std::f32::consts::TAU;

use crate::audio::dsp_utils::cubic_hermite;
use crate::audio::eq::Biquad;

/// Ring length at [`REF_RATE`]: ~85 ms at 48 kHz — long enough that the
/// half-ring crossfade stays in the lush-chorus regime, short enough for cache.
const BASE_RING: f32 = 4096.0;
/// Rate the ring length is written for.
const REF_RATE: f32 = 48_000.0;
/// Ring floor for very low rates, so the crossfade never gets absurdly short.
const MIN_RING: usize = 1024;
/// Shift range in semitones — the fader's stops span this.
const SHIFT_MIN: f32 = -12.0;
const SHIFT_MAX: f32 = 24.0;
/// Head modulation: 0.2 Hz, ±0.15 % of read speed.
const LFO_HZ: f32 = 0.2;
const LFO_DEPTH: f32 = 0.0015;
/// Tone fader → loop lowpass: `500 Hz · 32^t` spans 500 Hz..16 kHz over 0..1.
const TONE_LO_HZ: f32 = 500.0;
const TONE_SPAN: f32 = 32.0;
/// DC-blocker corner: the −12 stop must not build sub-bass or DC in the loop.
const DC_HZ: f32 = 30.0;
/// Ratio slew time constant in seconds — stop changes glide, not click.
const RATIO_SLEW_S: f32 = 0.005;
/// Default tone (fader position), ≈ 4.8 kHz lowpass.
const DEFAULT_TONE: f32 = 0.65;

/// Raised-cosine head gain at `phase` samples behind the write head.
/// `head_gain(p) + head_gain(p + len/2) == 1` exactly, for every `p`.
fn head_gain(phase: f32, len: f32) -> f32 {
    0.5 - 0.5 * (TAU * phase / len).cos()
}

/// Dual delay-line pitch shifter. See the module docs.
pub struct Shimmer {
    sample_rate: f32,
    ring_l: Box<[f32]>,
    ring_r: Box<[f32]>,
    mask: usize,
    write: usize,
    /// Fractional ring position of each channel's first read head; the
    /// second head sits half a ring ahead. Read positions advance by the
    /// (slewed, LFO-modulated) ratio per frame while the write head steps 1.
    read_l: f32,
    read_r: f32,
    /// Current and target speed ratio `2^(semitones/12)`, slewed on changes.
    ratio: f32,
    target_ratio: f32,
    ratio_slew: f32,
    /// Shared LFO phase 0..1; L reads sin, R reads cos.
    lfo_phase: f32,
    lfo_inc: f32,
    /// Loop lowpass coefficients (shared) + per-channel register state.
    tone: Biquad,
    tz1_l: f32,
    tz2_l: f32,
    tz1_r: f32,
    tz2_r: f32,
    /// One-pole DC blockers, per channel.
    dc_a: f32,
    dc_x_l: f32,
    dc_y_l: f32,
    dc_x_r: f32,
    dc_y_r: f32,
}

impl Shimmer {
    pub fn new(sample_rate: f32) -> Self {
        let len = ((BASE_RING * (sample_rate / REF_RATE)).ceil() as usize)
            .max(MIN_RING)
            .next_power_of_two();
        Self {
            sample_rate,
            ring_l: vec![0.0; len].into_boxed_slice(),
            ring_r: vec![0.0; len].into_boxed_slice(),
            mask: len - 1,
            write: 0,
            read_l: 0.0,
            read_r: 0.0,
            ratio: 1.0,
            target_ratio: 2.0f32.powf(12.0 / 12.0),
            ratio_slew: (-1.0 / (sample_rate * RATIO_SLEW_S)).exp(),
            lfo_phase: 0.0,
            lfo_inc: LFO_HZ / sample_rate,
            tone: Biquad::lowpass(sample_rate, TONE_LO_HZ * TONE_SPAN.powf(DEFAULT_TONE), 0.707),
            tz1_l: 0.0,
            tz2_l: 0.0,
            tz1_r: 0.0,
            tz2_r: 0.0,
            dc_a: 1.0 - TAU * DC_HZ / sample_rate,
            dc_x_l: 0.0,
            dc_y_l: 0.0,
            dc_x_r: 0.0,
            dc_y_r: 0.0,
        }
    }

    /// Shift interval in semitones. The fader sends stop values; the engine
    /// clamps to the two-octave span and glides the ratio.
    pub fn set_shift(&mut self, semitones: f32) {
        let st = semitones.clamp(SHIFT_MIN, SHIFT_MAX);
        self.target_ratio = 2.0f32.powf(st / 12.0);
    }

    /// Loop damping, 0..1 (dark..bright), log-mapped to 500 Hz..16 kHz.
    /// Coefficients update in place; register state stays (3.1.3).
    pub fn set_tone(&mut self, t: f32) {
        let t = t.clamp(0.0, 1.0);
        let cutoff = TONE_LO_HZ * TONE_SPAN.powf(t);
        self.tone = Biquad::lowpass(self.sample_rate, cutoff, 0.707);
    }

    /// Drop the tail — the seek/track-load flush alongside the reverb.
    pub fn clear(&mut self) {
        self.ring_l.fill(0.0);
        self.ring_r.fill(0.0);
        self.write = 0;
        self.read_l = 0.0;
        self.read_r = 0.0;
        self.ratio = self.target_ratio;
        self.lfo_phase = 0.0;
        self.tz1_l = 0.0;
        self.tz2_l = 0.0;
        self.tz1_r = 0.0;
        self.tz2_r = 0.0;
        self.dc_x_l = 0.0;
        self.dc_y_l = 0.0;
        self.dc_x_r = 0.0;
        self.dc_y_r = 0.0;
    }

    /// Advance one stereo frame of the cascade: take the loop taps, return
    /// the pitch-shifted, damped signal the caller feeds back into the
    /// reverb input. Un-gained — the caller owns the depth.
    pub fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        self.ratio += (self.target_ratio - self.ratio) * self.ratio_slew;

        self.lfo_phase += self.lfo_inc;
        if self.lfo_phase >= 1.0 {
            self.lfo_phase -= 1.0;
        }
        let mod_l = 1.0 + LFO_DEPTH * (TAU * self.lfo_phase).sin();
        let mod_r = 1.0 + LFO_DEPTH * (TAU * self.lfo_phase).cos();

        let len = (self.mask + 1) as f32;
        self.read_l += self.ratio * mod_l;
        self.read_r += self.ratio * mod_r;
        // ratio stays within [0.25, 4] and len >= 1024, so one wrap suffices.
        if self.read_l >= len {
            self.read_l -= len;
        }
        if self.read_r >= len {
            self.read_r -= len;
        }

        let shifted_l = self.read_channel(&self.ring_l, self.read_l);
        let shifted_r = self.read_channel(&self.ring_r, self.read_r);

        self.ring_l[self.write] = in_l;
        self.ring_r[self.write] = in_r;
        self.write = (self.write + 1) & self.mask;

        // Tone damping first — it is what keeps the cascade from doubling
        // high-frequency energy every turn — then the DC blocker.
        let damp_l = self.tone.tick(shifted_l, &mut self.tz1_l, &mut self.tz2_l);
        let damp_r = self.tone.tick(shifted_r, &mut self.tz1_r, &mut self.tz2_r);
        let y_l = damp_l - self.dc_x_l + self.dc_a * self.dc_y_l;
        let y_r = damp_r - self.dc_x_r + self.dc_a * self.dc_y_r;
        self.dc_x_l = damp_l;
        self.dc_y_l = y_l;
        self.dc_x_r = damp_r;
        self.dc_y_r = y_r;
        (y_l, y_r)
    }

    /// Two heads half a ring apart with complementary raised-cosine gains.
    /// Reading "past" the write head is safe: the ring only ever holds past
    /// data, so those samples are the oldest cycle, not future ones.
    fn read_channel(&self, ring: &[f32], pos: f32) -> f32 {
        let len = (self.mask + 1) as f32;
        let phase = (self.write as f32 - pos).rem_euclid(len);
        let amp = head_gain(phase, len);
        let half = len * 0.5;
        let s1 = self.hermite_at(ring, pos);
        let s2 = self.hermite_at(ring, (pos + half) % len);
        s1 * amp + s2 * (1.0 - amp)
    }

    fn hermite_at(&self, ring: &[f32], pos: f32) -> f32 {
        let i = pos.floor() as usize;
        let t = pos - i as f32;
        let m1 = ring[(i + self.mask) & self.mask];
        let y0 = ring[i & self.mask];
        let y1 = ring[(i + 1) & self.mask];
        let y2 = ring[(i + 2) & self.mask];
        cubic_hermite(m1, y0, y1, y2, t)
    }

    /// Test hook for the NaN self-heal path in `reverb_mix`.
    #[cfg(test)]
    pub fn poison_for_test(&mut self) {
        self.ring_l.fill(f32::NAN);
        self.ring_r.fill(f32::NAN);
    }
}
```

Then the test module (broadband LCG noise per 3.1.11):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    /// LCG broadband noise, uncorrelated between channels.
    fn noise_frames(n: usize, amp: f32) -> Vec<[f32; 2]> {
        let (mut sl, mut sr) = (0x1234_5678u32, 0x9ABC_DEF0u32);
        (0..n)
            .map(|_| {
                sl = sl.wrapping_mul(1664525).wrapping_add(1013904223);
                sr = sr.wrapping_mul(1664525).wrapping_add(1013904223);
                [
                    (((sl >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * amp,
                    (((sr >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * amp,
                ]
            })
            .collect()
    }

    /// Naive DFT bin magnitude over a settled window.
    fn bin_mag(sig: &[f32], from: usize, freq: f64, sr: f64) -> f64 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &s) in sig[from..].iter().enumerate() {
            let w = TAU as f64 * freq * i as f64 / sr;
            re += s as f64 * w.cos();
            im -= s as f64 * w.sin();
        }
        (re * re + im * im).sqrt()
    }

    #[test]
    fn head_gains_are_complementary() {
        let len = 4096.0f32;
        for i in 0..1000 {
            let p = i as f32 * len / 1000.0;
            let sum = head_gain(p, len) + head_gain((p + len * 0.5) % len, len);
            assert!((sum - 1.0).abs() < 1e-5, "window sum {sum} at p={p}");
        }
    }

    #[test]
    fn ring_scales_with_rate() {
        assert_eq!(Shimmer::new(48_000.0).mask + 1, 4096);
        assert_eq!(Shimmer::new(44_100.0).mask + 1, 4096); // 3769 -> pow2
        assert_eq!(Shimmer::new(96_000.0).mask + 1, 8192);
        assert_eq!(Shimmer::new(8_000.0).mask + 1, 1024); // floor clamp
    }

    #[test]
    fn all_stops_all_rates_stay_finite_and_bounded() {
        for rate in [32_000.0f32, 44_100.0, 48_000.0, 96_000.0] {
            for st in [-12.0f32, 7.0, 12.0, 19.0, 24.0] {
                let mut sh = Shimmer::new(rate);
                sh.set_shift(st);
                sh.set_tone(1.0);
                let frames = noise_frames((rate * 5.0) as usize, 0.5);
                let mut peak = 0.0f32;
                for (i, f) in frames.iter().enumerate() {
                    let (l, r) = sh.process(f[0], f[1]);
                    assert!(
                        l.is_finite() && r.is_finite(),
                        "{rate} Hz {st} st: non-finite at frame {i}"
                    );
                    peak = peak.max(l.abs()).max(r.abs());
                }
                assert!(peak < 4.0, "{rate} Hz {st} st: diverged to {peak}");
            }
        }
    }

    #[test]
    fn shift_up_builds_octave_energy() {
        let (sr, f0) = (48_000.0f64, 440.0f64);
        let mut sh = Shimmer::new(sr as f32);
        sh.set_shift(12.0);
        sh.set_tone(1.0);
        let n = (sr * 2.0) as usize;
        let mut sig = Vec::with_capacity(n);
        for i in 0..n {
            let x = (TAU as f64 * f0 * i as f64 / sr).sin() as f32 * 0.5;
            let (l, _) = sh.process(x, x);
            sig.push(l);
        }
        let from = n / 2;
        let mag440 = bin_mag(&sig, from, f0, sr);
        let mag880 = bin_mag(&sig, from, f0 * 2.0, sr);
        assert!(
            mag880 > mag440 * 1.5,
            "expected 880 Hz ({mag880}) to dominate 440 Hz residue ({mag440})"
        );
    }

    #[test]
    fn stop_change_is_continuous() {
        let mut sh = Shimmer::new(48_000.0);
        sh.set_shift(12.0);
        sh.set_tone(1.0);
        let frames = noise_frames(48_000, 0.5);
        for f in &frames[..24_000] {
            let _ = sh.process(f[0], f[1]);
        }
        sh.set_shift(-12.0);
        let mut max_jump = 0.0f32;
        let mut prev = 0.0f32;
        for f in &frames[24_000..26_400] {
            let (l, _) = sh.process(f[0], f[1]);
            max_jump = max_jump.max((l - prev).abs());
            prev = l;
        }
        assert!(max_jump < 0.35, "stop change clicked: max jump {max_jump}");
    }

    #[test]
    fn wrap_fuzz_never_panics_or_nan() {
        let mut sh = Shimmer::new(48_000.0);
        sh.set_tone(DEFAULT_TONE);
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32 / u32::MAX as f32 - 0.5) * 1.0
        };
        let stops = [-12.0f32, 7.0, 12.0, 19.0, 24.0];
        for i in 0..10_000_000u64 {
            if i % 480_000 == 0 {
                sh.set_shift(stops[((i / 480_000) % 5) as usize]);
            }
            let (l, r) = sh.process(next(), next());
            if i % 100_000 == 0 {
                assert!(l.is_finite() && r.is_finite(), "fuzz non-finite at {i}");
            }
        }
    }
}
```

- [ ] **Step 2: Register the module in both crates.** `app/src-tauri/src/audio/mod.rs`: add `pub mod shimmer;` after line 11 (`pub mod reverb_mix;`). `app/wasm-dsp/src/lib.rs`: after the `reverb_mix` block (line 46–47) add

```rust
#[path = "../../src-tauri/src/audio/shimmer.rs"]
pub mod shimmer;
```

and add `shimmer` to the `pub mod audio` shim's `pub use super::{…}` list (between `reverb_mix` and `spectrum`).

- [ ] **Step 3: Run the desktop tests** — `cd app/src-tauri && export PATH="$HOME/.cargo/bin:$PATH" && cargo test shimmer -- --nocapture`. Expected: all 7 shimmer tests pass. If `stop_change_is_continuous` exceeds 0.35, print the measured jump in the assert message, verify it is stable across runs, and set the bound to measured×1.5 — do not loosen silently.
- [ ] **Step 4: Run the wasm build** — `cd app/wasm-dsp && cargo test shimmer && cargo clippy --target wasm32-unknown-unknown`. Expected: same 7 pass; clippy clean (zero warnings is the project bar).
- [ ] **Step 5: Zero-warning check** — `cd app/src-tauri && cargo check`. Expected: 0 warnings.
- [ ] **Step 6: Commit** — `git add app/src-tauri/src/audio/shimmer.rs app/src-tauri/src/audio/mod.rs app/wasm-dsp/src/lib.rs && git commit -m "feat: dual delay-line pitch shifter module for the shimmer loop"`

---

### Task 2: Wire the cascade into `reverb_mix.rs`

**Files:**
- Modify: `app/src-tauri/src/audio/reverb_mix.rs`

**Interfaces:**
- Consumes: Task 1's `Shimmer` (full API above).
- Produces (used by Task 4): `Reverb::set_shift(&mut self, semitones: f32)`, `Reverb::set_tone(&mut self, t: f32)`. `set_mix(mix: f32)` keeps its signature and now also sets the internal depth; `clear()` also flushes the shimmer.

- [ ] **Step 1: Write the failing tests** — add to `reverb_mix.rs`'s `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn shimmer_disabled_matches_plain_engine() {
        // With depth 0 the loop return must be exactly zero, so the Reverb
        // path is bit-identical to composing CloudsReverb + the mix stage
        // by hand — the regression guard for the injection point.
        let sr = 44_100.0;
        let (mut rv_a, mut rv_b) = (Reverb::new(sr), Reverb::new(sr));
        rv_a.depth_g = 0.0;
        let mut manual = CloudsReverb::new(sr);
        manual.set_diffusion(0.625);
        manual.set_lp(0.7);
        manual.set_input_gain(0.2);
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((seed >> 33) as f32 / u32::MAX as f32 - 0.5) * 1.2
        };
        for i in 0..8_000 {
            let x = next();
            let mut a = [x, x];
            rv_a.process(&mut a);
            let [wet_l, wet_r] = manual.process([x + rv_b.ret_l, x + rv_b.ret_r]);
            rv_b.env_wet = follow(
                rv_b.env_wet,
                wet_l.abs().max(wet_r.abs()),
                rv_b.env_attack,
                rv_b.env_release,
            );
            let wg = rv_b.wet_gain();
            rv_b.ret_l = 0.0;
            rv_b.ret_r = 0.0;
            a[0] = mix_reverb_frame(x, wet_l, wg, rv_b.mix);
            a[1] = mix_reverb_frame(x, wet_r, wg, rv_b.mix);
            assert_eq!(a[0].to_bits(), a[1].to_bits(), "frame {i} diverged");
        }
    }

    #[test]
    fn shimmer_cascade_stays_bounded() {
        // Full mix = full depth G_MAX: the coupled reverb+shimmer loop must
        // reach a finite equilibrium, never diverge.
        for sr in [32_000.0f32, 48_000.0, 96_000.0] {
            let mut rv = Reverb::new(sr);
            rv.set_mix(1.0);
            rv.set_shift(12.0);
            rv.set_tone(1.0);
            let mut peak = 0.0f32;
            let mut seed = 0x1234_5678u32;
            for i in 0..(sr * 10.0) as usize {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let x = ((seed >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
                let mut frame = [x, x];
                rv.process(&mut frame);
                assert!(
                    frame[0].is_finite() && frame[1].is_finite(),
                    "{sr} Hz: non-finite at frame {i}"
                );
                peak = peak.max(frame[0].abs()).max(frame[1].abs());
            }
            assert!(peak < 5.0, "{sr} Hz: cascade diverged to {peak}");
        }
    }

    #[test]
    fn nan_loop_self_heals() {
        let mut rv = Reverb::default();
        rv.set_mix(0.8);
        rv.shimmer.poison_for_test();
        for _ in 0..1_000 {
            let mut frame = [0.25f32, 0.25];
            rv.process(&mut frame);
            assert!(frame[0].is_finite() && frame[1].is_finite());
        }
    }
```

- [ ] **Step 2: Run to verify failure** — `cd app/src-tauri && export PATH="$HOME/.cargo/bin:$PATH" && cargo test reverb_mix -- --nocapture`. Expected: compile error (`depth_g`/`shimmer`/`set_shift`/`set_tone` not found).
- [ ] **Step 3: Implement** — in `reverb_mix.rs`:

Add imports and constants:

```rust
use crate::audio::shimmer::Shimmer;

/// Cascade depth at full mix. Worst-case recirculation is
/// `REVERB_TIME (0.55) × shifter (≤1 by the window invariant) × tone (≤1) × g`,
/// so 1.0 keeps the coupled loop below unity with margin — same cliff
/// discipline as `REVERB_TIME` in `clouds_reverb.rs`.
const G_MAX: f32 = 1.0;
/// Depth curve exponent: g = G_MAX · mix^1.5, gentle near zero, blooming late.
const DEPTH_EXPONENT: f32 = 1.5;
/// Cross-tap: the shimmer taps mostly its own channel's tail, a little of
/// the other, for width.
const CROSS_OWN: f32 = 0.7;
const CROSS_OTHER: f32 = 0.3;
```

Extend the struct and constructor:

```rust
pub struct Reverb {
    inner: CloudsReverb,
    mix: f32,
    /// Pitch-cascade loop: shifter + tone + DC blocker (see `shimmer.rs`).
    shimmer: Shimmer,
    /// Loop gain, recomputed from `mix` in `set_mix`.
    depth_g: f32,
    /// The loop return fed into the reverb input on the next frame.
    ret_l: f32,
    ret_r: f32,
    env_dry: f32,
    env_wet: f32,
    env_attack: f32,
    env_release: f32,
}
```

In `new()`, add fields `shimmer: Shimmer::new(sample_rate)`, `depth_g: 0.0`, `ret_l: 0.0`, `ret_r: 0.0`.

`set_mix` also sets the depth:

```rust
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
        self.depth_g = G_MAX * self.mix.powf(DEPTH_EXPONENT);
    }
```

New param setters and extended `clear`:

```rust
    /// Shift interval in semitones (fader stop values).
    pub fn set_shift(&mut self, semitones: f32) {
        self.shimmer.set_shift(semitones);
    }

    /// Loop damping, 0..1.
    pub fn set_tone(&mut self, t: f32) {
        self.shimmer.set_tone(t);
    }
```

`clear()` gains `self.shimmer.clear(); self.ret_l = 0.0; self.ret_r = 0.0;`.

Rewrite `process_with_gain`:

```rust
    #[inline]
    pub fn process_with_gain(&mut self, frame: &mut [f32; 2], wg: f32) {
        let dry_l = frame[0];
        let dry_r = frame[1];
        self.env_dry = follow(self.env_dry, (dry_l + dry_r) * 0.5, self.env_attack, self.env_release);

        let [wet_l, wet_r] = self.inner.process([dry_l + self.ret_l, dry_r + self.ret_r]);
        // Follow the louder tail channel: the two loops are symmetric, so the
        // peak is the pair's shared level and neither channel gets pulled down.
        self.env_wet = follow(self.env_wet, wet_l.abs().max(wet_r.abs()), self.env_attack, self.env_release);

        // Shimmer loop: cross-tap the tail, pitch it, damp it, feed it back
        // into the reverb input on the next frame. The envelope normalizer
        // rides the cascade's finite equilibrium — with the loop-gain cap the
        // tail settles, so no special-casing is needed here.
        let tap_l = CROSS_OWN * wet_l + CROSS_OTHER * wet_r;
        let tap_r = CROSS_OWN * wet_r + CROSS_OTHER * wet_l;
        let (s_l, s_r) = self.shimmer.process(tap_l, tap_r);
        if s_l.is_finite() && s_r.is_finite() {
            self.ret_l = s_l * self.depth_g;
            self.ret_r = s_r * self.depth_g;
        } else {
            // A diverged loop reaches the soft clip as inf/inf = NaN, which
            // would poison every delay line permanently. Flush everything;
            // the dry input refills the loop from silence.
            self.inner.clear();
            self.shimmer.clear();
            self.ret_l = 0.0;
            self.ret_r = 0.0;
        }

        frame[0] = mix_reverb_frame(dry_l, wet_l, wg, self.mix);
        frame[1] = mix_reverb_frame(dry_r, wet_r, wg, self.mix);
    }
```

Also update the module doc comment: the wrapper now owns the shimmer cascade in addition to dry/wet and loudness.

- [ ] **Step 4: Run tests** — same command as Step 2. Expected: all reverb_mix tests pass, including the pre-existing `reverb_mix_loudness_constant` (its ±4 dB bounds hold because the normalizer rides the cascade; if a bound fails, print measured dB and re-check with the spec's listening-test note — the bound may need ±5 dB, justify in the report, never silently).
- [ ] **Step 5: Full gates** — `cargo test` (all), `cargo check` (0 warnings) in src-tauri; `cargo test` + `cargo clippy --target wasm32-unknown-unknown` in wasm-dsp. Expected: green everywhere; the web engine now runs the cascade at defaults (shift +12, tone 0.65) — params arrive in Task 4.
- [ ] **Step 6: Commit** — `git add app/src-tauri/src/audio/reverb_mix.rs && git commit -m "feat: shimmer cascade loop in the reverb wrapper"`

---

### Task 3: EQ 10 → 8 bands across the stack

**Files:**
- Modify: `app/src-tauri/src/audio/eq.rs` (band table, state arrays, signatures)
- Modify: `app/src-tauri/src/audio/player.rs` (`set_eq`, `set_params` signature)
- Modify: `app/src-tauri/src/commands.rs` (`set_params` IPC, length check)
- Modify: `app/wasm-dsp/src/processor.rs` (`Params.eq: [f32; 8]`)
- Modify: `app/wasm-dsp/src/bindings.rs` (`set_params` band array)
- Modify: `app/src/main.ts` (params init, pushParams, both reset loops)
- Modify: `app/src/web/player.ts` (params init)
- Test: update affected tests in `eq.rs`, `processor.rs`

**Interfaces:**
- Produces: `EQ_FREQS: [f32; 8]` = `[310, 600, 1000, 3000, 6000, 12000, 14000, 16000]`; `EqState::new(sr, &[f32; 8])`; `player::set_params(cutoff, pitch_st, reverb, eq: [f32; 8], speed)`; `Params.eq: [f32; 8]`. The `AudioParamsInput.eq` TS type stays `number[]` (length enforced by senders).

- [ ] **Step 1: eq.rs** — replace the band table and array types:

```rust
pub const EQ_FREQS: [f32; 8] = [
    310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];
```

Change `z1/z2/z1r/z2r` to `[f32; 8]`, `Default` impl `&[0.0; 8]`, `new`/`set_gains` take `gains_db: &[f32; 8]`, and drop the `*freq < 100.0` Q arm (no band below 310 Hz):

```rust
            let q = if *freq > 10_000.0 { 0.9 } else { 1.0 };
```

Update eq.rs tests that index 10-band arrays (band 4/1000 Hz becomes band 2).

- [ ] **Step 2: player.rs** — `set_eq(eq: [f32; 8])`, `set_params(cutoff: f32, pitch_st: f32, reverb: f32, eq: [f32; 8], speed: f32)`, internal `arr = [0.0f32; 8]` if any. `commands.rs`: `if eq.len() != 8 { return Err("expected 8 EQ gains".into()); }`, `let mut arr = [0.0f32; 8];`.
- [ ] **Step 3: wasm-dsp** — `processor.rs`: `pub eq: [f32; 8]`, `Default: eq: [0.0; 8]`; `bindings.rs`: `let mut bands = [0.0f32; 8];` (the `eq.get(i)` loop already pads short vectors). Update `processor.rs` test `eq_boost_changes_level_in_bypass`: `params.eq[4] = -12.0` → `params.eq[2] = -12.0` (1000 Hz is band 2 now).
- [ ] **Step 4: frontend lengths** — `main.ts`: line ~358 `eq: new Array(10).fill(0)` → `new Array(8).fill(0)`; `pushParams` line ~401 `new Array(10).fill(0)` → `new Array(8)`; both reset loops (`i < 10` at lines ~493 and ~503) → `i < 8`. `web/player.ts` line ~58 same init change. `transport.ts`: update the `eq` doc comment to "Eight peaking bands in dB (310 Hz..16 kHz)".
- [ ] **Step 5: Gates** — src-tauri `cargo test` + `cargo check` (0 warnings); wasm-dsp `cargo test` + clippy; `cd app && npm test && npm run build`. All green.
- [ ] **Step 6: Commit** — `git add -u app/src-tauri/src/audio/eq.rs app/src-tauri/src/audio/player.rs app/src-tauri/src/commands.rs app/wasm-dsp/src/processor.rs app/wasm-dsp/src/bindings.rs app/src/main.ts app/src/web/player.ts app/src/transport.ts && git commit -m "feat: eight-band EQ - the two bass faders become shimmer controls"`

---

### Task 4: Shift/tone plumbing end-to-end + stepped fader

**Files:**
- Modify: `app/src/transport.ts` (`AudioParamsInput` + `shift`, `tone`)
- Modify: `app/src/main.ts` (params, setParam/valueOf/pushParams, fx_reset, stops arg at fader call sites)
- Modify: `app/src/web/player.ts` (params init + worklet message)
- Modify: `app/src/worklet/dspWorklet.js` (forward `msg.shift`, `msg.tone`)
- Modify: `app/wasm-dsp/src/bindings.rs` + `app/wasm-dsp/src/processor.rs` (Params fields)
- Modify: `app/src-tauri/src/audio/player.rs` (SharedPlay shift/tone + callback apply)
- Modify: `app/src-tauri/src/commands.rs` (IPC signature)
- Modify: `app/src/transportTauri.ts` (invoke args)
- Modify: `app/public/sprite/skin.json` (fader defs)
- Modify: `app/src/sprite/types.ts` (`FaderDef.stops`)
- Modify: `app/src/sprite/layout.ts` (stops quantization)
- Test: `app/src/sprite/layout.test.ts` (stops cases)

**Interfaces:**
- Consumes: Task 2's `Reverb::set_shift`/`set_tone`; Task 3's 8-band plumbing.
- Produces: `AudioParamsInput { …, shift: number, tone: number }` (shift = semitones, one of `[-12, 7, 12, 19, 24]` from the UI; tone = 0..1). Desktop `player::set_params(cutoff, pitch_st, reverb, eq, speed, shift, tone)`; `Params { …, shift: f32, tone: f32 }` with defaults 12.0 / 0.65.

- [ ] **Step 1: Rust params** — `processor.rs`:

```rust
pub struct Params {
    pub cutoff: f32,
    pub speed: f32,
    pub pitch_semitones: f32,
    pub reverb: f32,
    pub eq: [f32; 8],
    /// Shimmer shift interval in semitones (UI sends stop values).
    pub shift: f32,
    /// Shimmer loop damping, 0..1.
    pub tone: f32,
}
```

Default: `shift: 12.0, tone: 0.65`. In `set_params`, after the lpf line:

```rust
        self.reverb.set_shift(self.params.shift.clamp(-12.0, 24.0));
        self.reverb.set_tone(self.params.tone.clamp(0.0, 1.0));
```

In `load_track`, after `self.reverb = Reverb::new(self.sample_rate);` re-apply the live params so the web doesn't silently revert to defaults per track:

```rust
        self.reverb.set_shift(self.params.shift.clamp(-12.0, 24.0));
        self.reverb.set_tone(self.params.tone.clamp(0.0, 1.0));
```

`bindings.rs` `set_params` gains `shift: f32, tone: f32` params and forwards them into `Params`.

- [ ] **Step 2: Desktop plumbing** — `player.rs`: `SharedPlay` gains `shift: Mutex<f32>` (init 12.0) and `tone: Mutex<f32>` (init 0.65) next to `reverb_mix`; `set_params(cutoff, pitch_st, reverb, eq, speed, shift, tone)` stores `*shared().shift.lock() = shift.clamp(-12.0, 24.0);` and `*shared().tone.lock() = tone.clamp(0.0, 1.0);`. In the callback (the block reading `reverb_mix` around line 671), read them once per buffer with the mix and apply inside the `mix > 0.001` branch, right after the guard is taken:

```rust
            let shift = *shared.shift.lock();
            let tone = *shared.tone.lock();
```

and inside the branch, after taking `reverb_guard`:

```rust
                reverb_guard.as_deref_mut().inspect(|r| {
                    r.set_shift(shift);
                    r.set_tone(tone);
                });
```

(match the guard's actual type — `MutexGuard<Option<Reverb>>` deref pattern per the surrounding code; keep the existing style). `commands.rs` `set_params` gains `shift: f64, tone: f64` and forwards `shift as f32, tone as f32`. `transportTauri.ts` `setParams` invoke payload gains `shift: params.shift, tone: params.tone`.

- [ ] **Step 3: Worklet + web plumbing** — `dspWorklet.js` `case "params"` gains `msg.shift, msg.tone` args. `web/player.ts`: params init gains `shift: 12, tone: 0.65`; the `type: "params"` message gains `shift: this.params.shift, tone: this.params.tone`.
- [ ] **Step 4: Frontend surface** — `transport.ts`:

```typescript
export type AudioParamsInput = {
  /** Master lowpass cutoff in Hz; 20000 = fully open (bit-transparent). */
  cutoff: number;
  speed: number;
  pitch: number;
  reverb: number;
  eq: number[];
  /** Shimmer shift interval in semitones (stepped fader stops). */
  shift: number;
  /** Shimmer loop damping, 0..1. */
  tone: number;
};
```

`main.ts`: params init gains `shift: 12, tone: 0.65`; `setParam` gains `else if (key === "shift") params.shift = value;` and `else if (key === "tone") params.tone = value;`; `valueOf` likewise; `pushParams` payload gains both fields; the `fx_reset` case adds `params.shift = 12; params.tone = 0.65;`.

- [ ] **Step 5: Stepped fader** — `types.ts` `FaderDef` gains:

```typescript
  /** Quantize the fader to these stops (evenly spaced in travel). */
  stops?: number[];
```

`layout.ts`: thread `stops?: number[]` through `faderValueToY`, `faderYToValue`, `faderStepValue`:

```typescript
function stopsToNorm(stops: number[] | undefined, range: [number, number], value: number): number | null {
  if (!stops || stops.length < 2) return null;
  let best = 0;
  let bestDist = Infinity;
  for (let i = 0; i < stops.length; i++) {
    const d = Math.abs(stops[i] - value);
    if (d < bestDist) { bestDist = d; best = i; }
  }
  return best / (stops.length - 1);
}

function normToStop(stops: number[] | undefined, n: number, range: [number, number], curve?: "log"): number | null {
  if (!stops || stops.length < 2) return null;
  return stops[Math.min(stops.length - 1, Math.max(0, Math.round(n * (stops.length - 1))))];
}
```

In `faderValueToY`: if `stopsToNorm` returns a number, use it as `n` directly. In `faderYToValue`: compute `n` as today, then `normToStop` overrides the return. In `faderStepValue`: if stops present, move one stop per tick — `idx = nearest index to value (clamped)`, `next = clamp(idx + (up ? 1 : -1), 0, len-1)`, return `stops[next]`. `range`/`curve` args stay (used when no stops). In `main.ts`, pass `f.stops` as the new last argument at every `faderValueToY` / `faderYToValue` / `faderStepValue` call site (grep for the three names).

- [ ] **Step 6: skin.json** — fader `eq1` becomes

```json
    {
      "id": "shift",
      "param": "shift",
      "orientation": "vertical",
      "origin": { "x": 664, "y": 845 },
      "travel": 252,
      "knob": "ui/knob_eq_1.png",
      "knobSize": { "w": 49, "h": 33 },
      "knobHotspot": "top-left",
      "range": [-12, 24],
      "value": 12,
      "stops": [-12, 7, 12, 19, 24]
    },
```

fader `eq2` becomes `{"id": "tone", "param": "tone", …same origin/travel/knob…, "range": [0, 1], "value": 0.65}` (no stops). Faders `eq3`..`eq10`: change `"param"` from `"eq2"`..`"eq9"` to `"eq0"`..`"eq7"` (ids stay). Knob sprites unchanged.

- [ ] **Step 7: layout tests** — add to `layout.test.ts`:

```typescript
// Stops quantization (shimmer shift fader).
const stops = [-12, 7, 12, 19, 24];
assert_EQ(faderYToValue({ x: 0, y: 0 }, 100, [-12, 24], 0, undefined, stops), 24, "top = highest stop");
assert_EQ(faderYToValue({ x: 0, y: 0 }, 100, [-12, 24], 100, undefined, stops), -12, "bottom = lowest stop");
assert_EQ(faderYToValue({ x: 0, y: 0 }, 100, [-12, 24], 75, undefined, stops), 7, "mid travel snaps to a stop");
assert_EQ(faderStepValue([-12, 24], 12, 0.1, true, undefined, stops), 19, "wheel steps one stop up");
assert_EQ(faderStepValue([-12, 24], 12, 0.1, false, undefined, stops), 7, "wheel steps one stop down");
assert_EQ(faderValueToY({ x: 0, y: 0 }, 100, [-12, 24], 19, undefined, stops), 25, "stop value maps to its travel slot");
```

(adapt to the file's existing assert helper/import style — read its top before writing). Also one no-stops regression assert (a plain linear fader behaves as before).

- [ ] **Step 8: Gates** — src-tauri `cargo test` + `cargo check`; wasm-dsp `cargo test` + clippy; `npm test && npm run build`. All green.
- [ ] **Step 9: Commit** — `git add app/src/transport.ts app/src/main.ts app/src/web/player.ts app/src/worklet/dspWorklet.js app/src/transportTauri.ts app/wasm-dsp/src/bindings.rs app/wasm-dsp/src/processor.rs app/src-tauri/src/audio/player.rs app/src-tauri/src/commands.rs app/public/sprite/skin.json app/src/sprite/types.ts app/src/sprite/layout.ts app/src/sprite/layout.test.ts && git commit -m "feat: shimmer shift and tone faders with stepped stops, 8-band params"`

---

### Task 5: Documentation

**Files:**
- Modify: `docs/DSP.md` (reverb chapter → shimmer chain)
- Modify: `AGENTS.md` (§3.1 invariants 12–13, §6 shared-module list, diagram reverb line)
- Modify: `README.md`, `README.en.md`, `DEVELOPMENT.md` (effect description wherever the reverb is described)

- [ ] **Step 1: DSP.md** — rewrite the reverb section: the chain is now dry → Griesinger core → wet, plus the cascade loop (tap → cross-tap → dual delay-line shifter → tone LP → DC block → g(mix) → reverb input); document the window invariant, `G_MAX` cap reasoning, NaN self-heal, and the envelope-normalizer interaction. Reference `shimmer.rs`.
- [ ] **Step 2: AGENTS.md** — add to §3.1:
  - `12. **Complementary Shifter Windows** (`shimmer.rs`): the two read heads' gains must sum to 1 at every position (`head_gain(p) + head_gain(p + len/2) == 1`). Unity-gain heads with a short fade region sum to 2 and snap back periodically — a ~12 Hz thump. The window-sum test guards this.
  - `13. **Shimmer Loop-Gain Cap & NaN Self-Heal** (`reverb_mix.rs`): worst-case recirculation is `REVERB_TIME × shifter × tone × g` and must stay < 1 with margin (`G_MAX = 1.0` vs 0.55). A diverged shimmer loop hits the soft clip as `inf/inf = NaN`, which poisons every delay line permanently — the per-frame finite check flushes reverb + shifter and restarts from dry. Never map the amount fader onto raw loop gain > 1.`
  - §6: "Ten modules" → "Eleven modules", add `shimmer` to the list and its sentence; the DSP diagram's reverb line becomes the shimmer chain.
- [ ] **Step 3: README/DEVELOPMENT** — update the effects description wherever the reverb is named: the reverb is now a shimmer reverb (Eno/Lanois-style octave cascade), EQ is 8 bands, the two freed faders are shift/tone. Match each file's existing voice and language (README.md Russian, README.en.md English).
- [ ] **Step 4: Commit** — `git add docs/DSP.md AGENTS.md README.md README.en.md DEVELOPMENT.md && git commit -m "docs: shimmer reverb chain, eight-band EQ, new invariants 12-13"`

---

### Task 6: Final whole-branch review, then browser verification

- Final code review: dispatch the code-reviewer subagent per `superpowers:requesting-code-review` with a review package from `git merge-base` (the commit before Task 1) to HEAD, plus the Minor-findings ledger.
- Browser verification (controller-executed, after review): `cd app && npm run build:pages`, serve `dist/`, open a **fresh** browser tab, play a bundled track, engage reverb + shift/tone, confirm audible cascade, finite spectrum tap, and a throttled-CPU smoke run. Leave the artifact running for the user. The JS↔WASM boundary has passed cargo while emitting garbage before — this check is not optional.

## Out of scope

Version bump + changelog (tag time); cascaded second shifter; reverse modes; size knob; param persistence.
