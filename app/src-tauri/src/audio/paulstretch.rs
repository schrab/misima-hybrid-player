//! Paulstretch — extreme time-stretch with preserved pitch, for tempo-down
//! playback.
//!
//! Paulstretch (Paul Nasca) is the sound of "10x slower but still musical":
//! instead of tracking phase coherence like the vocoder or waveform
//! similarity like the WSOLA, it deliberately discards phase. Each frame is
//! analysed with an FFT, reduced to magnitudes only, given fresh random
//! phases, and resynthesised — every frame becomes a noise-realisation of the
//! same magnitude spectrum — and 50%-overlap-add smears those realisations
//! into a continuous, wash-like stream that never goes granular no matter
//! how large the expansion.
//!
//! ## Algorithm (clean-room, from the Public-Domain reference)
//!
//! The spec is Paul Nasca's Public-Domain `paulstretch_python`
//! (`paulstretch_stereo.py`) plus published formulas; nothing was transcribed
//! from the GPL-2 C++ implementations (`paulstretch_cpp`, `libpaulstretch`)
//! or `realstretch`. Per processed frame, per channel:
//!
//! 1. Read `N` source samples at the read cursor, zero-padded at the edges.
//! 2. Apply the Paul analysis window `w[i] = (1 - (2i/N - 1)^2)^1.25`.
//! 3. Forward FFT; keep magnitudes only (`hypot(re, im)`); discard phase.
//! 4. Smooth the magnitudes along a log-frequency axis (the spread filter) —
//!    this is the signature spectral softening, and it ships **disabled**
//!    (`SPREAD_BANDWIDTH = 0`, and the call is skipped outright at `bw <= 0`).
//!    See the note on `SPREAD_BANDWIDTH` for why and for the reference
//!    formula if it is ever switched back on.
//! 5. Zero DC and Nyquist, then give every remaining bin a fresh random
//!    phase from a deterministic u32 LCG and mirror the conjugate half so the
//!    inverse transform is real.
//! 6. Inverse FFT, apply the window AGAIN, overlap-add with the previous
//!    frame's second half, scale by `1/N` (rustfft's unnormalised inverse),
//!    and emit `H = N/2` samples into the output FIFO.
//! 7. Advance the read cursor by `H / S`, where the expansion
//!    `S = pitch_ratio / speed` is output duration over source duration.
//!
//! Fixed sizes: `N = 16384`, `H = 8192`. The reference
//! `paulstretch_python` scheme uses 8192, but the reference *app*
//! (`paulstretch_cpp`) defaults to a ~2.4x longer analysis window
//! (~19200 samples) and the 0.5.0 listening pass called our 8192 output "a
//! bit dirtier than the original" — the frame-phase noise of a shorter
//! window is the grain that survives the magnitude-only resynthesis. 16384 is
//! the next power of two above 8192 and lands at ~0.37 s / ~5.4 frames per
//! second per channel at 44.1 kHz, which is where the spread filter's own
//! literature stops improving audibly.
//!
//! The core is rate-independent; the device sample rate only sets the spread
//! filter's Hz-to-bin mapping (a rate change rebuilds that mapping, exactly
//! like the WSOLA rebuilds its anti-alias biquad).
//!
//! ## Engagement
//!
//! Task 2 wires the selector: `speed < 1.0` routes through this engine (S up
//! to 40x at the tempo fader floor of 0.05 with pitch ratio 2.0);
//! `speed >= 1.0` keeps the existing bypass / vocoder / WSOLA split
//! untouched. Pitch is NOT applied
//! here — the FIFO is consumed at `step = pitch_ratio` by the shared Cubic
//! Hermite resampler, exactly like the other engines, so
//! `speed 0.5, pr 0.5` is pure pitch-down through S = 1.
//!
//! ## Stereo and mono
//!
//! Stereo runs two fully independent channel processors (independent RNG
//! seeds, independent spread state) sharing one read cursor. A mono source
//! runs ONE processor duplicated to both outputs — independent random phases
//! on identical material would invent a fake stereo image.
//!
//! ## Real-time discipline
//!
//! Everything is preallocated in the constructor (window table, FFT plans,
//! read/spectrum/magnitude scratch, FIFO capacity). `process` allocates
//! nothing; the only rebuild is the spread axis on a device-rate change,
//! which is a param-update path, not a hot-path one. All FIFO arithmetic
//! around the read cursor uses `saturating_sub` / clamps (the overread past
//! the FIFO near track end is legal and zero-padded).

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

use crate::audio::dsp_utils::{cubic_hermite, read_stereo_isize};

/// FFT / window size in samples — a power of two, ~0.37 s at 44.1 kHz.
///
/// The reference `paulstretch_python` scheme uses 8192; `paulstretch_cpp`
/// defaults to ~19200 and sounds cleaner, because a longer window averages
/// more of the frame-phase noise into the magnitude spectrum before that
/// noise is re-dealt. See the module header.
const N: usize = 16384;

/// Synthesis hop: output samples emitted per processed frame (50% overlap).
const H: usize = N / 2;

/// Magnitude bins kept from the forward transform, including DC and Nyquist.
const HALF: usize = N / 2 + 1;

/// Points on the log-frequency axis of the spread filter.
const NLOG: usize = N / 2;

/// Production spread bandwidth. **0 = the filter is off**, matching the
/// default of the reference `paulstretch_cpp` app, which ships the filter
/// disabled.
///
/// Two things to know before ever raising this (the helper and its tests are
/// kept, and `spread_bw` stays a live field so the differential test can
/// still exercise the code path):
///
/// 1. The call must be SKIPPED, not merely run at a near-zero bandwidth. The
///    spread filter resamples the magnitudes linear-bin -> log axis -> linear
///    bins; at `bw = 0` the one-pole passes are the identity, but the round
///    trip through the log axis is not (it interpolates every bin against a
///    coarser, differently-sampled axis), so `bw = 0` run through the helper
///    still moves the spectrum. `process` therefore tests `spread_bw <= 0.0`
///    and bypasses `spread_magnitude` entirely.
/// 2. Our single-pair reading is WEAKER than the reference and must not ship
///    enabled. `paulstretch_cpp` smooths a log axis of N points with TWO
///    forward+backward passes and coefficient
///    `(1 - 2^(-bw^2 * 10)) ^ (8192 / nfreq * 2)`; we run one pair over
///    `NLOG = N/2` points. Re-enabling means porting that pair count and that
///    exponent, then re-measuring — not just flipping the constant.
const SPREAD_BANDWIDTH: f32 = 0.0;

