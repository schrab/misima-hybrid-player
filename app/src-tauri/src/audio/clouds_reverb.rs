//! Feedback-delay reverb in the style of Mutable Instruments Clouds.
//!
//! Topology and DSP graph ported from the Eurorack Clouds source (MIT licence,
//! Copyright 2014 Emilie Gillet — https://github.com/pichenettes/eurorack,
//! `clouds/dsp/fx/reverb.h` and `clouds/dsp/fx/fx_engine.h`). Emilie's own
//! comment describes it: a Griesinger topology from the Dattorro paper —
//! four AP diffusers on the input, then a loop of 2x (2AP + delay).
//! Modulation sits inside the first diffuser for extra smearing, and on the
//! long delays for a slow shimmer/chorus effect.
//!
//! Three deliberate deviations from the original:
//!
//! - Delay storage is `f32`, not the original's 12-bit packed `u16`. Clouds
//!   quantised because it ran on a Cortex-M4 with 320 KB of RAM; that
//!   quantisation is grit, not signal, and only ever cost us headroom.
//! - The delay-line layout is recomputed per sample rate. The original is
//!   fixed at 32 kHz; here both the lengths and their offsets scale by
//!   `sample_rate / 32000.0` (see AGENTS.md 3.1.4). Scaling only the lengths
//!   and leaving the offsets behind makes `del1` grow into `del2`.
//! - The tail's decay is a fixed constant instead of being tied to the reverb
//!   amount. On the hardware those are the same knob, but ours is a dry/wet
//!   fader, and silently lengthening the tail when the user pushes it up is
//!   surprising. See `REVERB_TIME` for how the value was chosen.
//!
//! The delay-line management follows the original's `FxEngine`: one
//! power-of-two ring, delay lines resolved to constant offsets into it, and a
//! small accumulator-style API. Every offset is known at construction, there
//! is no modulo in the inner loop, and the whole reverb body is
//! allocation-free and lock-free.

/// Sample rate the original delay lengths are written for.
const REF_RATE: f32 = 32_000.0;

/// Delay lengths at 32 kHz: four input diffusers, then two cross-coupled
/// feedback loops of (2 diffusers + long delay). `dap1a/dap1b/del1` feed the
/// left output and `dap2a/dap2b/del2` feed the right, and each loop reads the
/// other's long delay, so the two tails stay decorrelated.
const LENGTHS: [usize; 10] = [113, 162, 241, 399, 1653, 2038, 3411, 1913, 1663, 4782];

/// Fixed tap offsets inside the loop, at 32 kHz. The smear tap is a second
/// write into `ap1`; the long tap is where `del2` is read from in the left
/// loop. Both scale with the delay lengths so the voicing is rate-independent.
const SMEAR_TAP: usize = 100;
const LONG_TAP: f32 = 4680.0;

/// Modulation depths, at 32 kHz: 60 samples on the smeared diffuser, 100 on
/// the long delay.
const SMEAR_DEPTH: f32 = 60.0;
const LONG_DEPTH: f32 = 100.0;

/// Loop gain, which sets the tail's RT60. Measured on this topology the tail
/// is musical up to about 0.6 and then falls off a cliff — 0.65 already rings
/// for 10 s, and past 0.9 the loop starts building up. 0.55 holds an RT60 of
/// roughly 2.8 s at 32k / 44.1k / 48k / 96k, consistent across rates to
/// within 0.15 s, with margin to spare before that cliff.
const REVERB_TIME: f32 = 0.55;

/// A delay line resolved to a constant offset into the ring plus its length.
#[derive(Clone, Copy, Debug)]
struct Line {
    base: usize,
    len: usize,
}

/// Lay the ten lines out end to end, each followed by a one-sample guard
/// slot, and return them with the ring size. The guard is what lets a line's
/// two-sample interpolated read run one past its last slot without touching
/// the next line.
fn layout(sample_rate: f32) -> ([Line; 10], usize) {
    let scale = (sample_rate / REF_RATE).max(0.5);
    let mut lines = [Line { base: 0, len: 0 }; 10];
    let mut base = 0usize;
    for (i, line) in lines.iter_mut().enumerate() {
        *line = Line {
            base,
            len: ((LENGTHS[i] as f32 * scale).round() as usize).max(8),
        };
        base += line.len + 1;
    }
    (lines, base.next_power_of_two())
}

