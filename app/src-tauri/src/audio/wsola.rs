//! High-quality real-time WSOLA (Waveform Similarity Overlap-Add) time-stretching
//! and Cubic Hermite resampling pitch-shifting engine.
//!
//! Features:
//! - Waveform similarity cross-correlation phase alignment (eliminates comb-filtering and metallic flutter)
//! - 50% overlap with periodic Hann window (guarantees exact 1.0 unity gain across all output samples)
//! - Unified stereo phase alignment (correlation on mono mix, same grain shift for L and R to preserve stereo imaging)
//! - 4-point Cubic Hermite (Catmull-Rom) interpolation for smooth, alias-free pitch-shifting
//! - Dynamic 2-pole lowpass filter to prevent aliasing during upward pitch shifts
//! - Zero steady-state heap allocations in hot audio processing loop

use crate::audio::eq::Biquad;

pub const WSOLA_WIN: usize = 1024;
pub const WSOLA_HOP: usize = 512;
pub const WSOLA_TEMP: usize = 256;
pub const WSOLA_DELTA: usize = 256;

#[derive(Debug, Clone)]
pub struct WsolaProcessor {
    window: Vec<f32>,
    tail_l: Vec<f32>,
    tail_r: Vec<f32>,
    fifo_l: Vec<f32>,
    fifo_r: Vec<f32>,
    fifo_read_pos: usize,
    last_tau: isize,
    nominal_pos: f64,
    initialized: bool,
    resample_phase: f64,
    lp_filter: Option<Biquad>,
    lp_z1_l: f32,
    lp_z2_l: f32,
    lp_z1_r: f32,
    lp_z2_r: f32,
    last_filter_ratio: f32,
}

impl Default for WsolaProcessor {
    fn default() -> Self {
        Self::new()
    }
}

