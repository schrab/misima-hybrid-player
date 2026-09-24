//! 10-band peaking EQ using RBJ biquad coefficients.

#[derive(Debug, Clone)]
pub struct Biquad {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self::identity()
    }
}

impl Biquad {
    pub fn identity() -> Self {
        Self {
            b0: 1.0,
            b1: 0.0,
            b2: 0.0,
            a1: 0.0,
            a2: 0.0,
        }
    }

    /// Peaking EQ, gain_db, freq Hz, Q.
    pub fn peaking(sample_rate: f32, freq: f32, q: f32, gain_db: f32) -> Self {
        if gain_db.abs() < 0.01 {
            return Self::identity();
        }
        let a = 10f32.powf(gain_db / 40.0);
        let w0 = 2.0 * std::f32::consts::PI * (freq / sample_rate).clamp(1e-4, 0.49);
        let alpha = w0.sin() / (2.0 * q);
        let cos_w0 = w0.cos();
        let b0 = 1.0 + alpha * a;
        let b1 = -2.0 * cos_w0;
        let b2 = 1.0 - alpha * a;
        let a0 = 1.0 + alpha / a;
        let a1 = -2.0 * cos_w0;
        let a2 = 1.0 - alpha / a;
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EqState {
    pub bands: [Biquad; 10],
    // per-band delay (stereo)
    z1: [f32; 10],
    z2: [f32; 10],
    z1r: [f32; 10],
    z2r: [f32; 10],
}

pub const EQ_FREQS: [f32; 10] = [
    60.0, 170.0, 310.0, 600.0, 1000.0, 3000.0, 6000.0, 12000.0, 14000.0, 16000.0,
];

impl Default for EqState {
    fn default() -> Self {
        Self::new(44_100.0, &[0.0; 10])
    }
}

impl EqState {
    pub fn new(sample_rate: f32, gains_db: &[f32; 10]) -> Self {
        let mut bands = core::array::from_fn(|_| Biquad::identity());
        for (i, freq) in EQ_FREQS.iter().enumerate() {
            let q = if *freq < 100.0 {
                0.7
            } else if *freq > 10_000.0 {
                0.9
            } else {
                1.0
            };
            bands[i] = Biquad::peaking(sample_rate, *freq, q, gains_db[i]);
        }
        Self {
            bands,
            z1: [0.0; 10],
            z2: [0.0; 10],
            z1r: [0.0; 10],
            z2r: [0.0; 10],
        }
    }

    pub fn set_gains(&mut self, sample_rate: f32, gains_db: &[f32; 10]) {
        for (i, freq) in EQ_FREQS.iter().enumerate() {
            let q = if *freq < 100.0 {
                0.7
            } else if *freq > 10_000.0 {
                0.9
            } else {
                1.0
            };
            self.bands[i] = Biquad::peaking(sample_rate, *freq, q, gains_db[i]);
        }
    }

    #[inline]
    fn tick(b: &Biquad, x: f32, z1: &mut f32, z2: &mut f32) -> f32 {
        let y = b.b0 * x + *z1;
        *z1 = b.b1 * x - b.a1 * y + *z2;
        *z2 = b.b2 * x - b.a2 * y;
        y
    }

    /// Process interleaved stereo in place (or mono pairs).
    pub fn process_interleaved(&mut self, data: &mut [f32], channels: usize) {
        if channels == 0 {
            return;
        }
        for frame in data.chunks_mut(channels) {
            if frame.is_empty() {
                continue;
            }
            let mut x = frame[0];
            for i in 0..10 {
                x = Self::tick(&self.bands[i], x, &mut self.z1[i], &mut self.z2[i]);
            }
            frame[0] = x;
            if channels >= 2 {
                let mut xr = frame[1];
                for i in 0..10 {
                    xr = Self::tick(&self.bands[i], xr, &mut self.z1r[i], &mut self.z2r[i]);
                }
                frame[1] = xr;
            }
        }
    }
}

/// Standard Schroeder allpass: y = -g*x + d;  d' = x + g*y
#[inline]
pub fn allpass_tick(buf: &mut [f32], idx: &mut usize, g: f32, x: f32) -> f32 {
    let bufout = buf[*idx];
    let y = -g * x + bufout;
    buf[*idx] = x + g * y;
    *idx = (*idx + 1) % buf.len();
    y
}

/// RMS energy helper used by tests.
#[allow(dead_code)]
pub fn rms(data: &[f32]) -> f32 {
    if data.is_empty() {
        return 0.0;
    }
    let sum: f32 = data.iter().map(|x| x * x).sum();
    (sum / data.len() as f32).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
            .collect()
    }

    #[test]
    fn allpass_pulse_energy() {
        let mut buf = vec![0.0f32; 64];
        let mut idx = 0usize;
        let g = 0.5f32;
        let mut peak = 0.0f32;
        for i in 0..200 {
            let x = if i < 10 { 1.0 } else { 0.0 };
            let y = allpass_tick(&mut buf, &mut idx, g, x);
            peak = peak.max(y.abs());
        }
        assert!(peak > 0.5 && peak < 1.5, "peak={peak}");
    }

    #[test]
    fn identity_when_zero_gain() {
        let b = Biquad::peaking(44100.0, 1000.0, 1.0, 0.0);
        assert_eq!(b.b0, 1.0);
        assert_eq!(b.b1, 0.0);
    }

    #[test]
    fn boost_increases_energy_near_band() {
        let sr = 44100.0;
        let mut sig = sine(1000.0, sr, 4410);
        // stereo interleave
        let mut stereo = Vec::with_capacity(sig.len() * 2);
        for s in &sig {
            stereo.push(*s);
            stereo.push(*s);
        }
        let before = rms(&stereo);
        let mut eq = EqState::new(sr, &[0.0, 0.0, 0.0, 0.0, 12.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        eq.process_interleaved(&mut stereo, 2);
        let after = rms(&stereo);
        assert!(after > before * 1.2, "before={before} after={after}");

        // cut
        sig = sine(1000.0, sr, 4410);
        let mut stereo2 = Vec::with_capacity(sig.len() * 2);
        for s in &sig {
            stereo2.push(*s);
            stereo2.push(*s);
        }
        let before2 = rms(&stereo2);
        let mut eq2 = EqState::new(sr, &[0.0, 0.0, 0.0, 0.0, -12.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        eq2.process_interleaved(&mut stereo2, 2);
        let after2 = rms(&stereo2);
        assert!(after2 < before2 * 0.8, "before={before2} after={after2}");
    }
}