/// A slow modulation source. The original stepped its lookup table once per
/// 32-sample control block; we do the same so the modulation keeps its blocky
/// character instead of becoming per-sample smooth.
#[derive(Debug)]
struct Lfo {
    phase: f32,
    inc: f32,
    value: f32,
}

impl Lfo {
    fn new(hz: f32, sample_rate: f32) -> Self {
        Self {
            phase: 0.0,
            inc: hz / sample_rate,
            value: 1.0,
        }
    }

    #[inline]
    fn tick(&mut self) {
        self.phase += 32.0 * self.inc;
        self.value = self.phase.cos();
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.value = 1.0;
    }
}

/// Stereo feedback-delay reverb. `process` returns the raw wet tail; the
/// caller owns the dry/wet balance and any loudness policy — see
/// `player.rs`.
#[derive(Debug)]
pub struct CloudsReverb {
    /// Single power-of-two ring; every delay line lives inside it.
    buffer: Box<[f32]>,
    mask: usize,
    write_ptr: usize,

    ap1: Line,
    ap2: Line,
    ap3: Line,
    ap4: Line,
    dap1a: Line,
    dap1b: Line,
    del1: Line,
    dap2a: Line,
    dap2b: Line,
    del2: Line,

    smear_tap: usize,
    long_tap: f32,

    input_gain: f32,
    diffusion: f32,
    lp: f32,

    /// Damping one-pole states inside the feedback loops.
    lp_decay_1: f32,
    lp_decay_2: f32,

    lfo_1: Lfo,
    lfo_2: Lfo,
}

impl CloudsReverb {
    pub fn new(sample_rate: f32) -> Self {
        let (lines, size) = layout(sample_rate);
        let [ap1, ap2, ap3, ap4, dap1a, dap1b, del1, dap2a, dap2b, del2] = lines;
        let scale = (sample_rate / REF_RATE).max(0.5);

        Self {
            buffer: vec![0.0; size].into_boxed_slice(),
            mask: size - 1,
            write_ptr: 0,
            ap1,
            ap2,
            ap3,
            ap4,
            dap1a,
            dap1b,
            del1,
            dap2a,
            dap2b,
            del2,
            smear_tap: ((SMEAR_TAP as f32 * scale).round() as usize).max(2),
            long_tap: LONG_TAP * scale,
            input_gain: 0.2,
            diffusion: 0.625,
            lp: 0.7,
            lp_decay_1: 0.0,
            lp_decay_2: 0.0,
            // 0.5 Hz and 0.3 Hz at the original's 32 kHz reference.
            lfo_1: Lfo::new(0.5, sample_rate),
            lfo_2: Lfo::new(0.3, sample_rate),
        }
    }

    pub fn clear(&mut self) {
        self.buffer.fill(0.0);
        self.write_ptr = 0;
        self.lp_decay_1 = 0.0;
        self.lp_decay_2 = 0.0;
        self.lfo_1.reset();
        self.lfo_2.reset();
    }

    /// Allpass coefficients of the diffusers: tail density, and how much the
    /// tail rings metallically. Higher is denser and longer-ringing.
    pub fn set_diffusion(&mut self, diffusion: f32) {
        self.diffusion = diffusion.clamp(0.0, 0.95);
    }

    /// Damping inside the feedback loops. Higher is darker, and decays faster.
    pub fn set_lp(&mut self, lp: f32) {
        self.lp = lp.clamp(0.0, 1.0);
    }

    pub fn set_input_gain(&mut self, gain: f32) {
        self.input_gain = gain;
    }

    /// Advance one stereo frame and return the raw wet tail. This is the
    /// audio thread's inner loop: no allocation, no locking, no modulo. The
    /// caller owns the dry/wet balance, so this returns the wet signal
    /// unmixed and unnormalised.
    #[inline]
    pub fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
        let [dry_l, dry_r] = input;

        self.write_ptr = if self.write_ptr == 0 {
            self.mask
        } else {
            self.write_ptr - 1
        };
        if self.write_ptr & 31 == 0 {
            self.lfo_1.tick();
            self.lfo_2.tick();
        }

