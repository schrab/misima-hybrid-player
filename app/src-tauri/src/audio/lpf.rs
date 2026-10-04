//! 4-pole (24 dB/oct) master lowpass — the cutoff fader's engine.
//!
//! Two cascaded RBJ lowpass biquads with a mild resonance bump. The fader's
//! top position maps to `OPEN_CUTOFF`, where the filter becomes identity and
//! clears its registers, so "fully open" is bit-transparent rather than
//! "20 kHz lowpass" (which would still shave the top octave). The bottom sits
//! at 30 Hz: almost closed, but the sub-bass floor stays alive.

use crate::audio::eq::Biquad;

/// Cutoff at or above this means fully open: identity coefficients, no state.
pub const OPEN_CUTOFF: f32 = 20_000.0;
/// Fader floor. Not fully closed — a whisper of sub-bass stays audible.
pub const CLOSED_CUTOFF: f32 = 30.0;

/// A flat 4th-order Butterworth would be -3 dB at the cutoff; multiplying the
/// pole Qs by this raises a ~+4 dB peak there instead. Kept far from
/// self-oscillation — RBJ biquads are stable for any finite Q.
const RESONANCE: f32 = 1.4;
/// Butterworth pole Qs for a 4th-order lowpass, resonated.
const Q1: f32 = 0.541_196 * RESONANCE;
const Q2: f32 = 1.306_563 * RESONANCE;

/// Master-bus lowpass: two RBJ biquad stages, stereo.
///
/// State layout follows `EqState`: coefficients live in the stage biquads and
/// the delay registers in parallel arrays outside them, so `set_cutoff` can
/// swap coefficients in place without wiping filter memory — that is what
/// keeps a fader sweep free of clicks (AGENTS.md 3.1.3).
#[derive(Debug, Clone)]
pub struct Lowpass4 {
    stages: [Biquad; 2],
    /// [stage][channel] z1/z2 for the transposed direct form II tick.
    z1: [[f32; 2]; 2],
    z2: [[f32; 2]; 2],
    sr: f32,
    cutoff: f32,
}

impl Lowpass4 {
    pub fn new(sample_rate: f32) -> Self {
        let mut this = Self {
            stages: [Biquad::identity(), Biquad::identity()],
            z1: [[0.0; 2]; 2],
            z2: [[0.0; 2]; 2],
            sr: 0.0,
            cutoff: OPEN_CUTOFF,
        };
        this.set_cutoff(sample_rate, OPEN_CUTOFF);
        this
    }

    /// True while fully open — `process_frame` is a no-op.
    pub fn is_open(&self) -> bool {
        self.cutoff >= OPEN_CUTOFF
    }

    /// Apply a new cutoff. The no-op when nothing changed matters: the
    /// desktop callback calls this every buffer and `set_params` fires on
    /// every pointermove of *any* fader drag.
    pub fn set_cutoff(&mut self, sample_rate: f32, hz: f32) {
        let sr = if sample_rate > 0.0 { sample_rate } else { 44_100.0 };
        // `clamp` passes NaN through, and NaN coefficients would silence the
        // output — treat a non-finite value (an undefined worklet message
        // field, say) as fully open instead of poisoning the chain.
        let hz = if hz.is_finite() {
            hz.clamp(CLOSED_CUTOFF, OPEN_CUTOFF)
        } else {
            OPEN_CUTOFF
        };
        if (hz - self.cutoff).abs() < 0.01 && (sr - self.sr).abs() < 0.01 {
            return;
        }
        self.sr = sr;
        self.cutoff = hz;
        if hz >= OPEN_CUTOFF {
            // Fully open: identity coefficients and empty registers, so the
            // top of the fader is bit-transparent. Entering bypass clears
            // once; leaving it starts from silence, not stale ringing.
            self.stages = [Biquad::identity(), Biquad::identity()];
            self.clear();
        } else {
            // Coefficients update in place; the registers keep their memory.
            self.stages[0] = Biquad::lowpass(sr, hz, Q1);
            self.stages[1] = Biquad::lowpass(sr, hz, Q2);
        }
    }

    /// Flush the filter memory (seek / track change).
    pub fn clear(&mut self) {
        self.z1 = [[0.0; 2]; 2];
        self.z2 = [[0.0; 2]; 2];
    }