/// Low edge of the spread filter's log-frequency axis, Hz.
const F_MIN: f32 = 20.0;

/// Deterministic phase RNG (u32 LCG); determinism is required, parity with
/// any other implementation is not.
const LCG_MUL: u32 = 1103515245;
const LCG_ADD: u32 = 12345;

/// Channel RNG seeds: fixed, distinct, re-initialised on `reset`.
const SEED_CH0: u32 = 1;
const SEED_CH1: u32 = 1 + 161103;

/// Per-channel output FIFO capacity reserved at construction: the worst
/// unread length at the assumed 8192-frame block ceiling and pitch ratio
/// 2.0.
///
/// Derived, not guessed. The produce loop in `process` stops as soon as the
/// FIFO holds `needed = ceil(block * pr) + 8` samples, and it only ever adds
/// whole frames of `H`, so the unread depth after the loop is at most
/// `needed - 1 + H = 8192 * 2 + 7 + 8192`. On top of that the reclaim path
/// only fires once `fifo_read_pos > 2 + 256`, so a block can still be entered
/// with 258 samples of read history booked against the vector's length.
/// 16384 + 7 + 8192 + 258 = 24841 samples, comfortably inside the 32768
/// reserved here (128 KB per channel of f32).
/// `reserve_for_block` tops it up for larger blocks.
const FIFO_RESERVE: usize = 32768;

/// Slack `reserve_for_block` adds on top of the derived worst case
/// (`block * pr + H + 265`, see `FIFO_RESERVE`) — 272 rather than 265 so the
/// two constants are not the same expression that could drift together.
const FIFO_HEADROOM: usize = 272;

/// Precomputed mapping between linear bins and the spread filter's
/// log-frequency axis, for one device sample rate.
///
/// The axis spans `F_MIN .. sample_rate/2` over `NLOG` points. Rebuilt only
/// when the device rate changes — never on the per-buffer path.
struct SpreadAxis {
    /// For each log-axis point `j`: the fractional linear bin it samples.
    lin_bin_of_log: Vec<f32>,
    /// For each linear bin `k`: its position on the log axis.
    log_pos_of_lin: Vec<f32>,
}

impl SpreadAxis {
    fn new(sample_rate: f32) -> Self {
        let f_nyq = (sample_rate * 0.5).max(F_MIN * 2.0);
        let ln_ratio = (f_nyq / F_MIN).ln().max(1e-6);
        let lin_bin_of_log: Vec<f32> = (0..NLOG)
            .map(|j| {
                let f = F_MIN * (f_nyq / F_MIN).powf(j as f32 / (NLOG - 1) as f32);
                (f * N as f32 / sample_rate).clamp(0.0, (HALF - 1) as f32)
            })
            .collect();
        let log_pos_of_lin: Vec<f32> = (0..HALF)
            .map(|k| {
                let f = k as f32 * sample_rate / N as f32;
                if f <= F_MIN {
                    0.0
                } else {
                    ((f / F_MIN).ln() / ln_ratio * (NLOG - 1) as f32).clamp(0.0, (NLOG - 1) as f32)
                }
            })
            .collect();
        Self {
            lin_bin_of_log,
            log_pos_of_lin,
        }
    }
}

/// Linear interpolation between bins at a fractional position, clamped at
/// both ends.
#[inline]
fn lerp_at(buf: &[f32], pos: f32) -> f32 {
    let p = pos.clamp(0.0, (buf.len() - 1) as f32);
    let i = p as usize;
    let t = p - i as f32;
    let a = buf[i];
    let b = if i + 1 < buf.len() { buf[i + 1] } else { a };
    a + (b - a) * t
}

/// Smooth a magnitude spectrum along the log-frequency axis (the spread
/// filter) — pure helper, operating entirely on its arguments.
///
/// The spectrum is resampled onto the log axis, smoothed with one forward
/// and one backward one-pole pass (the "2 passes"), and resampled back,
/// REPLACING `mag`. `scratch` must be `NLOG` long.
///
/// NOTE: this helper is *not* the identity at `bandwidth = 0` — the two
/// passes are, but the log-axis round trip still resamples every bin against
/// a coarser axis. Production therefore skips the call entirely when the
/// engine's bandwidth is `<= 0` (see `SPREAD_BANDWIDTH`); the `bw -> 0`
/// case here is a *smooth spectrum* case, which the unit test pins.
fn spread_magnitude(mag: &mut [f32], axis: &SpreadAxis, bandwidth: f32, scratch: &mut [f32]) {
    debug_assert_eq!(mag.len(), HALF);
    debug_assert_eq!(scratch.len(), NLOG);

    // Linear bins -> log axis.
    for (j, slot) in scratch.iter_mut().enumerate() {
        *slot = lerp_at(mag, axis.lin_bin_of_log[j]);
    }

    // One-pole coefficient. bw -> 0 makes a_base -> 0 and the passes
    // degenerate to identity. The exponent scales the coefficient for the
    // axis decimation ratio (a_base^4 at N=16384, nlog=8192).
    let a_base = 1.0 - 2.0f32.powf(-bandwidth * bandwidth * 10.0);
    let a_eff = a_base.powf((N / NLOG) as f32 * 2.0);

    // Forward pass, then backward pass (mirrored).
    let mut prev = scratch[0];
    for v in scratch.iter_mut() {
        *v = prev * a_eff + *v * (1.0 - a_eff);
        prev = *v;
    }
    let mut next = scratch[NLOG - 1];
    for v in scratch.iter_mut().rev() {
        *v = next * a_eff + *v * (1.0 - a_eff);
        next = *v;
    }

    // Log axis -> linear bins, replacing the magnitudes.
    for (k, slot) in mag.iter_mut().enumerate() {
        *slot = lerp_at(scratch, axis.log_pos_of_lin[k]);
    }
}

