//! Pitch shifter for the shimmer cascade — a faithful port of the Faust
//! `transpose` function (`ef.transpose` / the repository's
//! `examples/pitch_shifter.dsp`), the same shifter the working Valhalla-style
//! Faust shimmer patches use (`shimmer.dsp`, "Based on: ValhallaShimmer").
//!
//! A sawtooth delay ramp `d` advances by `1 − ratio` per sample and wraps at
//! the window `w` (2048 at 48 kHz). Two fractional taps read the ring at
//! delays `d` and `d + w`, crossfaded linearly: `min(d/xfade, 1)` with
//! `xfade = w/2`. The handoff at the wrap is continuous by construction —
//! tap 2's delay at `d = 0` equals tap 1's delay at `d → w` — so there is no
//! wrap click and no window-sweep tremolo beyond the intrinsic 2-tap comb,
//! which the loop's diffusion keeps shallow.
//!
//! Deliberately NOT here: any modulation of the output gain. The working
//! references shape character with pitch modulation only; a volume LFO was
//! reported live as "awful" and stays out.
//!
//! Everything is allocated once in `new`; `process` allocates nothing and
//! takes no locks. Tone updates replace the `Biquad` coefficients in place
//! and never wipe the per-channel registers (AGENTS.md 3.1.3).

use std::f64::consts::TAU;

use crate::audio::dsp_utils::cubic_hermite;
use crate::audio::eq::Biquad;

/// Ring length at [`REF_RATE`]: twice the window, so the deepest active read
/// (`d + w` with `d < xfade = w/2`) stays under `1.5·w` with margin.
const BASE_RING: f32 = 4096.0;
/// Rate the window is written for.
const REF_RATE: f32 = 48_000.0;
/// Ring floor for very low rates, so the window never gets absurdly short.
const MIN_RING: usize = 1024;
/// Shift range in semitones — the fader's stops span this.
const SHIFT_MIN: f32 = -12.0;
const SHIFT_MAX: f32 = 24.0;
/// Tone fader → loop lowpass: `500 Hz · 32^t` spans 500 Hz..16 kHz over 0..1.
const TONE_LO_HZ: f32 = 500.0;
const TONE_SPAN: f32 = 32.0;
/// DC-blocker corner: the −12 stop must not build sub-bass or DC in the loop.
const DC_HZ: f32 = 30.0;
/// Ratio slew time constant in seconds — stop changes glide, not click.
const RATIO_SLEW_S: f32 = 0.005;
/// Default tone (fader position), ≈ 4.8 kHz lowpass.
const DEFAULT_TONE: f32 = 0.65;

