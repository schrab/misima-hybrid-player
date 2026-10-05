//! Dry/wet balance, loudness policy and the shimmer cascade in front of the
//! `CloudsReverb` FDN.
//!
//! Platform-free: the types here touch no cpal, tauri, or parking_lot API, so
//! both `src-tauri` and the web `wasm-dsp` crate can `#[path]`-include them and
//! hear the same reverb.

use crate::audio::clouds_reverb::CloudsReverb;
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

/// Stereo feedback-delay reverb, with the loudness policy and the shimmer
/// cascade that sit in front of it. The DSP graph itself is `CloudsReverb`
/// (Mutable Instruments Clouds, MIT, Copyright 2014 Emilie Gillet — ported
/// in `audio/clouds_reverb.rs`) plus a `Shimmer` pitch shifter; this wrapper
/// owns the dry/wet balance, the level envelopes that gain-normalise the
/// tail, and the feedback path that feeds the shifted tail back into the
/// reverb input.
///
/// The envelopes exist because the raw tail level is extremely
/// material-dependent: 0.46x dry RMS on a steady tone, 4.5x on broadband, so
/// no fixed wet gain stays balanced. The cascade rides on top of that
/// normaliser: because the loop gain is capped below unity the coupled
/// reverb+shimmer loop settles at a finite equilibrium, so the envelope
/// follows the shimmered tail just as it follows the dry one.
#[derive(Debug)]
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

impl Default for Reverb {
    fn default() -> Self {
        Self::new(44_100.0)
    }
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let mut inner = CloudsReverb::new(sample_rate);
        // The voice, in one place. Diffusion is tail density (higher is
        // denser and rings longer), `lp` is damping in the feedback loops
        // (higher is darker and decays faster), and input gain feeds the
        // diffuser bank. These are the original's defaults.
        inner.set_diffusion(0.625);
        inner.set_lp(0.7);
        inner.set_input_gain(0.2);
        Self {
            inner,
            mix: 0.0,
            shimmer: Shimmer::new(sample_rate),
            depth_g: 0.0,
            ret_l: 0.0,
            ret_r: 0.0,
            env_dry: 0.0,
            env_wet: 0.0,
            // Level follower coefficients: fast attack, ~0.3 s release at any
            // sample rate.  The release time constant is -1 / (sr * ln(coeff)),
            // so coeff = exp(-1 / (sr * tau)).
            env_attack: 0.7,
            env_release: (-1.0 / (sample_rate * 0.3)).exp(),
        }
    }

    /// Wet/dry balance, 0..1, from the reverb fader. Also the cascade's depth
    /// control: the loop return rides the same fader, on a curve that stays out
    /// of the way at low settings and blooms towards the top.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
        self.depth_g = G_MAX * self.mix.powf(DEPTH_EXPONENT);
    }

    /// Shift interval in semitones (fader stop values).
    #[allow(dead_code)] // Task 4 wires set_shift/set_tone to the UI plumbing
    pub fn set_shift(&mut self, semitones: f32) {
        self.shimmer.set_shift(semitones);
    }

    /// Loop damping, 0..1.
    #[allow(dead_code)] // Task 4 wires set_shift/set_tone to the UI plumbing
    pub fn set_tone(&mut self, t: f32) {
        self.shimmer.set_tone(t);
    }

    /// Drop the tail — used on seek and track load so the previous track's
    /// reverb does not bleed into the new one. The cascade loop is flushed
    /// with it, otherwise a shifter ring of the old track keeps recirculating
    /// through the fresh reverb.
    pub fn clear(&mut self) {
        self.inner.clear();
        self.shimmer.clear();
        self.ret_l = 0.0;
        self.ret_r = 0.0;
        self.env_wet = 0.0;
        self.env_dry = 0.0;
    }

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

    /// Run one stereo frame through the reverb and the dry/wet balance.
    #[inline]
    #[allow(dead_code)]
    pub fn process(&mut self, frame: &mut [f32; 2]) {
        self.process_with_gain(frame, self.wet_gain());
    }

    /// Wet gain that holds the tail near dry loudness so 100% mix is a full
    /// wet reverb, not a quiet one. FLOOR avoids divide-by-zero on silence;
    /// clamp bounds pump on transients.
    ///
    /// TARGET sits above 1 because `mix_reverb_frame` soft-clips the tail by
    /// `w / sqrt(1 + w^2)`, which costs about 3 dB at w = 1 — so the envelope
    /// has to aim higher to land on the dry level. Measured on broadband
    /// material the wet comes out +0.1 dB at half mix, +1.3 dB at full.
    pub fn wet_gain(&self) -> f32 {
        const TARGET: f32 = 1.8;
        const FLOOR: f32 = 1.0e-4;
        (self.env_dry * TARGET / self.env_wet.max(FLOOR)).clamp(0.05, 8.0)
    }
}