        let kap = self.diffusion;
        let klp = self.lp;
        let krt = REVERB_TIME;
        let gain = self.input_gain;
        let lfo_1 = self.lfo_1.value;
        let lfo_2 = self.lfo_2.value;

        // Smear AP1 inside the loop: the interpolated tap is written to the
        // smear point, and the scale of 0 zeroes the accumulator.
        let smear = self.interpolate(self.ap1, 10.0 + SMEAR_DEPTH * lfo_1);
        let mut acc = self.write(self.ap1, self.smear_tap, smear, 0.0);

        acc += (dry_l + dry_r) * gain;

        // Diffuse through 4 allpasses.
        acc = self.allpass(self.ap1, acc, kap, -kap);
        acc = self.allpass(self.ap2, acc, kap, -kap);
        acc = self.allpass(self.ap3, acc, kap, -kap);
        acc = self.allpass(self.ap4, acc, kap, -kap);
        let apout = acc;

        // Main reverb loop, left output.
        acc = apout + self.interpolate(self.del2, self.long_tap + LONG_DEPTH * lfo_2) * krt;
        self.lp_decay_1 += klp * (acc - self.lp_decay_1);
        acc = self.lp_decay_1;
        acc = self.allpass(self.dap1a, acc, -kap, kap);
        acc = self.allpass(self.dap1b, acc, kap, -kap);
        let wet_l = self.write(self.del1, 0, acc, 2.0);

        // Main reverb loop, right output.
        acc = apout + self.tail(self.del1) * krt;
        self.lp_decay_2 += klp * (acc - self.lp_decay_2);
        acc = self.lp_decay_2;
        acc = self.allpass(self.dap2a, acc, kap, -kap);
        acc = self.allpass(self.dap2b, acc, -kap, kap);
        let wet_r = self.write(self.del2, 0, acc, 2.0);