/// Dual-tap crossfade pitch shifter. See the module docs.
#[derive(Debug)]
pub struct Shimmer {
    sample_rate: f32,
    ring_l: Box<[f32]>,
    ring_r: Box<[f32]>,
    mask: usize,
    write: usize,
    /// The Faust sawtooth delay ramp, `[0, w)`, advancing by `1 − ratio`
    /// (slewed) per frame. One ramp drives both channels; the channels
    /// decorrelate through the cross-tapped ring contents, not the phase.
    d: f32,
    /// Current and target speed ratio `2^(semitones/12)`, slewed on changes.
    ratio: f32,
    target_ratio: f32,
    ratio_slew: f32,
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
            d: 0.0,
            ratio: 1.0,
            target_ratio: 2.0,
            ratio_slew: (-1.0 / (sample_rate * RATIO_SLEW_S)).exp(),
            tone: Biquad::lowpass(sample_rate, TONE_LO_HZ * TONE_SPAN.powf(DEFAULT_TONE), 0.707),
            tz1_l: 0.0,
            tz2_l: 0.0,
            tz1_r: 0.0,
            tz2_r: 0.0,
            dc_a: 1.0 - TAU as f32 * DC_HZ / sample_rate,
            dc_x_l: 0.0,
            dc_y_l: 0.0,
            dc_x_r: 0.0,
            dc_y_r: 0.0,
        }
    }

    /// Shift interval in semitones. The fader sends stop values; the engine
    /// clamps to the range and glides the ratio.
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
        self.d = 0.0;
        self.ratio = self.target_ratio;
        self.tz1_l = 0.0;
        self.tz2_l = 0.0;
        self.tz1_r = 0.0;
        self.tz2_r = 0.0;
        self.dc_x_l = 0.0;
        self.dc_y_l = 0.0;
        self.dc_x_r = 0.0;
        self.dc_y_r = 0.0;
    }

    /// Advance one stereo frame of the cascade: take the loop input, return
    /// the pitch-shifted, damped signal. Un-gained — the caller owns both the
    /// loop feedback and the output level.
    pub fn process(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        self.ratio += (self.target_ratio - self.ratio) * self.ratio_slew;

        let len = (self.mask + 1) as f32;
        let w = len * 0.5;
        let i = 1.0 - self.ratio;
        // Faust: d = i : (+ : +(w) : fmod(_, w)) ~ _ — the +w keeps the wrap
        // positive when the increment is negative.
        self.d = (self.d + i + w).rem_euclid(w);

        let write_f = self.write as f32;
        let shifted_l = self.crossfade_read(&self.ring_l, write_f);
        let shifted_r = self.crossfade_read(&self.ring_r, write_f);

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

    /// The Faust transpose's tap layer: two fractional reads at delays `d`
    /// and `d + w`, linearly crossfaded with `min(d/xfade, 1)`. The handoff
    /// at the wrap lands on equal delays, so it is click-free.
    fn crossfade_read(&self, ring: &[f32], write_f: f32) -> f32 {
        let len = (self.mask + 1) as f32;
        let w = len * 0.5;
        let xfade = w * 0.5;
        let p1 = (write_f - self.d).rem_euclid(len);
        let p2 = (write_f - self.d - w).rem_euclid(len);
        let blend = (self.d / xfade).min(1.0);
        self.hermite_at(ring, p1) * blend + self.hermite_at(ring, p2) * (1.0 - blend)
    }

    /// Fractional ring read, cubic Hermite, indices wrapped manually. Reads
    /// past the write point land in the oldest cycle — valid data, and the
    /// taps that reach there are silent at those ramp positions.
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
            let w = TAU * freq * i as f64 / sr;
            re += s as f64 * w.cos();
            im -= s as f64 * w.sin();
        }
        (re * re + im * im).sqrt()
    }

    /// The crossfade tap layer must pass a constant ring through exactly:
    /// both taps read the same constant, the gains sum to 1, and the wrap
    /// handoff lands on equal delays. Sweeping `d` across the whole ramp
    /// (including the wrap) catches wrap clicks, crossfade gain errors, and
    /// blend off-by-ones. This drives `crossfade_read` directly — the tone
    /// lowpass and the DC blocker sit after it and would otherwise remove
    /// the DC the test is made of.
    #[test]
    fn constant_input_passes_through_at_every_ramp_position() {
        let mut sh = Shimmer::new(48_000.0);
        sh.ring_l.fill(0.5);
        sh.ring_r.fill(0.5);
        let len = (sh.mask + 1) as f32;
        let steps = 4096;
        let mut worst = 0.0f32;
        for i in 0..steps {
            sh.d = (i as f32 + 0.5) * len / steps as f32; // every ramp position
            let out = sh.crossfade_read(&sh.ring_l, 0.0);
            worst = worst.max((out - 0.5).abs());
        }
        assert!(
            worst < 1e-3,
            "constant ring distorted by {worst} — crossfade or wrap is broken"
        );
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

    /// The crossfade sweep must not tremolo the signal. With broadband drive
    /// the two taps are decorrelated, so the worst intrinsic dip is the
    /// 50/50 correlation null (~3 dB); a pure tone would sit exactly in a
    /// comb notch and swing to silence in ANY two-tap shifter, including the
    /// reference — the loop's diffusion is what keeps real material in the
    /// shallow regime, and the envelope contract is written for it.
    #[test]
    fn crossfade_sweep_does_not_tremolo() {
        let mut sh = Shimmer::new(48_000.0);
        sh.set_shift(12.0);
        sh.set_tone(1.0);
        let frames = noise_frames(48_000 * 2, 0.5);
        let hop = 960; // 20 ms
        let mut rmses = Vec::new();
        let mut start = 48_000; // settled half
        while start + hop <= frames.len() {
            let acc: f64 = frames[start..start + hop]
                .iter()
                .map(|f| (f[0] as f64) * (f[0] as f64))
                .sum();
            rmses.push((acc / hop as f64).sqrt());
            start += hop;
        }
        let lo = rmses.iter().cloned().fold(f64::MAX, f64::min);
        let hi = rmses.iter().cloned().fold(f64::MIN, f64::max);
        let swing = hi / lo.max(1e-9);
        assert!(
            swing < 2.0,
            "crossfade tremolo: output envelope swings {swing:.2}x \
             (hi {hi:.4}, lo {lo:.4}) over the window sweep"
        );
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
            let x = (TAU * f0 * i as f64 / sr).sin() as f32 * 0.5;
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
        // A 220 Hz sine driver: its per-sample delta floor is ~0.014, four
        // orders below the bound — white noise's own jump floor (~0.6) would
        // bury the click the test exists to catch.
        let sr = 48_000.0f64;
        let f0 = 220.0;
        let frame_at = |i: usize| (std::f64::consts::TAU * f0 * i as f64 / sr).sin() as f32 * 0.5;
        let mut prev = 0.0f32;
        for i in 0..24_000 {
            prev = sh.process(frame_at(i), frame_at(i)).0;
        }
        sh.set_shift(-12.0);
        let mut max_jump = 0.0f32;
        let mut peak = 0.0f32;
        for i in 24_000..26_400 {
            let (l, _) = sh.process(frame_at(i), frame_at(i));
            max_jump = max_jump.max((l - prev).abs());
            peak = peak.max(l.abs());
            prev = l;
        }
        // The bound checks waveform continuity (a click), not the glide
        // itself — a direct ratio assignment would still pass, by design.
        assert!(max_jump < 0.1, "stop change clicked: max jump {max_jump}");
        assert!(peak > 0.1, "test window went silent — dead shifter");
    }

    #[test]
    fn low_rate_max_ratio_wrap_stays_finite() {
        // 8 kHz drives the ring to the MIN_RING floor (1024 = window 512)
        // while +24 pins the ratio at 4 — the pointer crosses the window
        // boundary every ~128 frames over the run.
        let mut sh = Shimmer::new(8_000.0);
        sh.set_shift(24.0);
        sh.set_tone(1.0);
        let frames = noise_frames(100_000, 0.5);
        for (i, f) in frames.iter().enumerate() {
            let (l, r) = sh.process(f[0], f[1]);
            assert!(l.is_finite() && r.is_finite(), "non-finite at frame {i}");
        }
    }

    #[test]
    fn clear_silences_the_ring() {
        let mut sh = Shimmer::new(48_000.0);
        sh.set_shift(12.0);
        let frames = noise_frames(2_000, 0.5);
        for f in &frames {
            let _ = sh.process(f[0], f[1]);
        }
        sh.clear();
        let (l, r) = sh.process(0.0, 0.0);
        assert!(l.abs() < 1e-6 && r.abs() < 1e-6);
    }

    #[test]
    fn stop_churn_fuzz_stays_finite() {
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
