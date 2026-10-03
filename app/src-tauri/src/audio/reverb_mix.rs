//! Dry/wet balance and loudness policy in front of the `CloudsReverb` FDN.
//!
//! Platform-free: the types here touch no cpal, tauri, or parking_lot API, so
//! both `src-tauri` and the web `wasm-dsp` crate can `#[path]`-include them and
//! hear the same reverb.

use crate::audio::clouds_reverb::CloudsReverb;

/// Stereo feedback-delay reverb, with the loudness policy that sits in front
/// of it. The DSP graph itself is `CloudsReverb` (Mutable Instruments
/// Clouds, MIT, Copyright 2014 Emilie Gillet — ported in
/// `audio/clouds_reverb.rs`); this wrapper owns the dry/wet balance and the
/// level envelopes that gain-normalise the tail.
///
/// The envelopes exist because the raw tail level is extremely
/// material-dependent: 0.46x dry RMS on a steady tone, 4.5x on broadband, so
/// no fixed wet gain stays balanced.
#[derive(Debug)]
pub struct Reverb {
    inner: CloudsReverb,
    mix: f32,
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
            env_dry: 0.0,
            env_wet: 0.0,
            // Level follower coefficients: fast attack, ~0.3 s release at any
            // sample rate.  The release time constant is -1 / (sr * ln(coeff)),
            // so coeff = exp(-1 / (sr * tau)).
            env_attack: 0.7,
            env_release: (-1.0 / (sample_rate * 0.3)).exp(),
        }
    }

    /// Wet/dry balance, 0..1, from the reverb fader.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
    }

    /// Drop the tail — used on seek and track load so the previous track's
    /// reverb does not bleed into the new one.
    pub fn clear(&mut self) {
        self.inner.clear();
        self.env_wet = 0.0;
        self.env_dry = 0.0;
    }

    #[inline]
    pub fn process_with_gain(&mut self, frame: &mut [f32; 2], wg: f32) {
        let dry_l = frame[0];
        let dry_r = frame[1];
        self.env_dry = follow(self.env_dry, (dry_l + dry_r) * 0.5, self.env_attack, self.env_release);

        let [wet_l, wet_r] = self.inner.process([dry_l, dry_r]);
        // Follow the louder tail channel: the two loops are symmetric, so the
        // peak is the pair's shared level and neither channel gets pulled down.
        self.env_wet = follow(self.env_wet, wet_l.abs().max(wet_r.abs()), self.env_attack, self.env_release);

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
}