impl WsolaProcessor {
    pub fn new() -> Self {
        // Periodic Hann window: w[i] = 0.5 * (1 - cos(2*pi*i / N))
        // Exactly sums to 1.0 with 50% overlap (w[i] + w[i + N/2] == 1.0)
        let window: Vec<f32> = (0..WSOLA_WIN)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / WSOLA_WIN as f32).cos()))
            .collect();

        Self {
            window,
            tail_l: vec![0.0; WSOLA_HOP],
            tail_r: vec![0.0; WSOLA_HOP],
            fifo_l: Vec::with_capacity(8192),
            fifo_r: Vec::with_capacity(8192),
            fifo_read_pos: 0,
            last_tau: 0,
            nominal_pos: 0.0,
            initialized: false,
            resample_phase: 0.0,
            lp_filter: None,
            lp_z1_l: 0.0,
            lp_z2_l: 0.0,
            lp_z1_r: 0.0,
            lp_z2_r: 0.0,
            last_filter_ratio: 1.0,
        }
    }

    /// Reset internal state and seek to the given source frame position.
    pub fn reset(&mut self, pos: f64) {
        self.tail_l.fill(0.0);
        self.tail_r.fill(0.0);
        self.fifo_l.clear();
        self.fifo_r.clear();
        self.fifo_read_pos = 0;
        self.last_tau = pos as isize;
        self.nominal_pos = pos;
        self.initialized = false;
        self.resample_phase = 0.0;
        self.lp_z1_l = 0.0;
        self.lp_z2_l = 0.0;
        self.lp_z1_r = 0.0;
        self.lp_z2_r = 0.0;
    }

    /// Returns true if WSOLA has active buffered audio or grains produced.
    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        self.initialized && (self.fifo_l.len() > self.fifo_read_pos)
    }

    /// Calculate the actual source playback frame currently being heard,
    /// compensating for buffered samples in the output FIFO and time-stretch ratio.
    pub fn get_play_pos(&self, speed: f32, pitch_ratio: f32) -> f64 {
        let pr = pitch_ratio.clamp(0.25, 4.0) as f64;
        let stretch = (speed as f64 / pr).clamp(0.1, 10.0);
        let avail_fifo = (self.fifo_l.len().saturating_sub(self.fifo_read_pos) as f64) - self.resample_phase;
        let lag_in_source_frames = avail_fifo.max(0.0) * stretch;
        (self.nominal_pos - lag_in_source_frames).max(0.0)
    }

    /// Produce a single WSOLA grain (HOP_SIZE = 512 samples) into the FIFO.
    fn produce_grain(
        &mut self,
        samples: &[f32],
        ch: usize,
        total_frames: usize,
        stretch_factor: f64,
    ) {
        if total_frames == 0 {
            return;
        }

        if !self.initialized {
            let start = (self.nominal_pos as isize).max(0);
            self.last_tau = start;
            // Pre-fill the overlap tail with the second half of the initial grain
            for i in 0..WSOLA_HOP {
                let fi = start + WSOLA_HOP as isize + i as isize;
                let (sl, sr) = get_stereo_frame(samples, ch, total_frames, fi);
                let w = self.window[WSOLA_HOP + i];
                self.tail_l[i] = sl * w;
                self.tail_r[i] = sr * w;
            }
            // Smoothly fade in the first half of the initial grain
            for i in 0..WSOLA_HOP {
                let fi = start + i as isize;
                let (sl, sr) = get_stereo_frame(samples, ch, total_frames, fi);
                let w = self.window[i];
                self.fifo_l.push(sl * w);
                self.fifo_r.push(sr * w);
            }
            self.nominal_pos += (WSOLA_HOP as f64) * stretch_factor;
            self.initialized = true;
            return;
        }

        // Natural continuation template position: where the previous grain would continue naturally
        let tau_template = self.last_tau + WSOLA_HOP as isize;

        // Extract mono template for correlation
        let mut template = [0.0f32; WSOLA_TEMP];
        let mut template_energy = 0.0f32;
        for j in 0..WSOLA_TEMP {
            let s = get_mono_frame(samples, ch, total_frames, tau_template + j as isize);
            template[j] = s;
            template_energy += s * s;
        }

        let l_nominal = self.nominal_pos.round() as isize;

        let best_delta = if template_energy < 1e-6 {
            // Signal is near-silent; default to nominal position
            0isize
        } else {
            let max_delta = WSOLA_DELTA as isize;
            let mut best_delta = 0isize;
            let mut best_dot = 0.0f32;
            let mut best_energy = 1.0f32;
            let mut found_positive = false;

            // Coarse search: step by 2 samples
            let mut d = -max_delta;
            while d <= max_delta {
                let cand_start = l_nominal + d;
                let mut dot = 0.0f32;
                let mut cand_energy = 0.0f32;
                for j in 0..WSOLA_TEMP {
                    let c = get_mono_frame(samples, ch, total_frames, cand_start + j as isize);
                    dot += template[j] * c;
                    cand_energy += c * c;
                }
                if dot > 0.0 && cand_energy > 1e-6 {
                    // Compare dot^2 / cand_energy > best_dot^2 / best_energy without division
                    if !found_positive || (dot * dot * best_energy > best_dot * best_dot * cand_energy) {
                        found_positive = true;
                        best_dot = dot;
                        best_energy = cand_energy;
                        best_delta = d;
                    }
                }
                d += 2;
            }

            // Fine refinement: test best_delta - 1 and best_delta + 1
            for step in [-1isize, 1isize] {
                let cand_d = best_delta + step;
                if cand_d >= -max_delta && cand_d <= max_delta {
                    let cand_start = l_nominal + cand_d;
                    let mut dot = 0.0f32;
                    let mut cand_energy = 0.0f32;
                    for j in 0..WSOLA_TEMP {
                        let c = get_mono_frame(samples, ch, total_frames, cand_start + j as isize);
                        dot += template[j] * c;
                        cand_energy += c * c;
                    }
                    if dot > 0.0 && cand_energy > 1e-6 {
                        if !found_positive || (dot * dot * best_energy > best_dot * best_dot * cand_energy) {
                            found_positive = true;
                            best_dot = dot;
                            best_energy = cand_energy;
                            best_delta = cand_d;
                        }
                    }
                }
            }

            best_delta
        };

        let tau_k = (l_nominal + best_delta).clamp(0, total_frames.saturating_sub(WSOLA_WIN) as isize);

        // Overlap-add the first half of the grain with the previous tail
        for i in 0..WSOLA_HOP {
            let fi = tau_k + i as isize;
            let (sl, sr) = get_stereo_frame(samples, ch, total_frames, fi);
            let w = self.window[i];
            self.fifo_l.push(self.tail_l[i] + sl * w);
            self.fifo_r.push(self.tail_r[i] + sr * w);
        }

        // Store the second half of the grain as the new overlap tail
        for i in 0..WSOLA_HOP {
            let fi = tau_k + WSOLA_HOP as isize + i as isize;
            let (sl, sr) = get_stereo_frame(samples, ch, total_frames, fi);
            let w = self.window[WSOLA_HOP + i];
            self.tail_l[i] = sl * w;
            self.tail_r[i] = sr * w;
        }

        self.last_tau = tau_k;
        self.nominal_pos += (WSOLA_HOP as f64) * stretch_factor;
    }

    /// Process a block of audio frames with WSOLA time-stretching and pitch-shifting.
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

        let pr = pitch_ratio.clamp(0.25, 4.0);
        let stretch = (speed / pr).clamp(0.1, 10.0) as f64;
        let pitch_shifting = (pr - 1.0).abs() >= 0.002;

        // Dynamic 2-pole Butterworth lowpass filter to prevent aliasing when pitching up
        if pr > 1.05 {
            if self.lp_filter.is_none() || (self.last_filter_ratio - pr).abs() > 0.02 {
                let cutoff = (0.45 * sample_rate / pr).clamp(100.0, sample_rate * 0.49);
                self.lp_filter = Some(Biquad::lowpass(sample_rate, cutoff, 0.707));
                self.last_filter_ratio = pr;
            }
        } else {
            self.lp_filter = None;
        }

        // Calculate needed FIFO length to fulfill out_frames
        let needed_fifo = if pitch_shifting {
            ((out_frames as f64) * (pr as f64) + self.resample_phase).ceil() as usize + 8
        } else {
            out_frames
        };

        let max_grains = 32;
        let mut grain_count = 0;
        let fifo_start_len = self.fifo_l.len();

        while (self.fifo_l.len() - self.fifo_read_pos) < needed_fifo
            && self.nominal_pos < (total_frames + WSOLA_WIN) as f64
            && grain_count < max_grains
        {
            self.produce_grain(samples, ch, total_frames, stretch);
            grain_count += 1;
        }

        // Apply anti-aliasing filter to newly produced grain samples
        if let Some(lp) = &self.lp_filter {
            let new_start = fifo_start_len.max(self.fifo_read_pos);
            for i in new_start..self.fifo_l.len() {
                self.fifo_l[i] = lp.tick(self.fifo_l[i], &mut self.lp_z1_l, &mut self.lp_z2_l);
                self.fifo_r[i] = lp.tick(self.fifo_r[i], &mut self.lp_z1_r, &mut self.lp_z2_r);
            }
        }

        if !pitch_shifting {
            // Direct FIFO read (tempo stretch only, no pitch shift)
            let avail = self.fifo_l.len() - self.fifo_read_pos;
            let to_copy = out_frames.min(avail);
            let read_start = self.fifo_read_pos;
            out_l[..to_copy].copy_from_slice(&self.fifo_l[read_start..read_start + to_copy]);
            out_r[..to_copy].copy_from_slice(&self.fifo_r[read_start..read_start + to_copy]);
            if to_copy < out_frames {
                out_l[to_copy..out_frames].fill(0.0);
                out_r[to_copy..out_frames].fill(0.0);
            }
            self.fifo_read_pos += to_copy;
        } else {
            // 4-point Cubic Hermite (Catmull-Rom) interpolation for pitch shifting
            let step = pr as f64;
            let fifo_len = self.fifo_l.len();
            for m in 0..out_frames {
                let pos = (self.fifo_read_pos as f64) + self.resample_phase + (m as f64) * step;
                let k = pos.floor() as usize;
                let t = (pos - k as f64) as f32;

                if k + 2 < fifo_len {
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
                    // Linear fallback near tail edge
                    let s0_l = self.fifo_l[k];
                    let s1_l = if k + 1 < fifo_len { self.fifo_l[k + 1] } else { s0_l };
                    out_l[m] = s0_l + (s1_l - s0_l) * t;

                    let s0_r = self.fifo_r[k];
                    let s1_r = if k + 1 < fifo_len { self.fifo_r[k + 1] } else { s0_r };
                    out_r[m] = s0_r + (s1_r - s0_r) * t;
                } else {
                    out_l[m] = 0.0;
                    out_r[m] = 0.0;
                }
            }

            let final_pos = (self.fifo_read_pos as f64) + self.resample_phase + (out_frames as f64) * step;
            let consumed = final_pos.floor() as usize;
            self.resample_phase = final_pos - (consumed as f64);
            self.fifo_read_pos = consumed;
        }

        // Reclaim consumed FIFO space while retaining 2 samples of history for cubic interpolation
        let retain_history = 2usize;
        if self.fifo_read_pos > retain_history + 256 {
            let discard = self.fifo_read_pos - retain_history;
            let remaining = self.fifo_l.len() - discard;
            self.fifo_l.copy_within(discard.., 0);
            self.fifo_l.truncate(remaining);
            self.fifo_r.copy_within(discard.., 0);
            self.fifo_r.truncate(remaining);
            self.fifo_read_pos = retain_history;
        }

        if self.nominal_pos >= total_frames as f64 && (self.fifo_l.len() - self.fifo_read_pos) == 0 {
            *finished = true;
        }
    }
}

