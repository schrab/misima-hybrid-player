//! Stereo phase vocoder, for smooth pitch shifting.
//!
//! The WSOLA time-stretcher in `wsola.rs` is pitch-accurate but audibly
//! granular at large ratios: it time-compresses by `speed / pitch`, and every
//! unit of compression adds another overlapping copy of unrelated audio to
//! each output sample — grain overlap is `WIN / (HOP * stretch)`, which is 4x
//! at +1 octave. Those copies are correlated but not phase-coherent, and that
//! is what the ear hears as "granular".
//!
//! A phase vocoder has no such problem: it resynthesises each partial
//! individually at a stretched rate. A naive implementation smears transients
//! though, because neighbouring bins drift out of alignment between frames.
//! Phase locking (Laroche & Dolson, 1999) fixes that: every bin advances at
//! the rate of the spectral peak that owns it, so each partial moves as one
//! coherent object instead of dissolving.
//!
//! Two formulation details that were expensive to get wrong, recorded here so
//! they stay fixed:
//!
//! - **Anchor per frame, never accumulate.** The synthesis phase is rebuilt
//!   each frame as `previous frame's measured analysis phase + rate * synth
//!   hop`. An accumulator seeded once and advanced forever drifts: small
//!   per-frame rate errors accumulate into per-bin constant offsets, and a
//!   partial whose bins lose their relative alignment sums destructively
//!   (measured at up to -3.5 dB, dependent on where the partial sat on the
//!   bin grid). Anchoring per frame bounds any error to one frame.
//! - **Peaks need a magnitude floor.** A Hann-windowed partial sprouts
//!   sidelobe local maxima at -31.5 dB, three bins either side of its main
//!   lobe. Without a floor those sidelobes win the nearest-peak assignment for
//!   the main lobe's edge bins, splitting the partial against itself.
//!
//! ## Stereo
//!
//! No mono sum appears in the output path. Instantaneous frequencies are
//! estimated once from the **mid** spectrum — a complex average of the two
//! channels' spectra, which costs no extra transform — so both channels
//! undergo identical frequency warping and cannot drift apart in time. Peak
//! picking and phase locking then run per channel against that shared
//! estimate, so each channel keeps its own bin phases, and with them its own
//! stereo image.
//!
//! ## Structure
//!
//! STFT with a periodic Hann window. The **synthesis hop is fixed** at `n/4`,
//! where the window's squares sum to exactly 1.5, so the weighted overlap-add
//! normalisation is exact at every stretch ratio — varying the synthesis hop
//! instead made the window sum non-constant and the output level ripple by
//! up to 4x. It is the *analysis* hop that carries the ratio:
//! `analysis_hop = synth_hop * stretch`, giving a stream tempo of `stretch`,
//! which the caller's resampler then multiplies by the pitch ratio.
//!
//! Like the WSOLA this streams over a source buffer the caller owns, and
//! `process` does not allocate.

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;
use crate::audio::dsp_utils::{cubic_hermite as hermite, read_stereo_f64 as read_stereo};

/// FFT size. 2048 is ~46 ms at 44.1 kHz — long enough to resolve partials,
/// short enough to track transients.
const FFT_SIZE: usize = 2048;

/// Synthesis hop as a divisor of the FFT size (75% overlap), fixed so the
/// overlap-add normalisation is exact for every stretch ratio.
const OVERLAP: usize = 4;

/// Wrap an angle to (-pi, pi].
#[inline]
fn princarg(a: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut x = a % TAU;
    if x > PI {
        x -= TAU;
    } else if x <= -PI {
        x += TAU;
    }
    x
}

