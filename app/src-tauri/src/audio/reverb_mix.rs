//! Dry/wet balance and the shimmer cascade in front of the `CloudsReverb`
//! FDN.
//!
//! Platform-free: the types here touch no cpal, tauri, or parking_lot API, so
//! both `src-tauri` and the web `wasm-dsp` crate can `#[path]`-include them and
//! hear the same reverb.

use crate::audio::clouds_reverb::CloudsReverb;
use crate::audio::shimmer::Shimmer;

/// The cascade loop's own feedback gain. The shifter sits in ITS OWN loop —
/// tap + `CASCADE_FB`·previous cascade → shifter → cascade — completely
/// outside the reverb's `krt` recirculation, so the loop gain is
/// `CASCADE_FB × shifter (≤1 by the crossfade) ≤ 0.5`: unconditionally
/// stable, no build-and-dump at any level. This mirrors the working Faust
/// reference (`shimmer.dsp`), where the transposer's loop feedback tops out
/// at 0.35 and the shimmer's loudness never touches loop stability.
const CASCADE_FB: f32 = 0.5;
/// Cascade output level at full mix. Pure output gain — it scales what the
/// listener hears, never what the loop recirculates, so it can be raised
/// freely if the shimmer reads shy.
const CASCADE_LEVEL: f32 = 1.0;
/// Level curve exponent: level = CASCADE_LEVEL · mix^1.5, gentle near zero,
/// blooming late.
const LEVEL_EXPONENT: f32 = 1.5;
/// Cross-tap: the shimmer taps mostly its own channel's tail, a little of
/// the other, for width.
const CROSS_OWN: f32 = 0.7;
const CROSS_OTHER: f32 = 0.3;
/// Fixed wet gain ahead of the mix crossfade. The raw tail's level varies
/// with the material — a steady tone rings quieter than dense broadband —
/// and a fixed gain accepts that variance the way every reference reverb
/// does (Valhalla, zita, the Faust patches: none ride a gain detector).
/// The previous adaptive normalizer (`env_dry·TARGET/env_wet`) was exactly a
/// sidechain compressor — fast-attack detector on the tail, inverted gain —
/// and ducked the wet after every transient, which under the shimmer's
/// steady cascade the user heard as pumping. Removed by user decision
/// (2026-10-06).
const WET_GAIN: f32 = 1.0;

/// Stereo feedback-delay reverb with the shimmer cascade. The DSP graph
/// itself is `CloudsReverb` (Mutable Instruments Clouds, MIT, Copyright 2014
/// Emilie Gillet — ported in `audio/clouds_reverb.rs`) plus a `Shimmer` pitch
/// shifter; this wrapper owns the dry/wet balance and the cascade's own
/// fixed-gain loop whose output sums directly into the wet — the working
/// Faust reference's topology, where shimmer loudness is an output level and
/// never part of loop gain.
#[derive(Debug)]
pub struct Reverb {
    inner: CloudsReverb,
    mix: f32,
    /// Pitch-cascade loop: shifter + tone + DC blocker (see `shimmer.rs`).
    shimmer: Shimmer,
    /// Cascade output level, recomputed from `mix` in `set_mix`.
    casc_level: f32,
    /// The cascade's previous output — the loop feedback signal.
    casc_l: f32,
    casc_r: f32,
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
            casc_level: 0.0,
            casc_l: 0.0,
            casc_r: 0.0,
        }
    }

    /// Wet/dry balance, 0..1, from the reverb fader. Also the cascade's
    /// loudness: its loop gain is fixed at `CASCADE_FB`, so the fader only
    /// scales the output level — stable at any setting.
    pub fn set_mix(&mut self, mix: f32) {
        self.mix = mix.clamp(0.0, 1.0);
        self.casc_level = CASCADE_LEVEL * self.mix.powf(LEVEL_EXPONENT);
    }

    /// Shift interval in semitones (fader stop values).
    pub fn set_shift(&mut self, semitones: f32) {
        self.shimmer.set_shift(semitones);
    }

    /// Loop damping, 0..1 (dark..bright).
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
        self.casc_l = 0.0;
        self.casc_r = 0.0;
    }

    /// Run one stereo frame through the reverb, the cascade and the dry/wet
    /// balance.
    #[inline]
    pub fn process_with_gain(&mut self, frame: &mut [f32; 2]) {
        // Sanitize the *source* here. The cascade guard below costs
        // `shimmer.clear()` — the whole ring — whenever the shifter returns a
        // non-finite sample. If the input itself is persistently non-finite
        // (a malformed decode), that branch fires every single frame at
        // ring-clearing cost on the RT thread. Mute the bad frame up front
        // instead — one compare per channel — so a broken decode costs
        // silence rather than a meltdown, and the cascade guard keeps doing
        // its real job (catching a *diverged cascade*, rare and transient).
        let dry_l = if frame[0].is_finite() { frame[0] } else { 0.0 };
        let dry_r = if frame[1].is_finite() { frame[1] } else { 0.0 };

        let [wet_l, wet_r] = self.inner.process([dry_l, dry_r]);

        // The cascade runs in its own loop — cross-tapped tail plus
        // `CASCADE_FB` times the previous cascade output, through the
        // shifter — completely outside the reverb's `krt` recirculation. Its
        // output sums directly into the wet, so shimmer loudness
        // (`casc_level`) is pure output gain and can never push the loop
        // past unity: the working Faust reference's topology.
        let tap_l = CROSS_OWN * wet_l + CROSS_OTHER * wet_r;
        let tap_r = CROSS_OWN * wet_r + CROSS_OTHER * wet_l;
        let (s_l, s_r) = self.shimmer.process(
            tap_l + CASCADE_FB * self.casc_l,
            tap_r + CASCADE_FB * self.casc_r,
        );
        if s_l.is_finite() && s_r.is_finite() {
            self.casc_l = s_l;
            self.casc_r = s_r;
        } else {
            // A diverged cascade reaches the output soft clip as
            // `inf/inf = NaN`. Flush the shifter and the loop state; the
            // tail refills the loop from silence. The reverb itself is
            // untouched — it is no longer part of this loop.
            self.shimmer.clear();
            self.casc_l = 0.0;
            self.casc_r = 0.0;
        }

        // The cascade sums after the tail's soft clip; the callers' final
        // output clamp is the last line of defence for the sum.
        frame[0] = mix_reverb_frame(dry_l, wet_l, self.mix) + self.casc_l * self.casc_level;
        frame[1] = mix_reverb_frame(dry_r, wet_r, self.mix) + self.casc_r * self.casc_level;
    }

    /// Convenience wrapper for tests and offline rendering.
    #[inline]
    #[allow(dead_code)]
    pub fn process(&mut self, frame: &mut [f32; 2]) {
        self.process_with_gain(frame);
    }
}