/// 4-point Cubic Hermite (Catmull-Rom) interpolation.
/// Exact at t=0 and t=1, C^1 smooth continuous derivative across intervals.
#[inline(always)]
fn cubic_hermite(y_m1: f32, y0: f32, y1: f32, y2: f32, t: f32) -> f32 {
    let c0 = y0;
    let c1 = 0.5 * (y1 - y_m1);
    let c2 = y_m1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - y_m1) + 1.5 * (y0 - y1);
    ((c3 * t + c2) * t + c1) * t + c0
}

#[inline(always)]
fn get_mono_frame(samples: &[f32], ch: usize, total_frames: usize, f: isize) -> f32 {
    if f < 0 || f as usize >= total_frames {
        0.0
    } else {
        let idx = (f as usize) * ch;
        if ch > 1 {
            (samples[idx] + samples[idx + 1]) * 0.5
        } else {
            samples[idx]
        }
    }
}

#[inline(always)]
fn get_stereo_frame(samples: &[f32], ch: usize, total_frames: usize, f: isize) -> (f32, f32) {
    if f < 0 || f as usize >= total_frames {
        (0.0, 0.0)
    } else {
        let idx = (f as usize) * ch;
        let l = samples[idx];
        let r = if ch > 1 { samples[idx + 1] } else { l };
        (l, r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hann_window_perfect_reconstruction() {
        let wsola = WsolaProcessor::new();
        // For 50% overlap, window[i] + window[i + HOP] must equal 1.0 for all i
        for i in 0..WSOLA_HOP {
            let sum = wsola.window[i] + wsola.window[i + WSOLA_HOP];
            assert!((sum - 1.0).abs() < 1e-6, "Hann window failed at {i}: {sum}");
        }
    }

    #[test]
    fn test_cubic_hermite_grid_points() {
        let y_m1 = 0.2f32;
        let y0 = 0.5f32;
        let y1 = 0.8f32;
        let y2 = 0.3f32;

        let v0 = cubic_hermite(y_m1, y0, y1, y2, 0.0);
        let v1 = cubic_hermite(y_m1, y0, y1, y2, 1.0);

        assert!((v0 - y0).abs() < 1e-6, "Expected {y0}, got {v0}");
        assert!((v1 - y1).abs() < 1e-6, "Expected {y1}, got {v1}");

        let mid = cubic_hermite(y_m1, y0, y1, y2, 0.5);
        assert!(mid.is_finite());
    }

    #[test]
    fn test_wsola_stretch_sine() {
        // Generate a 1-second 440 Hz stereo sine wave at 44.1 kHz
        let sr = 44100.0f32;
        let num_frames = 44100;
        let mut samples = Vec::with_capacity(num_frames * 2);
        for i in 0..num_frames {
            let s = (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / sr).sin();
            samples.push(s);
            samples.push(s);
        }

        let mut wsola = WsolaProcessor::new();
        let block_size = 512;
        let mut out_l = vec![0.0f32; block_size];
        let mut out_r = vec![0.0f32; block_size];
        let mut finished = false;

        // Process 10 blocks at 1.25x speed
        for _ in 0..10 {
            wsola.process(
                &samples,
                2,
                num_frames,
                block_size,
                1.25,
                1.0,
                sr,
                &mut out_l,
                &mut out_r,
                &mut finished,
            );
            for s in out_l.iter().chain(out_r.iter()) {
                assert!(s.is_finite());
                assert!(s.abs() <= 1.2, "Signal clipped or blew up: {s}");
            }
        }
        assert!(!finished);
    }

    #[test]
    fn test_wsola_pitch_shift_stability() {
        let sr = 44100.0f32;
        let num_frames = 44100;
        let mut samples = Vec::with_capacity(num_frames * 2);
        for i in 0..num_frames {
            let s = (2.0 * std::f32::consts::PI * 440.0 * (i as f32) / sr).sin();
            samples.push(s);
            samples.push(s);
        }

        let mut wsola = WsolaProcessor::new();
        let block_size = 512;
        let mut out_l = vec![0.0f32; block_size];
        let mut out_r = vec![0.0f32; block_size];
        let mut finished = false;

        // Process blocks at +5 semitones (ratio ~ 1.3348)
        let pr = 2.0f32.powf(5.0 / 12.0);
        for _ in 0..10 {
            wsola.process(
                &samples,
                2,
                num_frames,
                block_size,
                1.0,
                pr,
                sr,
                &mut out_l,
                &mut out_r,
                &mut finished,
            );
            for s in out_l.iter().chain(out_r.iter()) {
                assert!(s.is_finite());
                assert!(s.abs() <= 1.2, "Signal exploded: {s}");
            }
        }
        assert!(!finished);
    }

    #[test]
    fn test_wsola_seek_reset() {
        let mut wsola = WsolaProcessor::new();
        wsola.reset(10000.0);
        assert_eq!(wsola.nominal_pos, 10000.0);
        assert_eq!(wsola.last_tau, 10000);
        assert!(!wsola.initialized);
        assert_eq!(wsola.fifo_l.len(), 0);
    }
}
