//! Dual delay-line pitch shifter for the shimmer reverb loop.
//!
//! Two read heads half a ring apart, one ring per channel, traverse it at
//! `ratio` times the write head's speed — +12 semitones laps them twice per
//! write lap. They carry complementary raised-cosine gains, `a1 + a2 = 1` at
//! every position, which is what keeps the crossfade free of the periodic
//! 6 dB thump a short fade region produces. Reads are cubic Hermite; the ring
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
// `Debug` exists so `Reverb`'s derive survives: this is a field of it.
#[derive(Debug)]
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
            // Default stop is +12 semitones, i.e. an octave up: ratio 2.
            target_ratio: 2.0,
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
        // ratio stays within [0.5, 4] and len >= 1024, so one wrap suffices.
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

    /// Sine driver for tests whose per-sample delta must be small enough that a
    /// bound on it means something (see `stop_change_is_continuous`).
    fn sine_frames(n: usize, freq: f32, amp: f32, sr: f32) -> Vec<f32> {
        (0..n)
            .map(|i| (TAU * freq * i as f32 / sr).sin() * amp)
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
        // Driven by a 220 Hz sine rather than broadband noise: noise gives a
        // per-sample delta floor of ~0.6, which buries any click under the
        // floor; the sine's floor is ~2π·220/48000·0.5 ≈ 0.014, so the
        // absolute bound below is a real bound. The bound checks *waveform*
        // continuity — a click — not the glide itself: a direct ratio
        // assignment would pass it too, by design.
        let sr = 48_000.0f32;
        let mut sh = Shimmer::new(sr);
        sh.set_shift(12.0);
        sh.set_tone(1.0);
        let frames = sine_frames(26_400, 220.0, 0.5, sr);
        // `prev` carries the warmup's last output across `set_shift`, so the
        // delta that straddles the call itself is inside the measurement — a
        // discontinuous `set_shift` would land in that one pair.
        let mut prev = 0.0f32;
        for &x in &frames[..24_000] {
            prev = sh.process(x, x).0;
        }
        sh.set_shift(-12.0);
        let mut max_jump = 0.0f32;
        let mut peak = 0.0f32;
        for &x in &frames[24_000..26_400] {
            let (l, _) = sh.process(x, x);
            max_jump = max_jump.max((l - prev).abs());
            peak = peak.max(l.abs());
            prev = l;
        }
        assert!(max_jump < 0.1, "stop change clicked: max jump {max_jump}");
        // Without this the test passes just as happily on a dead shifter, so
        // it only means something while the output is actually alive.
        assert!(peak > 0.1, "window is silent: peak {peak}");
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

    /// The read-position wrap itself: the shortest ring the engine can build
    /// (MIN_RING floor, via an 8 kHz device) against the fastest ratio it can
    /// be set to. At ratio 4.006 the read pointer advances ~4 samples a frame
    /// over a 1024-sample ring, so it crosses the wrap ~390 times in the run
    /// below rather than never.
    #[test]
    fn read_position_wrap_stays_finite_at_the_extremes() {
        let mut sh = Shimmer::new(8_000.0);
        sh.set_shift(24.0); // ratio 4 — four write laps per read lap
        sh.set_tone(1.0);
        let frames = noise_frames(100_000, 0.5);
        for (i, f) in frames.iter().enumerate() {
            let (l, r) = sh.process(f[0], f[1]);
            assert!(
                l.is_finite() && r.is_finite(),
                "wrap non-finite at frame {i}"
            );
        }
    }

    /// `clear` is the seek/track-load flush: the rings and every filter
    /// register go, so a silent frame into a cleared shifter is silent out.
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
        assert!(
            l.abs() < 1e-6 && r.abs() < 1e-6,
            "clear left residue: {l} / {r}"
        );
    }
}