/// Rewrite one channel's spectrum with locked, stretched phases.
///
/// `synth` is the per-bin synthesis phase accumulator, `ana` scratch for this
/// frame's measured analysis phases, `seeded` reports whether `synth` has been
/// anchored yet. `peaks` is scratch. Free function so the caller's disjoint
/// field borrows stay disjoint.
#[allow(clippy::too_many_arguments)]
fn resynthesise(
    spec: &mut [Complex<f32>],
    synth: &mut [f32],
    ana: &mut [f32],
    owner: &mut [usize],
    inst_freq: &[f32],
    peaks: &mut Vec<usize>,
    seeded: &mut bool,
    n: usize,
    half: usize,
    hs: f64,
    identity: bool,
) {
    // Peaks: local maxima of the magnitude spectrum, above a floor relative to
    // the frame's strongest bin. The floor exists because a Hann-windowed
    // partial sprouts sidelobe local maxima at -31.5 dB, three bins either
    // side of its main lobe — and those steal the main lobe's edge bins during
    // nearest-peak assignment, which splits the partial and lets it cancel
    // against itself. -28 dB sits under the sidelobe level with margin while
    // keeping every partial that carries audible energy.
    peaks.clear();
    let mut frame_max = 0.0f32;
    for s in &spec[1..half] {
        let m = s.norm();
        if m > frame_max {
            frame_max = m;
        }
    }
    let floor = frame_max * 0.04;
    for k in 1..half.saturating_sub(1) {
        let m = spec[k].norm();
        if m > floor && m > spec[k - 1].norm() && m >= spec[k + 1].norm() {
            peaks.push(k);
        }
    }

    if peaks.is_empty() {
        // Silence, or a pure noise floor: fall back to advancing every bin on
        // its own, which is the plain phase vocoder.
        for (k, o) in owner.iter_mut().enumerate().take(half) {
            *o = k;
        }
    } else {
        // Nearest-peak regions of influence. A forward walk suffices: both the
        // bins and the peaks are increasing, so the nearest peak index is
        // monotonic in k.
        let mut pi = 0usize;
        // The index k is the semantic here: bin position, compared against the
        // peak table, not just a slice subscript.
        #[allow(clippy::needless_range_loop)]
        for k in 0..half {
            while pi + 1 < peaks.len()
                && (k as isize - peaks[pi + 1] as isize).abs()
                    < (k as isize - peaks[pi] as isize).abs()
            {
                pi += 1;
            }
            owner[k] = peaks[pi];
        }
    }

    if identity {
        // Test only: leave the spectrum untouched. The mirror and DC fixup
        // below are no-ops on a forward transform of real input.
        return;
    }

    // Strict Laroche & Dolson identity locking. Only peak bins accumulate a
    // phase (at their locked rate, seeded from the measured analysis phases on
    // the first frame that carries signal); every other bin is rebuilt each
    // frame as its peak's synthesis phase plus its own measured offset from
    // that peak. Rebuilding — rather than accumulating per bin — is what makes
    // this robust: per-bin accumulators diverge whenever a bin's owning peak
    // changes between frames, and a partial whose bins drift apart sums
    // destructively (measured at -5.4 dB).
    for k in 0..half {
        ana[k] = spec[k].arg();
    }
    if !*seeded && frame_max > 1e-6 {
        synth[..half].copy_from_slice(&ana[..half]);
        *seeded = true;
    } else {
        for k in 0..half {
            if owner[k] == k {
                synth[k] += inst_freq[k] * hs as f32;
            }
        }
    }
    for k in 0..half {
        let p = owner[k];
        synth[k] = synth[p] + (ana[k] - ana[p]);
    }
    for k in 0..half {
        spec[k] = Complex::from_polar(spec[k].norm(), synth[k]);
    }

    // Mirror the half spectrum into the conjugate half.
    for k in 1..half {
        spec[n - k] = spec[k].conj();
    }
    // DC and Nyquist are real bins: clear the imaginary rounding error only.
    // Assigning `norm()` to `re` would also force the sign positive, and a
    // negative analysis DC (ordinary in music) would flip by pi — a constant
    // 2*|X0| added to the whole frame. Steady-state overlap-add buries that,
    // but the first frame after a reset divides by a partial sum of squared
    // windows, which amplifies a constant by 1/(N*w[i]) where a Hann window
    // approaches zero. That is the burst every vocoder reset used to produce.
    spec[0].im = 0.0;
}