/// Reverb mix for one channel: linear dry→wet crossfade of the tail, with
/// the fixed [`WET_GAIN`] in front and a unity-slope soft clip as the tail's
/// limiter — it saturates gracefully when dense broadband material pushes
/// the tail past full scale at high mix.
#[inline]
fn mix_reverb_frame(dry: f32, wet_raw: f32, mix: f32) -> f32 {
    let w = wet_raw * WET_GAIN;
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

    /// The wet/dry sweep's loudness report. With the adaptive normalizer
    /// gone there is no level constancy to assert — a fixed gain accepts the
    /// material variance (a steady tone rings quieter than dense broadband,
    /// by design of every reference reverb). What must still hold: finite
    /// output everywhere and no runaway with the mix wide open. The printed
    /// dB table documents the actual spread for tuning [`WET_GAIN`].
    #[test]
    fn reverb_mix_loudness_report() {
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
                let mut peak = 0.0f32;
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
                    peak = peak.max(frame[0].abs()).max(frame[1].abs());
                    assert!(frame[0].is_finite() && frame[1].is_finite());
                }
                let cnt = ((N - WARM) * 2) as f64;
                let ratio = (acc_out / cnt).sqrt() / (acc_in / cnt).sqrt();
                let db = 20.0 * ratio.log10() as f32;
                println!("{label} mix={mix} loudness vs dry: {db:+.2} dB (peak {peak:.3})");
                assert!(db > -30.0 && db < 30.0, "{label} mix={mix} runaway {db:+.2} dB");
            }
        }
    }

    #[test]
    fn shimmer_disabled_matches_plain_engine() {
        // Cascade level 0 must make the Reverb path bit-identical to
        // composing CloudsReverb + the mix stage by hand.
        //
        // `mix = 1.0` is load-bearing: at the default 0 the wet path is
        // multiplied by zero and both sides collapse to the dry sample, so the
        // assert would pass no matter what the reverb did. At full wet the
        // equality spans the reverb and the mix stage for real.
        //
        // What it cannot cover is the cascade itself: at level 0 its output
        // sum is identically zero, so the sum's placement (after the soft
        // clip) is invisible here. `shimmer_cascade_sums_into_the_wet` below
        // is the test for that.
        let sr = 44_100.0;
        let (mut rv_a, mut rv_b) = (Reverb::new(sr), Reverb::new(sr));
        rv_a.set_mix(1.0);
        rv_a.casc_level = 0.0;
        rv_b.mix = 1.0;
        rv_b.casc_level = 0.0;
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
            let [wet_l, wet_r] = manual.process([x, x]);
            // Compare the engine's channel against the replica's *same*
            // channel. `CloudsReverb` is a stereo FDN whose per-channel line
            // lengths differ, so L and R are legitimately not sample-equal —
            // a left-vs-right assert says nothing about the wiring, and
            // folding the replica into `a` before comparing throws away
            // `rv_a`'s output entirely.
            let b0 = mix_reverb_frame(x, wet_l, rv_b.mix);
            let b1 = mix_reverb_frame(x, wet_r, rv_b.mix);
            assert_eq!(a[0].to_bits(), b0.to_bits(), "frame {i} left diverged");
            assert_eq!(a[1].to_bits(), b1.to_bits(), "frame {i} right diverged");
        }
    }

    /// The cascade must actually reach the output. Two engines see identical
    /// input and differ only in `casc_level`; their outputs must separate
    /// once the shifter's rings stop being empty. A cascade that is wired
    /// but never summed (or summed after the mix stage, where `mix = 0`
    /// would mute it twice) fails this.
    #[test]
    fn shimmer_cascade_sums_into_the_wet() {
        let sr = 44_100.0;
        let (mut off, mut on) = (Reverb::new(sr), Reverb::new(sr));
        off.set_mix(1.0);
        on.set_mix(1.0);
        off.casc_level = 0.0;
        on.casc_level = 1.0;
        let mut seed = 0x1234_5678u32;
        let mut out_diff = usize::MAX;
        for i in 0..8_000 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let x = ((seed >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
            let mut a = [x, x];
            let mut b = [x, x];
            off.process(&mut a);
            on.process(&mut b);
            if a[0].to_bits() != b[0].to_bits() && out_diff == usize::MAX {
                out_diff = i;
            }
        }
        assert!(
            out_diff > 0 && out_diff != usize::MAX,
            "the cascade never changed the output: it is not being summed at all"
        );
        println!("cascade enters the output at frame {out_diff}");
    }

    #[test]
    fn shimmer_cascade_stays_bounded() {
        // Full mix = full cascade level. The cascade loop itself is stable by
        // construction (feedback 0.5, shifter gain ≤ 1), but the test still
        // guards the whole summed path — tail + cascade — across rates and
        // the undamped tone corner (tone 0, 500 Hz lowpass), which
        // recirculates hardest.
        for sr in [32_000.0f32, 48_000.0, 96_000.0] {
            for tone in [0.0f32, 0.65, 1.0] {
                let mut rv = Reverb::new(sr);
                rv.set_mix(1.0);
                rv.set_shift(12.0);
                rv.set_tone(tone);
                let mut peak = 0.0f32;
                let mut seed = 0x1234_5678u32;
                for i in 0..(sr * 10.0) as usize {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    let x = ((seed >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
                    let mut frame = [x, x];
                    rv.process(&mut frame);
                    assert!(
                        frame[0].is_finite() && frame[1].is_finite(),
                        "{sr} Hz tone {tone}: non-finite at frame {i}"
                    );
                    peak = peak.max(frame[0].abs()).max(frame[1].abs());
                }
                assert!(peak < 5.0, "{sr} Hz tone {tone}: cascade diverged to {peak}");
            }
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

    /// Naive DFT bin magnitude over the settled half of a signal.
    fn bin_mag(sig: &[f32], from: usize, freq: f64, sr: f64) -> f64 {
        use std::f64::consts::TAU;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (i, &s) in sig[from..].iter().enumerate() {
            let w = TAU * freq * i as f64 / sr;
            re += s as f64 * w.cos();
            im -= s as f64 * w.sin();
        }
        (re * re + im * im).sqrt()
    }

    /// The cascade must actually be audible, not merely wired and bounded: an
    /// implementation that summed it at 1/10 gain, or a mistyped level
    /// exponent, would pass every other test in this file. The reverb core
    /// alone cannot turn 440 Hz into 880 Hz — only the pitch cascade can —
    /// so the octave bin at full level must dwarf the same bin with the
    /// cascade muted. Broadband drive cannot work here: the reverb's own
    /// noise tail floods the octave bin, masking exactly the contribution
    /// this test exists to see. Divergence is the broadband tests' job above.
    #[test]
    fn shimmer_cascade_is_audible() {
        let sr = 48_000.0f64;
        let f0 = 440.0;
        let n = (sr * 2.0) as usize;
        let mut full = Reverb::new(sr as f32);
        full.set_mix(1.0);
        full.set_shift(12.0);
        full.set_tone(1.0);
        let mut muted = Reverb::new(sr as f32);
        muted.set_mix(1.0);
        muted.set_shift(12.0);
        muted.set_tone(1.0);
        muted.casc_level = 0.0;
        let (mut sig_full, mut sig_muted) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let x = (std::f64::consts::TAU * f0 * i as f64 / sr).sin() as f32 * 0.5;
            let mut a = [x, x];
            full.process(&mut a);
            sig_full.push(a[0]);
            let mut b = [x, x];
            muted.process(&mut b);
            sig_muted.push(b[0]);
        }
        let from = n / 2;
        let oct_full = bin_mag(&sig_full, from, f0 * 2.0, sr);
        let oct_muted = bin_mag(&sig_muted, from, f0 * 2.0, sr);
        // Measured at level 1.0 with the Faust-port shifter (the single-pass
        // cascade builds less octave pile-up than the old reverb-fed loop,
        // and that is the reference architecture's honest number). A level
        // scaled by 0.1 lands far below, so the 6x bound still separates
        // them with margin on both sides.
        assert!(
            oct_full > oct_muted * 6.0,
            "cascade inaudible: octave bin {oct_full:.3} at full level vs \
             {oct_muted:.3} with the cascade muted"
        );
    }
}