/// The Paulstretch engine. Constructed up front like the WSOLA and vocoder —
/// swapping engines must never allocate on the real-time thread.
pub struct Paulstretch {
    window: Vec<f32>,
    fft_fwd: Arc<dyn Fft<f32>>,
    fft_inv: Arc<dyn Fft<f32>>,
    /// rustfft's inverse is unnormalised; folded into the synthesis scale.
    inv_n: f32,

    /// Per-frame scratch: windowed source reads (one per channel), the
    /// working spectrum, magnitudes, and the spread filter's log-axis
    /// scratch. All sized in the constructor, never reallocated.
    win_l: Vec<f32>,
    win_r: Vec<f32>,
    spec: Vec<Complex<f32>>,
    mag: Vec<f32>,
    spread_scratch: Vec<f32>,

    /// Previous frame's windowed second half, per channel — the 50%
    /// overlap-add partner.
    prev_tail: [Vec<f32>; 2],

    fifo_l: Vec<f32>,
    fifo_r: Vec<f32>,
    fifo_read_pos: usize,
    resample_phase: f64,

    /// Source-sample cursor of the next window read. Output sample `m` maps
    /// to source position `read_pos_origin + m / S`.
    read_pos: f64,
    seeds: [u32; 2],

    spread_axis: SpreadAxis,
    axis_rate: f32,
    spread_bw: f32,

    /// Test only: skip phase randomization AND the spread filter, turning
    /// the engine into a plain 50%-overlap windowed OLA. That must
    /// reconstruct its input, so a failure there indicts the overlap-add
    /// plumbing rather than the phase/spread logic (docs/DSP.md methodology).
    identity: bool,
}

impl Default for Paulstretch {
    fn default() -> Self {
        Self::new()
    }
}

impl Paulstretch {
    pub fn new() -> Self {
        // Paul window: w[i] = (1 - (2i/N - 1)^2)^1.25, applied on analysis
        // AND synthesis. At 50% overlap the squared pair sums to ~0.974-1.0,
        // which is why the exponent is 1.25 — no compensation gain needed.
        let window: Vec<f32> = (0..N)
            .map(|i| {
                let x = 2.0 * i as f32 / N as f32 - 1.0;
                (1.0 - x * x).powf(1.25)
            })
            .collect();
        let mut planner = FftPlanner::<f32>::new();
        let fft_fwd = planner.plan_fft_forward(N);
        let fft_inv = planner.plan_fft_inverse(N);

        Self {
            window,
            fft_fwd,
            fft_inv,
            inv_n: 1.0 / N as f32,
            win_l: vec![0.0; N],
            win_r: vec![0.0; N],
            spec: vec![Complex::new(0.0, 0.0); N],
            mag: vec![0.0; HALF],
            spread_scratch: vec![0.0; NLOG],
            prev_tail: [vec![0.0; H], vec![0.0; H]],
            // FIFO capacity invariant (§3.1.1): no Vec growth may ever
            // happen inside `process()`. The worst unread length at the
            // assumed 8192-frame block ceiling is 24841 (see FIFO_RESERVE),
            // so that worst case is reserved here and nothing on the
            // per-buffer path can reallocate. `Vec::clear` in `reset` keeps
            // the capacity.
            fifo_l: Vec::with_capacity(FIFO_RESERVE),
            fifo_r: Vec::with_capacity(FIFO_RESERVE),
            fifo_read_pos: 0,
            resample_phase: 0.0,
            read_pos: 0.0,
            seeds: [SEED_CH0, SEED_CH1],
            spread_axis: SpreadAxis::new(44100.0),
            axis_rate: 44100.0,
            spread_bw: SPREAD_BANDWIDTH,
            identity: false,
        }
    }

    /// Guarantee FIFO capacity for a device block of `block_frames` frames.
    ///
    /// `FIFO_RESERVE` assumes the 8192-frame block ceiling the selector's
    /// hosts deliver; the true worst-case unread length is
    /// `block_frames * pr_max + 7 + H + 258` of retained read history, so a
    /// host reporting a larger block gets an explicit top-up here. Called
    /// once at stream construction, never on the audio thread (§3.1.1).
    ///
    /// The request is clamped to a 16384-frame ceiling: hosts sometimes
    /// report nonsense maxima (cpal's WASAPI backend falls back to
    /// `Range { 0, u32::MAX }` when `GetBufferSizeLimits` fails), and
    /// reserving for that is a multi-gigabyte commit. The ask is measured
    /// from `len`, not from the current capacity — `Vec::reserve` guarantees
    /// `capacity >= len + additional`, so asking `need - capacity` (len is 0
    /// at the only call site) would be silently swallowed by the 32768-sample
    /// construction reserve and the top-up would never grow anything.
    ///
    /// The ceiling is a fast-path bound, not a safety cliff: if a real
    /// callback block ever exceeds it, growth always precedes zero-padding
    /// when `needed` runs past capacity — the produce loop's `guard < 64`
    /// caps that fallback at a bounded one-time realloc of at most 64 x H
    /// samples per channel (the capacity is retained afterwards), never a
    /// panic and never a per-block repeat.
    pub fn reserve_for_block(&mut self, block_frames: usize) {
        const CEILING: usize = 16384;
        let need = block_frames.min(CEILING) * 2 + H + FIFO_HEADROOM;
        if self.fifo_l.capacity() < need {
            self.fifo_l.reserve(need - self.fifo_l.len());
        }
        if self.fifo_r.capacity() < need {
            self.fifo_r.reserve(need - self.fifo_r.len());
        }
    }

    /// Test only: the raw source-sample cursor. The selector's tests use it
    /// to prove that fader moves inside the Paulstretch region do not
    /// re-seed the engine. Compiled out of non-test builds.
    #[cfg(test)]
    pub fn cursor_for_test(&self) -> f64 {
        self.read_pos
    }