pub struct PhaseVocoder {
    n: usize,
    half: usize,
    /// Synthesis hop in samples — fixed at `n / OVERLAP`.
    hs: usize,

    window: Vec<f32>,
    fft_fwd: Arc<dyn Fft<f32>>,
    fft_inv: Arc<dyn Fft<f32>>,

    spec_l: Vec<Complex<f32>>,
    spec_r: Vec<Complex<f32>>,
    spec_mid: Vec<Complex<f32>>,

    /// Per channel: the synthesis phase accumulator, this frame's measured
    /// analysis phases, the peak that owns each bin, and whether the
    /// accumulator has been anchored yet (cleared on seek).
    synth_phase: [Vec<f32>; 2],
    ana_phase: [Vec<f32>; 2],
    owner: [Vec<usize>; 2],
    seeded: [bool; 2],

    /// Scratch for the per-frame peak list, kept allocated so `process` does
    /// not allocate on the real-time thread.
    peak_buf: Vec<usize>,

    /// This frame's instantaneous frequency per bin in rad/sample — shared by
    /// both channels, estimated from the mid spectrum.
    inst_freq: Vec<f32>,
    /// Previous mid phase, for the deviation measurement.
    prev_mid_phase: Vec<f32>,

    in_pos: f64,
    out_frac: f64,
    primed: bool,

    /// Overlap-add accumulator and window-squared normaliser, each `2 * n` so
    /// a full window of history survives the largest possible emit.
    ola: [Vec<f32>; 2],
    norm: Vec<f32>,

    fifo_l: Vec<f32>,
    fifo_r: Vec<f32>,
    fifo_read: usize,

    /// Test only: leave the spectra's phases untouched, turning the engine
    /// into a plain analysis->synthesis STFT. That must reconstruct exactly,
    /// so a failure there indicts the overlap-add plumbing rather than the
    /// phase logic.
    identity_phases: bool,
}

impl Default for PhaseVocoder {
    fn default() -> Self {
        Self::new()
    }
}