#[inline]
fn follow(env: f32, x: f32, attack: f32, release: f32) -> f32 {
    let ax = x.abs();
    if ax > env {
        env + (ax - env) * attack
    } else {
        env * release + ax * (1.0 - release)
    }
}

/// Reverb mix for one channel: linear dry→wet crossfade of an
/// envelope-normalized wet tail. At 100% the output is pure wet at roughly
/// dry loudness — the full effect — while the adaptive gain keeps the sweep
/// from the old ~16 dB loudness dip. `wg` is computed once per callback
/// buffer; the clip is unity-slope near zero.
#[inline]
fn mix_reverb_frame(dry: f32, wet_raw: f32, wg: f32, mix: f32) -> f32 {
    let w = wet_raw * wg;
    let wet = w / (1.0 + w * w).sqrt();
    dry * (1.0 - mix) + wet * mix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverb_dry_ish_stability() {
        let mut rv = Reverb::default();
        let mut peak = 0.0f32;
        for i in 0..8_000 {
            let mut frame = [if i < 50 { 1.0 } else { 0.0 }; 2];
            rv.process(&mut frame);
            peak = peak.max(frame[0].abs()).max(frame[1].abs());
        }
        assert!(peak.is_finite() && peak < 5.0);
    }

    #[test]
    fn reverb_mix_loudness_constant() {
        // 100% mix must be a full wet reverb at roughly dry loudness (not
        // the old ~16 dB drop), and mid settings must stay in the same
        // ballpark. The wet tail is gain-normalized to the dry envelope
        // (raw tail varies ~20 dB between tonal and broadband material).
        // LCG noise keeps the signal repeatable.
        const N: usize = 44_100;
        const WARM: usize = 8_820;
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut noise = Vec::with_capacity(N);
        for _ in 0..N {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            noise.push((((seed >> 33) as f32 / (u32::MAX as f32)) - 0.5) * 1.2);
        }
        let sine: Vec<f32> = (0..N)
            .map(|i| (((i as f32) * std::f32::consts::TAU * 440.0 / 44_100.0).sin()) * 0.5)
            .collect();
        for (label, sig) in [("noise", &noise), ("sine", &sine)] {
            for mix in [0.0f32, 0.5, 1.0] {
                let mut rv = Reverb::default();
                rv.set_mix(mix);
                let (mut acc_in, mut acc_out) = (0.0f64, 0.0f64);
                for (i, &x) in sig.iter().enumerate() {
                    let mut frame = [x, x];
                    rv.process(&mut frame);
                    if i > WARM {
                        // Output is summed over both channels, so the dry
                        // reference has to be as well — otherwise mix=0 reads
                        // +3 dB purely from the channel count.
                        acc_in += 2.0 * (x * x) as f64;
                        acc_out += (frame[0] * frame[0]) as f64 + (frame[1] * frame[1]) as f64;
                    }
                }
                let cnt = ((N - WARM) * 2) as f64;
                let ratio = (acc_out / cnt).sqrt() / (acc_in / cnt).sqrt();
                let db = 20.0 * ratio.log10() as f32;
                println!("{label} mix={mix} loudness vs dry: {db:+.2} dB");
                // Broadband (music-like) material must land within ±2 dB of
                // dry, so pushing the fader changes the character and not the
                // level. A pure tone can dip a few dB at mid-mix — phase
                // cancellation against its own coherent tail.
                assert!(db > -4.0 && db < 4.0, "{label} mix={mix} drift {db:+.2} dB");
            }
        }
    }

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
}