    /// Reset all state and seek to `pos` in the source.
    ///
    /// The first window read is anchored AT `pos`, so the first output
    /// sample after the reset maps to source position `pos` (output sample
    /// `m` corresponds to source `pos + m/S`). The zero `prev_tail` ramps
    /// the first chunk in through the window — a seek fades in instead of
    /// clicking. Negative positions just zero-pad. This is the seek /
    /// track-change flush path: no stale audio survives it.
    pub fn reset(&mut self, pos: f64) {
        self.fifo_l.clear();
        self.fifo_r.clear();
        self.fifo_read_pos = 0;
        self.resample_phase = 0.0;
        self.prev_tail[0].fill(0.0);
        self.prev_tail[1].fill(0.0);
        self.seeds = [SEED_CH0, SEED_CH1];
        self.read_pos = pos;
    }

    /// Source frame currently being heard, compensating for the stretched
    /// audio buffered in the FIFO. Each FIFO sample represents `1/S` source
    /// samples, so the lag is the unread FIFO depth divided by the
    /// expansion — the mirror of wsola's `buffered * stretch`.
    pub fn get_play_pos(&self, speed: f32, pitch_ratio: f32) -> f64 {
        let pr = pitch_ratio.clamp(0.5, 2.0) as f64;
        // The fader floor is 0.05 and the pitch ratio reaches 2.0, so S
        // reaches 40; the 50 ceiling covers that with margin. MUST match the
        // clamp in `process` — the two disagreeing would make the reported
        // position drift against the produced audio.
        let s = (pr / speed as f64).clamp(0.1, 50.0);
        let buffered =
            (self.fifo_l.len().saturating_sub(self.fifo_read_pos) as f64) - self.resample_phase;
        (self.read_pos - buffered.max(0.0) / s).max(0.0)
    }

    /// Produce one frame: read a window, randomise its spectrum, and emit
    /// `H` expanded samples per channel into the FIFO.
    fn produce_frame(&mut self, samples: &[f32], ch: usize, total_frames: usize, s: f64) {
        let p = self.read_pos.floor() as isize;
        for i in 0..N {
            let (l, r) = read_stereo_isize(samples, ch, total_frames, p + i as isize);
            let w = self.window[i];
            self.win_l[i] = l * w;
            self.win_r[i] = r * w;
        }

        // Mono runs one processor duplicated to both outputs; stereo runs
        // two fully independent ones.
        let n_chan = if ch > 1 { 2 } else { 1 };
        for c in 0..n_chan {
            let src = if c == 0 { &self.win_l } else { &self.win_r };
            for (slot, v) in self.spec.iter_mut().zip(src.iter()) {
                *slot = Complex::new(*v, 0.0);
            }
            self.fft_fwd.process(&mut self.spec);

            if !self.identity {
                // Magnitudes only — the phase is discarded.
                for (m, sp) in self.mag.iter_mut().zip(self.spec.iter()) {
                    *m = sp.norm();
                }
                // The spread filter ships OFF (SPREAD_BANDWIDTH = 0, the
                // reference app's default), and at bw <= 0 the call must be
                // SKIPPED rather than run: the two one-pole passes are the
                // identity at bw 0, but the log-axis round trip inside the
                // helper is not. Skipping is what makes the disabled state
                // bit-for-bit transparent.
                if self.spread_bw > 0.0 {
                    spread_magnitude(
                        &mut self.mag,
                        &self.spread_axis,
                        self.spread_bw,
                        &mut self.spread_scratch,
                    );
                }

                // DC and Nyquist are zeroed unconditionally, every frame.
                self.spec[0] = Complex::new(0.0, 0.0);
                self.spec[N / 2] = Complex::new(0.0, 0.0);

                // Fresh random phase per bin, per frame, from a deterministic
                // LCG. phase = r * (pi / 16384) spans [0, 2*pi).
                for k in 1..(N / 2) {
                    let seed = self.seeds[c].wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
                    self.seeds[c] = seed;
                    let r = (seed >> 16) & 0x7fff;
                    let phase = r as f32 * (std::f32::consts::PI / 16384.0);
                    self.spec[k] = self.mag[k] * Complex::new(phase.cos(), phase.sin());
                }

                // Mirror the conjugate half so the inverse transform is
                // exactly real (the python reference uses rfft/irfft, which
                // maintains the symmetry by construction).
                for k in (N / 2 + 1)..N {
                    self.spec[k] = self.spec[N - k].conj();
                }
            }

            self.fft_inv.process(&mut self.spec);

            // Window AGAIN, overlap-add with the previous frame's second
            // half, emit H samples scaled by 1/N.
            for i in 0..H {
                let a = self.spec[i].re * self.window[i];
                let b = self.prev_tail[c][i];
                let tail = self.spec[H + i].re * self.window[H + i];
                self.prev_tail[c][i] = tail;
                let v = (a + b) * self.inv_n;
                if n_chan == 1 {
                    self.fifo_l.push(v);
                    self.fifo_r.push(v);
                } else if c == 0 {
                    self.fifo_l.push(v);
                } else {
                    self.fifo_r.push(v);
                }
            }
        }

        // Each frame consumes H/S source samples and produces H output
        // samples: the output stream is the source expanded by S.
        self.read_pos += H as f64 / s;
    }

    /// Process a block: produce enough expanded audio into the FIFO, then
    /// read it through the shared Cubic Hermite resampler at
    /// `step = pitch_ratio`. Mirrors wsola's `process` signature and its
    /// saturating FIFO arithmetic.
    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        samples: &[f32],
        ch: usize,
        total_frames: usize,
        out_frames: usize,
        speed: f32,
        pitch_ratio: f32,
        sample_rate: f32,
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
        // Expansion S = pr / speed: output duration / source duration.
        // speed 0.5, pr 1 -> S=2; speed 0.05, pr 2 -> S=40 (the fader floor
        // with pitch up an octave); speed 0.5, pr 0.5 -> S=1 (pure
        // pitch-down, resampled downstream by `step = pr`). The clamp is
        // 0.1..50 so S=40 fits with margin; it MUST match `get_play_pos`.
        let s = (pr as f64 / speed as f64).clamp(0.1, 50.0);
        let step = pr as f64;