impl PhaseVocoder {
    pub fn new() -> Self {
        let n = FFT_SIZE;
        let half = n / 2 + 1;
        let hs = n / OVERLAP;
        // Periodic Hann — analysis and synthesis share it, and dividing by the
        // running sum of its square makes the overlap-add exact.
        let window: Vec<f32> = (0..n)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / n as f32).cos())
            .collect();
        let mut planner = FftPlanner::<f32>::new();
        let fft_fwd = planner.plan_fft_forward(n);
        let fft_inv = planner.plan_fft_inverse(n);
        let buf_len = 2 * n;

        Self {
            n,
            half,
            hs,
            window,
            fft_fwd,
            fft_inv,
            spec_l: vec![Complex::new(0.0, 0.0); n],
            spec_r: vec![Complex::new(0.0, 0.0); n],
            spec_mid: vec![Complex::new(0.0, 0.0); n],
            synth_phase: [vec![0.0; half], vec![0.0; half]],
            ana_phase: [vec![0.0; half], vec![0.0; half]],
            owner: [vec![0; half], vec![0; half]],
            seeded: [false, false],
            inst_freq: vec![0.0; half],
            prev_mid_phase: vec![0.0; half],
            in_pos: -(n as f64),
            out_frac: 0.0,
            primed: false,
            ola: [vec![0.0; buf_len], vec![0.0; buf_len]],
            norm: vec![0.0; buf_len],
            fifo_l: Vec::with_capacity(8192),
            fifo_r: Vec::with_capacity(8192),
            fifo_read: 0,
            peak_buf: Vec::with_capacity(64),
            identity_phases: false,
        }
    }

    /// Reset all state and seek to `pos` in the source.
    ///
    /// Analysis starts one window *before* `pos` so the first frames have a
    /// full set of overlapping windows behind them. Without that prime the
    /// weighted overlap-add divides by a partial sum of squared windows for
    /// the first `n` output samples, which amplifies by `1/w[i]` and spikes
    /// hard where the Hann window approaches zero. Reading before the start
    /// yields zeros, which is exactly the right content to prime with.
    pub fn reset(&mut self, pos: f64) {
        self.in_pos = pos - self.n as f64;
        self.out_frac = 0.0;
        self.primed = false;
        self.fifo_l.clear();
        self.fifo_r.clear();
        self.fifo_read = 0;
        for c in 0..2 {
            self.synth_phase[c].fill(0.0);
            self.ana_phase[c].fill(0.0);
            self.seeded[c] = false;
            self.ola[c].fill(0.0);
        }
        self.norm.fill(0.0);
        self.prev_mid_phase.fill(0.0);
    }

    /// Source frame currently being heard. `in_pos` is where the next analysis
    /// frame will read, so the heard position trails it by half a window of
    /// lookahead plus whatever has been stretched but not yet emitted.
    pub fn get_play_pos(&self, speed: f32, pitch_ratio: f32) -> f64 {
        let pr = pitch_ratio.clamp(0.5, 2.0) as f64;
        let stretch = (speed as f64 / pr).clamp(0.1, 2.0);
        let buffered = self.fifo_l.len().saturating_sub(self.fifo_read) as f64;
        (self.in_pos - self.n as f64 * 0.5 - buffered * stretch).max(0.0)
    }

    /// Produce `out_frames` stereo frames of pitch-shifted audio.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        samples: &[f32],
        ch: usize,
        total_frames: usize,
        out_frames: usize,
        speed: f32,
        pitch_ratio: f32,
        out_l: &mut [f32],
        out_r: &mut [f32],
        finished: &mut bool,
    ) {
        if total_frames == 0 {
            out_l[..out_frames].fill(0.0);
            out_r[..out_frames].fill(0.0);
            return;
        }

        let pr = pitch_ratio.clamp(0.5, 2.0);
        let stretch = (speed / pr).clamp(0.1, 2.0) as f64;
        let step = pr as f64;

        // The resampler consumes `step` FIFO samples per output frame, so the
        // FIFO must hold out_frames * step of them.
        let needed = (out_frames as f64 * step).ceil() as usize + 8;
        let end = total_frames as f64 + self.n as f64;

        // Run whole analysis frames until the FIFO covers this block. The cap
        // is a safety net against a pathological ratio, not a limit reached
        // in normal operation.
        let mut guard = 0usize;
        while self.fifo_l.len().saturating_sub(self.fifo_read) < needed
            && self.in_pos < end
            && guard < 64
        {
            self.run_frame(samples, ch, total_frames, stretch);
            guard += 1;
        }

        // Resample the stretched FIFO by `step`.
        let fifo_len = self.fifo_l.len();
        for m in 0..out_frames {
            let pos = self.fifo_read as f64 + m as f64 * step;
            let idx = pos.floor() as usize;
            let t = (pos - pos.floor()) as f32;
            // saturating_sub: idx is legitimately 0 right after a reset.
            let km1 = idx.saturating_sub(1);
            if idx + 2 < fifo_len {
                out_l[m] = hermite(
                    self.fifo_l[km1],
                    self.fifo_l[idx],
                    self.fifo_l[idx + 1],
                    self.fifo_l[idx + 2],
                    t,
                );
                out_r[m] = hermite(
                    self.fifo_r[km1],
                    self.fifo_r[idx],
                    self.fifo_r[idx + 1],
                    self.fifo_r[idx + 2],
                    t,
                );
            } else if idx < fifo_len {
                let nxt = (idx + 1).min(fifo_len - 1);
                out_l[m] = self.fifo_l[idx] + (self.fifo_l[nxt] - self.fifo_l[idx]) * t;
                out_r[m] = self.fifo_r[idx] + (self.fifo_r[nxt] - self.fifo_r[idx]) * t;
            } else {
                out_l[m] = 0.0;
                out_r[m] = 0.0;
            }
        }
        self.fifo_read = ((self.fifo_read as f64 + out_frames as f64 * step) as usize).min(fifo_len);

        // Reclaim consumed FIFO space, keeping two samples of history for the
        // interpolator.
        let retain = 2usize;
        if self.fifo_read > retain + 256 {
            let discard = (self.fifo_read - retain).min(self.fifo_l.len());
            self.fifo_l.copy_within(discard.., 0);
            self.fifo_l.truncate(self.fifo_l.len() - discard);
            self.fifo_r.copy_within(discard.., 0);
            self.fifo_r.truncate(self.fifo_r.len() - discard);
            self.fifo_read = retain.min(self.fifo_l.len());
        }

        if self.in_pos >= end && self.fifo_l.len().saturating_sub(self.fifo_read) == 0 {
            *finished = true;
        }
    }

    /// Analyse one window, resynthesise it, overlap-add it, and advance.
    ///
    /// The analysis hop carries the stretch ratio: `ha = hs * stretch`, giving
    /// a stream tempo of `ha / hs = stretch`. The synthesis hop stays fixed so
    /// the overlap-add normalisation is exact.
    fn run_frame(
        &mut self,
        samples: &[f32],
        ch: usize,
        total_frames: usize,
        stretch: f64,
    ) {
        let n = self.n;
        // Fractional is fine: the read position is already fractional, and the
        // expected-advance maths below uses the same value.
        let ha = self.hs as f64 * stretch;
        let start = self.in_pos;

        for i in 0..n {
            let (l, r) = read_stereo(samples, ch, total_frames, start + i as f64);
            self.spec_l[i] = Complex::new(l * self.window[i], 0.0);
            self.spec_r[i] = Complex::new(r * self.window[i], 0.0);
        }
        self.fft_fwd.process(&mut self.spec_l);
        self.fft_fwd.process(&mut self.spec_r);

        // Mid spectrum: one complex average, no extra transform. Both channels
        // take their instantaneous frequencies from here, so they are warped
        // identically and cannot drift apart.
        for k in 0..n {
            self.spec_mid[k] = (self.spec_l[k] + self.spec_r[k]) * 0.5;
        }

        // Instantaneous frequency per bin: the expected advance over the
        // analysis hop, corrected by the measured deviation wrapped to
        // (-pi, pi].
        let ha_f = ha as f32;
        for k in 0..self.half {
            let omega = std::f32::consts::TAU * k as f32 / n as f32;
            let expected = omega * ha_f;
            let cur = self.spec_mid[k].arg();
            let adv = if self.primed {
                expected + princarg(cur - self.prev_mid_phase[k] - expected)
            } else {
                expected
            };
            self.inst_freq[k] = adv / ha_f;
            self.prev_mid_phase[k] = cur;
        }

        resynthesise(
            &mut self.spec_l,
            &mut self.synth_phase[0],
            &mut self.ana_phase[0],
            &mut self.owner[0],
            &self.inst_freq,
            &mut self.peak_buf,
            &mut self.seeded[0],
            n,
            self.half,
            self.hs as f64,
            self.identity_phases,
        );
        resynthesise(
            &mut self.spec_r,
            &mut self.synth_phase[1],
            &mut self.ana_phase[1],
            &mut self.owner[1],
            &self.inst_freq,
            &mut self.peak_buf,
            &mut self.seeded[1],
            n,
            self.half,
            self.hs as f64,
            self.identity_phases,
        );

        self.fft_inv.process(&mut self.spec_l);
        self.fft_inv.process(&mut self.spec_r);
        self.overlap_add();

        self.in_pos += ha;
        self.primed = true;
    }

    /// Overlap-add the resynthesised frame and emit one synthesis hop into
    /// the FIFO.
    fn overlap_add(&mut self) {
        let n = self.n;
        let buf_len = 2 * n;
        let emit = self.hs;

        // Drop what we are about to emit, keeping one window of history.
        self.ola[0].copy_within(emit..buf_len, 0);
        self.ola[1].copy_within(emit..buf_len, 0);
        self.norm.copy_within(emit..buf_len, 0);
        for x in self.ola[0][buf_len - emit..].iter_mut() {
            *x = 0.0;
        }
        for x in self.ola[1][buf_len - emit..].iter_mut() {
            *x = 0.0;
        }
        for x in self.norm[buf_len - emit..].iter_mut() {
            *x = 0.0;
        }

        // rustfft's inverse transform is unnormalised, so the time-domain
        // frame comes back scaled by N. Fold the 1/N in here: dividing the
        // overlap-add by the running sum of squared synthesis windows then
        // reconstructs exactly, with no extra gain.
        let inv_n = 1.0 / n as f32;
        for i in 0..n {
            let w = self.window[i];
            self.norm[i] += w * w;
            self.ola[0][i] += self.spec_l[i].re * w * inv_n;
            self.ola[1][i] += self.spec_r[i].re * w * inv_n;
        }

        for i in 0..emit {
            let g = if self.norm[i] > 1e-8 {
                1.0 / self.norm[i]
            } else {
                0.0
            };
            if std::env::var("VOCODER_PROBE").is_ok() && self.fifo_l.len() < emit * 2 {
                eprintln!(
                    "PROBE frame={} i={i} w={:.4e} norm={:.4e} ola={:.6e} g={:.4e} out={:.4}",
                    self.fifo_l.len() / emit,
                    self.window[i],
                    self.norm[i],
                    self.ola[0][i],
                    g,
                    self.ola[0][i] * g
                );
            }
            self.fifo_l.push(self.ola[0][i] * g);
            self.fifo_r.push(self.ola[1][i] * g);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use rustfft::num_complex::Complex;
    use rustfft::FftPlanner;

    const SR: f32 = 44_100.0;

    /// Stereo sine: `freq` on the left, `freq * 1.5` on the right, so the two
    /// channels are distinguishable in a downmix.
    fn stereo_sine(frames: usize, freq: f32) -> Vec<f32> {
        let mut s = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let t = i as f32 / SR;
            s.push((std::f32::consts::TAU * freq * t).sin());
            s.push((std::f32::consts::TAU * freq * t * 1.5).sin());
        }
        s
    }

    fn sine(frames: usize, freq: f32) -> Vec<f32> {
        let mut s = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let v = (std::f32::consts::TAU * freq * i as f32 / SR).sin();
            s.push(v);
            s.push(v);
        }
        s
    }

    /// Render `secs` of the left channel.
    fn render_left(
        v: &mut PhaseVocoder,
        samples: &[f32],
        total: usize,
        speed: f32,
        pr: f32,
        secs: f32,
    ) -> Vec<f32> {
        let blocks = ((secs * SR) as usize) / 512;
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        let mut out = Vec::with_capacity(blocks * 512);
        for _ in 0..blocks {
            let mut fin = false;
            v.process(samples, 2, total, 512, speed, pr, &mut l, &mut r, &mut fin);
            out.extend_from_slice(&l);
        }
        out
    }

    fn peak_hz(x: &[f32], sr: f32) -> f32 {
        let n = x.len();
        let mut buf: Vec<Complex<f32>> = x.iter().map(|v| Complex::new(*v, 0.0)).collect();
        let fft = FftPlanner::<f32>::new().plan_fft_forward(n);
        fft.process(&mut buf);
        let mut best = 0usize;
        let mut bestv = 0.0f32;
        for (k, cell) in buf.iter().enumerate().take(n / 2).skip(1) {
            let m = cell.norm();
            if m > bestv {
                bestv = m;
                best = k;
            }
        }
        best as f32 * sr / n as f32
    }

    #[test]
    fn unity_ratio_reconstructs_the_input() {
        // speed == pitch_ratio means no time-stretch and no pitch change: the
        // vocoder must be transparent, which is what makes it safe to use.
        // The frequency sweep guards the phase-locking formulation: level loss
        // used to depend on where the partial sat relative to the bin grid.
        let total = (SR * 4.0) as usize;
        let a = 20_000;
        let b = a + 8192;
        for f in [300.0f32, 440.0, 660.0, 1000.0, 2000.0] {
            let samples = sine(total, f);
            let mut v = PhaseVocoder::new();
            let out = render_left(&mut v, &samples, total, 1.0, 1.0, 2.0);
            let in_rms: f32 =
                (a..b).map(|i| samples[i * 2].powi(2)).sum::<f32>() / (b - a) as f32;
            let out_rms: f32 = out[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32;
            let ratio = (out_rms / in_rms).sqrt();
            assert!(
                (0.9..1.1).contains(&ratio),
                "{f:.0} Hz unity level changed: {ratio:.3}"
            );
            let peak = peak_hz(&out[a..b], SR);
            assert!(
                (0.97 * f..1.03 * f).contains(&peak),
                "{f:.0} Hz moved to {peak:.1} Hz at unity ratio"
            );
        }
    }

    #[test]
    fn pitch_is_accurate_and_independent_of_speed() {
        // Measured on the left channel: a mono downmix of two differently
        // pitched channels peaks at whichever channel came out stronger, which
        // masqueraded as a pitch error.
        let total = (SR * 6.0) as usize;
        let samples = stereo_sine(total, 440.0);
        for (label, speed, pr) in [
            ("+1 octave", 1.0f32, 2.0f32),
            ("-1 octave", 1.0f32, 0.5f32),
            ("+1 octave at 2x speed", 2.0f32, 2.0f32),
            ("+1 octave at 0.5x speed", 0.5f32, 2.0f32),
        ] {
            let mut v = PhaseVocoder::new();
            let out = render_left(&mut v, &samples, total, speed, pr, 2.5);
            let a = 20_000;
            let b = a + 16384;
            let f = peak_hz(&out[a..b], SR);
            let expect = 440.0 * pr;
            let cents = (f / expect).ln() * 1200.0 / std::f32::consts::LN_2;
            assert!(
                cents.abs() < 30.0,
                "{label}: {f:.1} Hz, expected {expect:.1} ({cents:+.0} cents)"
            );
        }
    }

    #[test]
    fn output_is_stable_and_finite() {
        let total = (SR * 4.0) as usize;
        let mut samples = Vec::with_capacity(total * 2);
        let mut seed = 0x9E3779B97F4A7C15u64;
        for _ in 0..total {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let v = ((seed >> 33) as f32 / u32::MAX as f32) - 0.5;
            samples.push(v * 0.4);
            samples.push(v * 0.4);
        }
        for (speed, pr) in [(1.0f32, 2.0f32), (0.5, 2.0), (1.5, 1.7), (1.0, 0.5)] {
            let mut v = PhaseVocoder::new();
            let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
            for _ in 0..200 {
                let mut fin = false;
                v.process(&samples, 2, total, 512, speed, pr, &mut l, &mut r, &mut fin);
                for s in l.iter().chain(r.iter()) {
                    assert!(s.is_finite(), "non-finite output at speed {speed} pr {pr}");
                    assert!(s.abs() < 4.0, "output diverged to {s}");
                }
            }
        }
    }

    #[test]
    fn stereo_image_survives() {
        // Uncorrelated channels must not collapse into each other, and both
        // must carry real signal.
        let total = (SR * 4.0) as usize;
        let mut samples = Vec::with_capacity(total * 2);
        for i in 0..total {
            let t = i as f32 / SR;
            samples.push((std::f32::consts::TAU * 440.0 * t).sin());
            samples.push((std::f32::consts::TAU * 587.0 * t).sin());
        }
        let mut v = PhaseVocoder::new();
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        let (mut lr, mut rr, mut dot) = (0.0f32, 0.0f32, 0.0f32);
        for _ in 0..150 {
            let mut fin = false;
            v.process(&samples, 2, total, 512, 1.0, 2.0, &mut l, &mut r, &mut fin);
            lr += l.iter().map(|v| v * v).sum::<f32>();
            rr += r.iter().map(|v| v * v).sum::<f32>();
            dot += l.iter().zip(r.iter()).map(|(a, b)| a * b).sum::<f32>();
        }
        assert!(lr > 1e-3 && rr > 1e-3, "a channel went silent ({lr:.5}, {rr:.5})");
        let corr = dot / (lr.sqrt() * rr.sqrt());
        assert!(corr.abs() < 0.9, "channels collapsed together ({corr:.3})");
    }

    #[test]
    fn reset_clears_state_and_seeks() {
        let total = (SR * 4.0) as usize;
        let samples = stereo_sine(total, 440.0);
        let mut v = PhaseVocoder::new();
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        for _ in 0..100 {
            let mut fin = false;
            v.process(&samples, 2, total, 512, 1.0, 2.0, &mut l, &mut r, &mut fin);
        }
        v.reset(0.0);
        // Analysis is primed one window before the seek point, so the first
        // frames have a full overlap behind them.
        assert_eq!(v.in_pos, -(v.n as f64));
        assert!(v.fifo_l.is_empty() && v.fifo_read == 0);
        let mut fin = false;
        v.process(&samples, 2, total, 512, 1.0, 2.0, &mut l, &mut r, &mut fin);
        assert!(l.iter().all(|s| s.is_finite()));
    }

    #[test]
    fn identity_phases_reconstruct_exactly() {
        // With the phase processing switched off the engine is a plain STFT
        // analysis->synthesis, which must reconstruct bit-for-bit. A failure
        // here indicts the overlap-add plumbing, not the phase logic.
        let total = (SR * 4.0) as usize;
        let samples = sine(total, 300.0);
        let mut v = PhaseVocoder::new();
        v.identity_phases = true;
        let out = render_left(&mut v, &samples, total, 1.0, 1.0, 2.0);
        let a = 20_000;
        let b = a + 8192;
        let in_rms: f32 = (a..b).map(|i| samples[i * 2].powi(2)).sum::<f32>() / (b - a) as f32;
        let out_rms: f32 = out[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32;
        let ratio = (out_rms / in_rms).sqrt();
        assert!(
            (0.99..1.01).contains(&ratio),
            "pure STFT did not reconstruct: {ratio:.3}"
        );
    }

    #[test]
    fn reset_into_the_middle_of_a_track_does_not_explode() {
        // A seek or an engine switch calls reset() at a non-zero source
        // position. The prime frame then carries real audio, so the
        // overlap-add divides a real numerator by a partial sum of squared
        // windows — and a periodic Hann starts at zero, so the first emitted
        // block came out amplified by 1/w[i], thousands of times over. With
        // the reverb tail attached that burst rings for seconds.
        let total = (SR * 4.0) as usize;
        let samples = sine(total, 300.0);
        let mut v = PhaseVocoder::new();
        v.reset(total as f64 * 0.5);
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        let mut fin = false;
        v.process(&samples, 2, total, 512, 1.0, 1.334, &mut l, &mut r, &mut fin);
        let peak = l
            .iter()
            .chain(r.iter())
            .fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak.is_finite(), "reset produced a non-finite sample: {peak}");
        assert!(peak < 2.0, "reset at mid-track spiked to {peak:.1}");
    }
}
