//! Log-spaced FFT spectrum bins for the visualizer.

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

pub struct SpectrumAnalyzer {
    fft_size: usize,
    bins_out: usize,
    window: Vec<f32>,
    scratch: Vec<Complex<f32>>,
    planner: FftPlanner<f32>,
    magnitudes: Vec<f32>,
}

impl SpectrumAnalyzer {
    pub fn new(fft_size: usize, bins_out: usize) -> Self {
        assert!(fft_size.is_power_of_two());
        let window: Vec<f32> = (0..fft_size)
            .map(|i| {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (fft_size as f32 - 1.0)).cos()
            })
            .collect();
        Self {
            fft_size,
            bins_out,
            window,
            scratch: vec![Complex::new(0.0, 0.0); fft_size],
            planner: FftPlanner::new(),
            magnitudes: vec![0.0; bins_out],
        }
    }

    /// Analyze mono samples (length may be shorter; pads with zeros).
    pub fn analyze(&mut self, mono: &[f32]) -> &[f32] {
        let n = self.fft_size;
        for i in 0..n {
            let s = mono.get(i).copied().unwrap_or(0.0);
            self.scratch[i] = Complex::new(s * self.window[i], 0.0);
        }
        let fft = self.planner.plan_fft_forward(n);
        fft.process(&mut self.scratch);

        // half spectrum
        let half = n / 2;
        let out = self.bins_out;
        for b in 0..out {
            // log-spaced from bin 1 .. half
            let f0 = 1.0 + (half as f32 - 1.0) * ((b as f32) / out as f32).powf(1.6);
            let f1 = 1.0 + (half as f32 - 1.0) * (((b + 1) as f32) / out as f32).powf(1.6);
            let i0 = (f0 as usize).clamp(1, half.saturating_sub(1));
            let i1 = ((f1 as usize).max(i0 + 1)).min(half);
            let mut acc = 0.0f32;
            let mut cnt = 0u32;
            for i in i0..i1 {
                let c = self.scratch[i];
                acc += (c.re * c.re + c.im * c.im).sqrt();
                cnt += 1;
            }
            let mag = if cnt > 0 { acc / cnt as f32 } else { 0.0 };
            // compress to 0..1-ish
            self.magnitudes[b] = (mag * 0.05).min(1.0);
        }
        &self.magnitudes
    }

    #[allow(dead_code)]
    pub fn bins(&self) -> &[f32] {
        &self.magnitudes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sine_produces_energy() {
        let sr = 44100.0;
        let n = 2048;
        let freq = 1000.0;
        let samples: Vec<f32> = (0..n)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin())
            .collect();
        let mut sa = SpectrumAnalyzer::new(2048, 32);
        let bins = sa.analyze(&samples).to_vec();
        let total: f32 = bins.iter().sum();
        assert!(total > 0.1, "spectrum total energy {total}");
        assert!(bins.iter().any(|b| *b > 0.0));
    }

    #[test]
    fn silence_is_zero() {
        let mut sa = SpectrumAnalyzer::new(1024, 16);
        let bins = sa.analyze(&vec![0.0; 1024]).to_vec();
        assert!(bins.iter().all(|b| *b < 1e-6));
    }
}