    #[inline(always)]
    pub fn process_frame(&mut self, frame: &mut [f32; 2]) {
        if self.is_open() {
            return;
        }
        for (c, sample) in frame.iter_mut().enumerate() {
            let mid = self.stages[0].tick(*sample, &mut self.z1[0][c], &mut self.z2[0][c]);
            *sample = self.stages[1].tick(mid, &mut self.z1[1][c], &mut self.z2[1][c]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    fn sine(freq: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
            .collect()
    }

    /// LCG noise, deterministic, ±0.6.
    fn noise(n: usize) -> Vec<f32> {
        let mut seed = 0x1234_5678_9abc_def1u64;
        (0..n)
            .map(|_| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                ((seed >> 33) as f32 / (u32::MAX >> 1) as f32 - 1.0) * 0.6
            })
            .collect()
    }

    /// Run interleaved stereo through the filter, skipping the settle-in.
    fn filtered(lpf: &mut Lowpass4, mono: &[f32], skip: usize) -> Vec<f32> {
        let mut out = Vec::with_capacity(mono.len() - skip);
        for (i, s) in mono.iter().enumerate() {
            let mut frame = [*s, *s];
            lpf.process_frame(&mut frame);
            if i >= skip {
                out.push(frame[0]);
            }
        }
        out
    }

    fn db(delta_rms: f32, reference: f32) -> f32 {
        20.0 * (delta_rms / reference).log10()
    }

    #[test]
    fn open_is_bit_transparent() {
        let mut lpf = Lowpass4::new(SR);
        let sig = noise(512);
        for (i, s) in sig.iter().enumerate() {
            let mut frame = [*s, *s];
            lpf.process_frame(&mut frame);
            assert_eq!(frame[0], sig[i]);
            assert_eq!(frame[1], sig[i]);
        }
    }

    #[test]
    fn closing_then_reopening_is_transparent_again() {
        let mut lpf = Lowpass4::new(SR);
        lpf.set_cutoff(SR, 500.0);
        for s in noise(4096) {
            let mut frame = [s, s];
            lpf.process_frame(&mut frame);
        }
        lpf.set_cutoff(SR, OPEN_CUTOFF);
        for s in noise(512) {
            let mut frame = [s, s];
            lpf.process_frame(&mut frame);
            assert_eq!(frame[0], s);
            assert_eq!(frame[1], s);
        }
    }

    #[test]
    fn two_octaves_above_cutoff_lands_near_48db() {
        let mut lpf = Lowpass4::new(SR);
        lpf.set_cutoff(SR, 1_000.0);
        let out = filtered(&mut lpf, &sine(4_000.0, SR as usize), SR as usize / 4);
        let drop = db(crate::audio::eq::rms(&out), 0.5f32.sqrt());
        assert!(
            (-58.0..=-40.0).contains(&drop),
            "attenuation at 2 octaves: {drop} dB"
        );
    }

    #[test]
    fn passband_stays_put() {
        let mut lpf = Lowpass4::new(SR);
        lpf.set_cutoff(SR, 1_000.0);
        let out = filtered(&mut lpf, &sine(200.0, SR as usize), SR as usize / 4);
        let drop = db(crate::audio::eq::rms(&out), 0.5f32.sqrt());
        assert!(drop > -2.0, "passband droop: {drop} dB");
    }

    #[test]
    fn resonance_bump_at_the_cutoff() {
        let mut lpf = Lowpass4::new(SR);
        lpf.set_cutoff(SR, 1_000.0);
        let out = filtered(&mut lpf, &sine(1_000.0, SR as usize), SR as usize / 4);
        let gain = db(crate::audio::eq::rms(&out), 0.5f32.sqrt());
        // Butterworth would sit at -3 dB; the resonated pair peaks near +3 dB.
        assert!(gain > -0.5, "no resonance bump: {gain} dB at cutoff");
    }

    #[test]
    fn almost_closed_at_the_floor() {
        let mut lpf = Lowpass4::new(SR);
        lpf.set_cutoff(SR, CLOSED_CUTOFF);
        let out = filtered(&mut lpf, &sine(1_000.0, SR as usize), SR as usize / 4);
        let drop = db(crate::audio::eq::rms(&out), 0.5f32.sqrt());
        assert!(drop < -60.0, "floor not closed: {drop} dB");
    }

    #[test]
    fn sweep_stays_finite_and_bounded() {
        let mut lpf = Lowpass4::new(SR);
        let sig = noise(SR as usize);
        // Sweep up through the whole range in small steps and back down,
        // filtering while the coefficients move — the fader-drag stress case.
        let steps = 400;
        for (step, chunk) in sig.chunks(sig.len() / steps).enumerate() {
            let t = 1.0 - (step as f32 / steps as f32 - 1.0).abs();
            let hz = CLOSED_CUTOFF * (OPEN_CUTOFF / CLOSED_CUTOFF).powf(t);
            lpf.set_cutoff(SR, hz);
            for &s in chunk {
                let mut frame = [s, s];
                lpf.process_frame(&mut frame);
                assert!(frame[0].is_finite() && frame[0].abs() < 8.0);
                assert!(frame[1].is_finite() && frame[1].abs() < 8.0);
            }
        }
    }

    #[test]
    fn clear_matches_a_fresh_filter() {
        let sig = noise(8_192);
        let mut used = Lowpass4::new(SR);
        used.set_cutoff(SR, 500.0);
        for &s in &sig {
            let mut frame = [s, s];
            used.process_frame(&mut frame);
        }
        used.clear();

        let mut fresh = Lowpass4::new(SR);
        fresh.set_cutoff(SR, 500.0);
        for &s in &sig {
            let mut a = [s, s];
            used.process_frame(&mut a);
            let mut b = [s, s];
            fresh.process_frame(&mut b);
            assert_eq!(a, b);
        }
    }
}