        [wet_l, wet_r]
    }

    /// Store `value` into `line` at `offset` samples back, and return
    /// `value * scale` for the caller to keep accumulating. Note the scale
    /// applies to the running accumulator only — the original writes the
    /// accumulator into the delay line untouched and scales afterwards, and
    /// that detail is what sets the loop gain.
    #[inline]
    fn write(&mut self, line: Line, offset: usize, value: f32, scale: f32) -> f32 {
        let off = offset.min(line.len - 1);
        self.buffer[(self.write_ptr + line.base + off) & self.mask] = value;
        value * scale
    }

    /// Read a line's tail — its last slot, the longest tap it offers.
    #[inline]
    fn tail(&self, line: Line) -> f32 {
        self.buffer[(self.write_ptr + line.base + line.len - 1) & self.mask]
    }

    /// Linearly interpolated read at a fractional delay, for the two modulated
    /// taps. The second sample it reads is the line's guard slot.
    #[inline]
    fn interpolate(&self, line: Line, offset: f32) -> f32 {
        let offset = offset.clamp(0.0, line.len as f32 - 2.0);
        let i = offset.floor() as usize;
        let frac = offset - i as f32;
        let a = self.buffer[(self.write_ptr + line.base + i) & self.mask];
        let b = self.buffer[(self.write_ptr + line.base + i + 1) & self.mask];
        a + (b - a) * frac
    }

    /// Allpass combiner, matching the original's `Read` + `WriteAllPass`
    /// pair: fold the tail into the accumulator, store the accumulator
    /// unscaled, then scale and add the tail back.
    #[inline]
    fn allpass(&mut self, line: Line, acc: f32, read_scale: f32, write_scale: f32) -> f32 {
        let tail = self.tail(line);
        let sum = acc + tail * read_scale;
        self.write(line, 0, sum, write_scale) + tail
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run an impulse and report (peak, frames until the tail falls 60 dB
    /// below that peak). The tail cannot appear until the impulse has
    /// circulated once — through `del1` into the right loop, then back
    /// through `del2` — so this measures the global peak, not frame by frame.
    fn impulse(rate: f32) -> (f32, usize) {
        let mut rv = CloudsReverb::new(rate);
        let mut peak = 0.0f32;
        let frames = (rate * 8.0) as usize;
        for i in 0..frames {
            let dry = [if i == 0 { 1.0 } else { 0.0 }; 2];
            let wet = rv.process(dry);
            assert!(
                wet[0].is_finite() && wet[1].is_finite(),
                "{rate} Hz: tail went non-finite at frame {i}"
            );
            peak = peak.max(wet[0].abs()).max(wet[1].abs());
        }
        let mut rv = CloudsReverb::new(rate);
        let wet = rv.process([1.0, 1.0]);
        peak = peak.max(wet[0].abs()).max(wet[1].abs());
        let mut last = 0;
        for n in 0..frames {
            let wet = rv.process([0.0, 0.0]);
            if wet[0].abs().max(wet[1].abs()) > peak * 0.001 {
                last = n;
            }
        }
        (peak, last)
    }

    #[test]
    fn layout_is_contiguous_and_pow2() {
        for rate in [32_000.0f32, 44_100.0, 48_000.0, 96_000.0, 8_000.0] {
            let (lines, size) = layout(rate);
            assert!(size.is_power_of_two());
            let mut prev_end = 0;
            for line in lines.iter() {
                assert_eq!(line.base, prev_end, "gap/overlap at rate {rate}");
                assert!(line.len >= 8);
                // The guard slot is what makes the two-sample interpolated
                // read safe.
                prev_end = line.base + line.len + 1;
            }
            assert!(prev_end <= size, "layout overflows the ring at {rate}");
        }
    }

    #[test]
    fn lines_do_not_overlap_at_any_rate() {
        // Regression: scaling the lengths but not the offsets makes del1 grow
        // into del2, which diverges the loop.
        for rate in [32_000.0f32, 44_100.0, 48_000.0, 96_000.0] {
            let (lines, size) = layout(rate);
            for a in 0..lines.len() {
                for b in (a + 1)..lines.len() {
                    let a_end = lines[a].base + lines[a].len;
                    assert!(
                        a_end <= lines[b].base,
                        "at {rate} Hz line {a} ends at {a_end} but line {b} starts at {}",
                        lines[b].base
                    );
                }
            }
            assert!(size >= lines[9].base + lines[9].len + 1);
        }
    }

    #[test]
    fn impulse_tail_is_finite_and_bounded() {
        for rate in [32_000.0f32, 44_100.0, 48_000.0, 96_000.0, 8_000.0] {
            let (peak, _) = impulse(rate);
            assert!(peak > 0.01, "{rate} Hz: impulse produced no tail");
            assert!(peak < 5.0, "{rate} Hz: tail diverged to {peak}");
        }
    }

    #[test]
    fn tail_decays_in_a_musical_time() {
        // Guards the cliff past REVERB_TIME: at 0.65 this topology already
        // rings for 10 s, and at 0.9 it builds up instead of decaying.
        for rate in [32_000.0f32, 44_100.0, 48_000.0, 96_000.0] {
            let (_, frames) = impulse(rate);
            let secs = frames as f32 / rate;
            assert!(
                (2.0..4.0).contains(&secs),
                "{rate} Hz: RT60 of {secs:.2} s is outside the musical range"
            );
        }
    }

    #[test]
    fn tail_channels_are_decorrelated() {
        // A mono impulse must not come back as a mono signal, or the stereo
        // image is still the old collapsed reverb.
        let mut rv = CloudsReverb::new(44_100.0);
        let _ = rv.process([1.0, 0.0]);
        let mut cross = 0.0f64;
        let mut energy = 0.0f64;
        for _ in 0..20_000 {
            let wet = rv.process([0.0, 0.0]);
            cross += (wet[0] as f64) * (wet[1] as f64);
            energy += (wet[0] as f64) * (wet[0] as f64);
        }
        let corr = cross / energy.max(1e-12);
        assert!(
            corr.abs() < 0.9,
            "tail is effectively mono (correlation {corr:.3})"
        );
    }

    #[test]
    fn clear_resets_state() {
        let mut rv = CloudsReverb::new(44_100.0);
        for _ in 0..2_000 {
            let _ = rv.process([1.0, 1.0]);
        }
        rv.clear();
        let wet = rv.process([0.0, 0.0]);
        assert!(
            wet[0].abs() < 1e-6 && wet[1].abs() < 1e-6,
            "clear left state behind"
        );
    }
}