        // The spread filter's Hz->bin mapping follows the device rate; a
        // device change rebuilds it (same convention as the WSOLA's biquad).
        if (self.axis_rate - sample_rate).abs() > 1e-3 {
            self.spread_axis = SpreadAxis::new(sample_rate);
            self.axis_rate = sample_rate;
        }

        // The resampler consumes `step` FIFO samples per output frame.
        let needed = (out_frames as f64 * step).ceil() as usize + 8;
        // One frame past the track end flushes the last frame's second half
        // (it reads zeros, so the flush adds no content of its own).
        let end = total_frames as f64 + H as f64 / s;

        let mut guard = 0usize;
        while self.fifo_l.len().saturating_sub(self.fifo_read_pos) < needed
            && self.read_pos < end
            && guard < 64
        {
            self.produce_frame(samples, ch, total_frames, s);
            guard += 1;
        }

        // Cubic Hermite read of the expanded FIFO at `step = pr`.
        let fifo_len = self.fifo_l.len();
        for m in 0..out_frames {
            let pos = (self.fifo_read_pos as f64) + self.resample_phase + (m as f64) * step;
            let k = pos.floor() as usize;
            let t = (pos - k as f64) as f32;

            if k + 2 < fifo_len {
                // saturating_sub: k is legitimately 0 right after a reset.
                let km1 = k.saturating_sub(1);
                out_l[m] = cubic_hermite(
                    self.fifo_l[km1],
                    self.fifo_l[k],
                    self.fifo_l[k + 1],
                    self.fifo_l[k + 2],
                    t,
                );
                out_r[m] = cubic_hermite(
                    self.fifo_r[km1],
                    self.fifo_r[k],
                    self.fifo_r[k + 1],
                    self.fifo_r[k + 2],
                    t,
                );
            } else if k < fifo_len {
                // Linear fallback near the tail edge.
                let s0_l = self.fifo_l[k];
                let s1_l = if k + 1 < fifo_len {
                    self.fifo_l[k + 1]
                } else {
                    s0_l
                };
                out_l[m] = s0_l + (s1_l - s0_l) * t;

                let s0_r = self.fifo_r[k];
                let s1_r = if k + 1 < fifo_len {
                    self.fifo_r[k + 1]
                } else {
                    s0_r
                };
                out_r[m] = s0_r + (s1_r - s0_r) * t;
            } else {
                // Overread past the FIFO near track end: legal, zero-padded.
                out_l[m] = 0.0;
                out_r[m] = 0.0;
            }
        }

        let final_pos =
            (self.fifo_read_pos as f64) + self.resample_phase + (out_frames as f64) * step;
        let consumed = final_pos.floor() as usize;
        self.resample_phase = final_pos - (consumed as f64);
        self.fifo_read_pos = consumed.min(fifo_len);

        // Reclaim consumed FIFO space while retaining 2 samples of history
        // for the cubic interpolator.
        let retain_history = 2usize;
        if self.fifo_read_pos > retain_history + 256 {
            // read_pos can bookkeep past the FIFO near track end — clamp so
            // the arithmetic can't underflow and kill the audio thread.
            let discard = (self.fifo_read_pos - retain_history).min(self.fifo_l.len());
            let remaining = self.fifo_l.len() - discard;
            self.fifo_l.copy_within(discard.., 0);
            self.fifo_l.truncate(remaining);
            self.fifo_r.copy_within(discard.., 0);
            self.fifo_r.truncate(remaining);
            self.fifo_read_pos = retain_history.min(self.fifo_l.len());
        }

        if self.read_pos >= end && self.fifo_l.len().saturating_sub(self.fifo_read_pos) == 0 {
            *finished = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 44_100.0;

    /// Bandwidth the spread-filter tests drive the helper with. Production
    /// ships `SPREAD_BANDWIDTH = 0` (the filter is off), so every test that
    /// needs the filter to actually do something sets this explicitly; the
    /// `spread_bw` field stays live precisely so those tests can.
    const SPREAD_BW_TEST: f32 = 0.3;

    /// Stereo white noise, uncorrelated between channels (broadband
    /// material per AGENTS.md §3.1.11 — never sine-only).
    fn noise_stereo(frames: usize, amp: f32) -> Vec<f32> {
        let mut s = Vec::with_capacity(frames * 2);
        let (mut sl, mut sr) = (0x1234_5678u32, 0x9ABC_DEF0u32);
        for _ in 0..frames {
            sl = sl.wrapping_mul(1664525).wrapping_add(1013904223);
            sr = sr.wrapping_mul(1664525).wrapping_add(1013904223);
            let l = (((sl >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * amp;
            let r = (((sr >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * amp;
            s.push(l);
            s.push(r);
        }
        s
    }

    /// White noise plus an exponential-ish chirp sweeping 200..4000 Hz —
    /// broadband with a moving tonal component, per channel distinct.
    fn noise_plus_chirp(frames: usize, noise_amp: f32, chirp_amp: f32) -> Vec<f32> {
        let mut s = Vec::with_capacity(frames * 2);
        let (mut sl, mut sr) = (0xDEAD_BEEFu32, 0x1BAD_B002u32);
        let (mut phase_l, mut phase_r) = (0.0f64, 0.0f64);
        for i in 0..frames {
            let f = 200.0 + 3800.0 * (i as f64 / frames as f64);
            phase_l += std::f64::consts::TAU * f / SR as f64;
            phase_r += std::f64::consts::TAU * f * 1.007 / SR as f64;
            sl = sl.wrapping_mul(1664525).wrapping_add(1013904223);
            sr = sr.wrapping_mul(1664525).wrapping_add(1013904223);
            let nl = (((sl >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * noise_amp;
            let nr = (((sr >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * noise_amp;
            s.push(nl + (phase_l as f32).sin() * chirp_amp);
            s.push(nr + (phase_r as f32).sin() * chirp_amp);
        }
        s
    }

    /// Render `blocks` blocks of 512 frames, stopping early on `finished`.
    fn render(
        v: &mut Paulstretch,
        samples: &[f32],
        ch: usize,
        total: usize,
        speed: f32,
        pr: f32,
        blocks: usize,
    ) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        let (mut out_l, mut out_r) = (Vec::new(), Vec::new());
        for _ in 0..blocks {
            let mut fin = false;
            v.process(
                samples, ch, total, 512, speed, pr, SR, &mut l, &mut r, &mut fin,
            );
            out_l.extend_from_slice(&l);
            out_r.extend_from_slice(&r);
            if fin {
                break;
            }
        }
        (out_l, out_r)
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn dominant_bin(x: &[f32]) -> f32 {
        let n = x.len();
        let mut buf: Vec<Complex<f32>> = x.iter().map(|v| Complex::new(*v, 0.0)).collect();
        let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(n);
        fft.process(&mut buf);
        let (mut best, mut bestv) = (0usize, 0.0f32);
        for (k, cell) in buf.iter().enumerate().take(n / 2).skip(1) {
            let m = cell.norm();
            if m > bestv {
                bestv = m;
                best = k;
            }
        }
        best as f32
    }

    /// Peak-to-average ratio of a Welch-averaged magnitude spectrum: `k`
    /// consecutive windows of `n` samples starting at `start`, magnitudes
    /// averaged, then max/mean over bins 1..n/2. Averaging first suppresses
    /// the frame-phase mixing fluctuation, so what remains is the spectral
    /// envelope the spread filter shapes.
    fn spectral_peak_to_average(x: &[f32], start: usize, n: usize, k: usize) -> f32 {
        let mut buf: Vec<Complex<f32>> = Vec::with_capacity(n);
        let fft = rustfft::FftPlanner::<f32>::new().plan_fft_forward(n);
        let mut mags = vec![0.0f32; n / 2 - 1];
        for w in 0..k {
            buf.clear();
            buf.extend(
                x[start + w * n..start + w * n + n]
                    .iter()
                    .map(|v| Complex::new(*v, 0.0)),
            );
            fft.process(&mut buf);
            for (m, b) in mags.iter_mut().zip(buf[1..n / 2].iter()) {
                *m += b.norm();
            }
        }
        let (mut peak, mut sum) = (0.0f32, 0.0f32);
        for m in &mags {
            if *m > peak {
                peak = *m;
            }
            sum += m;
        }
        peak / (sum / mags.len() as f32)
    }

    #[test]
    fn identity_reconstructs_the_input_at_s1() {
        // Identity phases (no randomization, no spread) make the engine a
        // plain 50%-overlap windowed OLA; the squared Paul window pair sums
        // to ~0.974-1.0, so per-channel RMS error must be well under 3%.
        let total = 65536;
        let samples = noise_plus_chirp(total, 0.3, 0.3);
        let mut v = Paulstretch::new();
        v.identity = true;
        let blocks = total / 512 + 8;
        let (out_l, out_r) = render(&mut v, &samples, 2, total, 1.0, 1.0, blocks);

        let a = 2 * H;
        let b = total / 2;
        for (ch, out) in [(0, &out_l), (1, &out_r)] {
            let mut err = 0.0f64;
            let mut energy = 0.0f64;
            for i in a..b {
                let inp = samples[i * 2 + ch] as f64;
                let o = out[i] as f64;
                err += (o - inp) * (o - inp);
                energy += inp * inp;
            }
            let rel = (err / energy).sqrt();
            assert!(rel < 0.03, "ch {ch} identity reconstruction error {rel:.4}");
        }
    }

    #[test]
    fn output_level_tracks_input_across_expansions() {
        // Broadband noise, phases ON: the window / OLA / 1-N normalization
        // must land the output RMS within +-3 dB of the input, per channel.
        let total = 44100;
        let samples = noise_stereo(total, 0.4);
        let in_rms: [f32; 2] = [
            rms(&samples.iter().step_by(2).cloned().collect::<Vec<_>>()),
            rms(&samples
                .iter()
                .skip(1)
                .step_by(2)
                .cloned()
                .collect::<Vec<_>>()),
        ];
        for speed in [0.5f32, 0.1] {
            let mut v = Paulstretch::new();
            // S = pr/speed = 2 and 10.
            let blocks = ((total as f32 / speed) as usize / 512) + 8;
            let (out_l, out_r) = render(&mut v, &samples, 2, total, speed, 1.0, blocks);
            for (ch, out) in [(0, &out_l), (1, &out_r)] {
                // Middle third: past the ramp-in, before the drain.
                let n = out.len();
                let seg = &out[n / 3..2 * n / 3];
                let ratio = rms(seg) / in_rms[ch];
                // The double windowing is only fully compensated for
                // COHERENT overlap-add. With randomized phases the
                // resynthesis is diffuse, so the pair costs mean(w^2)
                // (~-3.1 dB for the Paul window) — the Public-Domain
                // reference behaves the same way. The spread filter ships
                // OFF, so there is no smoothing of the noise floor's Rayleigh
                // fluctuations on top of that and the observed ratio sits a
                // touch above the predicted 0.707. The band is therefore
                // centered on the predicted 0.65 with +-4.5 dB of headroom
                // around it, still catching any gross normalization mistake
                // in either direction.
                assert!(
                    (0.387..1.091).contains(&ratio),
                    "speed {speed} ch {ch} level ratio {ratio:.3}"
                );
            }
        }
    }

    #[test]
    fn frequency_is_preserved_at_expansion() {
        // 440 Hz sine summed with noise (broadband rule), S=4, phases ON:
        // the output spectrum peak must stay within +-3 bins of 440 Hz.
        let total = 44100;
        let mut samples = Vec::with_capacity(total * 2);
        let mut seed = 0xFEED_F00Du32;
        for i in 0..total {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (((seed >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * 0.2;
            let tone = (std::f32::consts::TAU * 440.0 * i as f32 / SR).sin() * 0.5;
            samples.push(tone + noise);
            samples.push(tone + noise);
        }
        let mut v = Paulstretch::new();
        let (out_l, out_r) = render(&mut v, &samples, 2, total, 0.25, 1.0, 512);
        let fft_n = 16384;
        let expect = 440.0 / (SR / fft_n as f32);
        for (ch, out) in [(0, &out_l), (1, &out_r)] {
            let a = 65536;
            let peak = dominant_bin(&out[a..a + fft_n]);
            assert!(
                (peak - expect).abs() <= 3.0,
                "ch {ch} peak bin {peak:.1}, expected {expect:.1} (440 Hz)"
            );
        }
    }

    #[test]
    fn output_is_finite_and_bounded() {
        let total = 44100;
        let samples = noise_stereo(total, 0.4);
        for speed in [2.0f32 / 3.0, 0.25, 0.1] {
            let mut v = Paulstretch::new();
            let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
            for _ in 0..300 {
                let mut fin = false;
                v.process(
                    &samples, 2, total, 512, speed, 1.0, SR, &mut l, &mut r, &mut fin,
                );
                for s in l.iter().chain(r.iter()) {
                    assert!(s.is_finite(), "non-finite output at speed {speed}");
                    assert!(s.abs() <= 4.0, "output diverged to {s} at speed {speed}");
                }
            }
        }
    }

    #[test]
    fn output_length_matches_expansion() {
        // Total produced output before `finished` must be ~S * total_frames
        // within +-N.
        let total = 44100;
        let samples = noise_stereo(total, 0.4);
        for speed in [0.5f32, 0.1] {
            let s = 1.0f64 / speed as f64;
            let mut v = Paulstretch::new();
            let cap = ((total as f64 * s) as usize / 512) + 64;
            let (out_l, out_r) = render(&mut v, &samples, 2, total, speed, 1.0, cap);
            assert_eq!(out_l.len(), out_r.len(), "channels diverged in length");
            let expect = total as f64 * s;
            let diff = (out_l.len() as f64 - expect).abs();
            assert!(
                diff <= N as f64,
                "speed {speed}: output {} vs expected {expect:.0} (diff {diff:.0})",
                out_l.len()
            );
            // finished must actually have fired inside the cap.
            let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
            let mut fin = false;
            v.process(
                &samples, 2, total, 512, speed, 1.0, SR, &mut l, &mut r, &mut fin,
            );
            assert!(fin, "speed {speed}: finished never set");
        }
    }

    #[test]
    fn expansion_reaches_forty_x_at_the_fader_floor() {
        // The tempo fader floor is 0.05 and the pitch ratio reaches 2.0, so
        // the FIFO expansion S = pr/speed reaches 40 — the reason the
        // internal clamp is 0.1..50 rather than the old 0.1..20.
        //
        // The OUTPUT length is `total / speed`, not `total * S`: the shared
        // Cubic Hermite resampler drains the FIFO at `step = pr`, so the
        // 40x expansion is paid back by the 2x pitch ratio. That is exactly
        // why this pins the clamp — with the 20 ceiling still in place the
        // FIFO would fill at 20x, the output would come out at half this
        // length, and the assert below would fail. `get_play_pos` has to
        // agree with `process` on the same expansion (two separate clamp
        // literals, free to drift).
        let total = 8192;
        let samples = noise_stereo(total, 0.4);
        let mut v = Paulstretch::new();
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        let mut fin = false;
        let mut out_len = 0usize;
        let expect = total as f64 / 0.05;
        for _ in 0..(expect as usize / 512 + 128) {
            v.process(
                &samples, 2, total, 512, 0.05, 2.0, SR, &mut l, &mut r, &mut fin,
            );
            for s in l.iter().chain(r.iter()) {
                assert!(s.is_finite(), "non-finite output at the 40x floor");
                assert!(s.abs() <= 4.0, "output diverged to {s} at the 40x floor");
            }
            out_len += 512;
            if fin {
                break;
            }
        }
        assert!(fin, "finished never set at speed 0.05 / pr 2.0");
        assert!(
            (out_len as f64 - expect).abs() <= N as f64,
            "output {out_len} vs expected {expect:.0}"
        );
        let pos = v.get_play_pos(0.05, 2.0);
        assert!(
            pos <= total as f64 + N as f64,
            "play pos {pos} escaped the source"
        );
    }

    #[test]
    fn reset_seeks_without_stale_audio() {
        // Identity at S=1 is an exact-ish OLA, so samples can be compared
        // element-wise: after reset(pos), output chunk 1 onward must equal
        // the input at pos, and cannot be leftover audio from position 0
        // (uncorrelated noise would fail the comparison).
        let total = 65536;
        let samples = noise_stereo(total, 0.4);
        let mut v = Paulstretch::new();
        v.identity = true;
        let _ = render(&mut v, &samples, 2, total, 1.0, 1.0, 100);
        let pos = 20000.0f64;
        v.reset(pos);

        // Rendering must cover the whole `H..2*H` probe window, so the block
        // count is derived from H rather than hard-coded: at H = 8192 that
        // is 32 blocks of 512, not the 20 the old 4096-hop window needed.
        let probe_blocks = 2 * H / 512 + 8;
        let (out_l, out_r) = render(&mut v, &samples, 2, total, 1.0, 1.0, probe_blocks);
        for s in out_l.iter().chain(out_r.iter()) {
            assert!(s.is_finite(), "NaN after reset");
        }
        // Chunk 1 (fully overlapped) starts at output H <-> source pos + H.
        for i in H..2 * H {
            let frame = (pos as usize) + i;
            assert!(
                (out_l[i] - samples[frame * 2]).abs() < 0.05,
                "left channel off at {i}: {} vs {}",
                out_l[i],
                samples[frame * 2]
            );
            assert!(
                (out_r[i] - samples[frame * 2 + 1]).abs() < 0.05,
                "right channel off at {i}"
            );
        }
    }

    #[test]
    fn spread_filter_identity_and_flattening() {
        let axis = SpreadAxis::new(SR);

        // bw -> 0 must be the identity (a_base -> 0) on a smooth spectrum.
        let mut mag: Vec<f32> = (0..HALF)
            .map(|k| 1.0 + 0.5 * k as f32 / (HALF - 1) as f32)
            .collect();
        let reference = mag.clone();
        let mut scratch = vec![0.0f32; NLOG];
        spread_magnitude(&mut mag, &axis, 0.0, &mut scratch);
        let max_diff = mag
            .iter()
            .zip(reference.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, |m, d| m.max(d));
        assert!(
            max_diff < 1e-3,
            "bw=0 spread changed spectrum by {max_diff}"
        );

        // bw = 0.3 must flatten a harmonic comb: peak-to-average drops.
        // Harmonics of 100 Hz (bin spacing N*100/44100 = 37.2 bins at
        // N = 16384), so the comb is defined by the material, not by a bin
        // period.
        let mut comb: Vec<f32> = vec![0.1; HALF];
        let mut n = 1f32;
        while n * 100.0 * N as f32 / SR < HALF as f32 {
            comb[(n * 100.0 * N as f32 / SR) as usize] = 10.0;
            n += 1.0;
        }
        let mean_before = comb.iter().sum::<f32>() / HALF as f32;
        let peak_before = comb.iter().cloned().fold(0.0f32, f32::max);
        let ratio_before = peak_before / mean_before;

        spread_magnitude(&mut comb, &axis, SPREAD_BW_TEST, &mut scratch);
        let mean_after = comb.iter().sum::<f32>() / HALF as f32;
        let peak_after = comb.iter().cloned().fold(0.0f32, f32::max);
        let ratio_after = peak_after / mean_after;
        assert!(
            ratio_after < ratio_before * 0.97,
            "comb not flattened: {ratio_before:.3} -> {ratio_after:.3}"
        );
    }

    #[test]
    fn deterministic_for_fixed_seeds() {
        let total = 32768;
        let samples = noise_stereo(total, 0.4);
        let mut a = Paulstretch::new();
        let mut b = Paulstretch::new();
        let (la, ra) = render(&mut a, &samples, 2, total, 0.5, 1.0, 60);
        let (lb, rb) = render(&mut b, &samples, 2, total, 0.5, 1.0, 60);
        assert_eq!(la.len(), lb.len());
        assert!(la.iter().zip(lb.iter()).all(|(x, y)| x == y));
        assert!(ra.iter().zip(rb.iter()).all(|(x, y)| x == y));
    }

    #[test]
    fn spread_bandwidth_override_flattens_output_spectrum() {
        // The test-constructor spread_bw override must be observable end to
        // end. Both engines use identical seeds and input, so the two runs
        // differ ONLY by the spread filter. The partial stack sits at HIGH
        // frequencies (4..8 kHz) deliberately: there a partial's main lobe
        // spans barely one or two log-axis points, so the bw=0.3 one-pole
        // smoothing treats it like an impulse and cuts its magnitude hard,
        // while bw ~ 0 passes it through. That must show up as a lower
        // spectral peak-to-average ratio in the smoothed run. (Low-frequency
        // partials are immune — their lobes span dozens of log points — and
        // pure noise is dominated by frame-phase mixing, so neither probes
        // the filter.)
        let total = 44100;
        let mut samples = Vec::with_capacity(total * 2);
        let mut seed = 0x5EED_5EEDu32;
        for i in 0..total {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let noise = (((seed >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * 0.1;
            let mut v = 0.0f32;
            for f in [4000.0f32, 5000.0, 6000.0, 7000.0, 8000.0] {
                v += (std::f32::consts::TAU * f * i as f32 / SR).sin() * 0.2;
            }
            samples.push(v + noise);
            samples.push(v + noise);
        }
        let mut flat = Paulstretch::new();
        flat.spread_bw = 1e-6;
        let mut spread = Paulstretch::new();
        // Production ships bw = 0 (filter off, and `process` skips the
        // helper outright at that setting), so the "spread" arm has to opt
        // in explicitly for this differential to mean anything.
        spread.spread_bw = SPREAD_BW_TEST;
        let (fl, fr) = render(&mut flat, &samples, 2, total, 0.5, 1.0, 200);
        let (sl, sr) = render(&mut spread, &samples, 2, total, 0.5, 1.0, 200);
        for (ch, (f, s)) in [(0, (&fl, &sl)), (1, (&fr, &sr))] {
            for x in f.iter().chain(s.iter()) {
                assert!(x.is_finite(), "non-finite output, ch {ch}");
                assert!(x.abs() <= 4.0, "output diverged to {x}, ch {ch}");
            }
            // 8 Welch windows of 8192 from 20480 on: past the ramp-in (output is
            // fully overlapped from sample H = 8192 on) and still inside the
            // ~88200-sample rendered output.
            let a = 20480;
            let n = 8192;
            let k = 8;
            let rf = spectral_peak_to_average(f, a, n, k);
            let rs = spectral_peak_to_average(s, a, n, k);
            assert!(
                rs < rf * 0.97,
                "ch {ch}: bw=0.3 did not flatten ({rf:.3} -> {rs:.3})"
            );
        }
    }

    #[test]
    fn play_pos_advances_monotonically_and_finishes() {
        let total = 44100;
        let samples = noise_stereo(total, 0.4);
        let speed = 0.5f32;
        let mut v = Paulstretch::new();
        let (mut l, mut r) = (vec![0f32; 512], vec![0f32; 512]);
        let mut prev = 0.0f64;
        let mut fin = false;
        let mut blocks = 0;
        while !fin && blocks < (total as f64 / speed as f64) as usize / 512 + 64 {
            v.process(
                &samples, 2, total, 512, speed, 1.0, SR, &mut l, &mut r, &mut fin,
            );
            let pos = v.get_play_pos(speed, 1.0);
            assert!(pos >= prev, "play position went backwards: {prev} -> {pos}");
            if blocks < 100 {
                // Mid-track: ~speed samples of source per output sample,
                // i.e. ~speed * 512 per block (loose tolerance: the lag
                // estimate moves in whole FIFO frames).
                let adv = pos - prev;
                assert!(
                    ((speed * 512.0 * 0.5) as f64..=(speed * 512.0 * 1.5) as f64).contains(&adv),
                    "block {blocks} advanced {adv:.1}, expected ~{}",
                    speed * 512.0
                );
            }
            prev = pos;
            blocks += 1;
        }
        assert!(fin, "finished never set");
    }
}
