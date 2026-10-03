//! Engine selector between the WSOLA and phase-vocoder time-stretchers.
//!
//! Platform-free: depends only on the two stretch engines, so both
//! `src-tauri` and the web `wasm-dsp` crate can `#[path]`-include it.

use crate::audio::phase_vocoder::PhaseVocoder;
use crate::audio::wsola::WsolaProcessor;

/// Time-stretching engine for the DSP path.
///
/// The split follows the synthesis hop. A phase vocoder reconstructs exactly
/// only while its analysis hop stays at or under a quarter of the FFT window —
/// that is where the Hann window's squares still sum to a flat 1.5. Measured:
/// level is accurate to ~0.3 dB at stretch 1.0, drifts to +2.3 dB by stretch
/// 2.0, and blows up completely at stretch 4.0 where consecutive frames stop
/// overlapping at all. So the vocoder is used for `stretch <= 1`, which is
/// exactly the region where the WSOLA is granular — it time-compresses by
/// `speed / pitch`, so pitch-up means compression and 4x grain overlap at
/// +1 octave. For `stretch > 1` the WSOLA is in its good expansion
/// direction and sounds fine there.
///
/// Both engines are constructed up front — swapping one for the other must
/// never allocate on the real-time thread.
pub struct Stretcher {
    wsola: WsolaProcessor,
    vocoder: PhaseVocoder,
    using_vocoder: bool,
}

impl Default for Stretcher {
    fn default() -> Self {
        Self::new()
    }
}

impl Stretcher {
    pub fn new() -> Self {
        Self {
            wsola: WsolaProcessor::new(),
            vocoder: PhaseVocoder::new(),
            using_vocoder: false,
        }
    }

    /// Choose the engine for these parameters, re-seeding it at `pos` when
    /// the choice changes so a fader crossing never resumes from stale state.
    pub fn select(&mut self, speed: f32, pr: f32, pos: f64) {
        let stretch = speed / pr;
        let want = (pr - 1.0).abs() >= 0.002 && stretch <= 1.0;
        if want == self.using_vocoder {
            return;
        }
        self.using_vocoder = want;
        self.reset(pos);
    }

    pub fn reset(&mut self, pos: f64) {
        self.wsola.reset(pos);
        self.vocoder.reset(pos);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn process(
        &mut self,
        samples: &[f32],
        ch: usize,
        total_frames: usize,
        frames: usize,
        speed: f32,
        pr: f32,
        sample_rate: f32,
        bl: &mut [f32],
        br: &mut [f32],
        finished: &mut bool,
    ) {
        if self.using_vocoder {
            self.vocoder
                .process(samples, ch, total_frames, frames, speed, pr, bl, br, finished);
        } else {
            self.wsola.process(
                samples, ch, total_frames, frames, speed, pr, sample_rate, bl, br, finished,
            );
        }
    }

    pub fn get_play_pos(&self, speed: f32, pr: f32) -> f64 {
        if self.using_vocoder {
            self.vocoder.get_play_pos(speed, pr)
        } else {
            self.wsola.get_play_pos(speed, pr)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wsola_pipeline_integration() {
        let mut wsola = WsolaProcessor::new();
        let sr = 44100.0f32;
        let num_frames = 2048;
        let mut samples = Vec::with_capacity(num_frames * 2);
        for i in 0..num_frames {
            let s = ((i as f32) * 0.01).sin();
            samples.push(s);
            samples.push(s);
        }

        let mut out_l = vec![0.0f32; 512];
        let mut out_r = vec![0.0f32; 512];
        let mut finished = false;

        wsola.process(
            &samples,
            2,
            num_frames,
            512,
            1.2,
            1.05,
            sr,
            &mut out_l,
            &mut out_r,
            &mut finished,
        );

        assert!(out_l.iter().any(|v| v.abs() > 1e-4));
        assert!(out_r.iter().any(|v| v.abs() > 1e-4));
        assert!(!finished);
    }

    #[test]
    fn select_swaps_engine_on_pitch_up() {
        let mut s = Stretcher::new();
        // Bypass region: neither engine.
        s.select(1.0, 1.0, 0.0);
        assert!(!s.using_vocoder);
        // Pitch up at 1.0x speed means compression (stretch = 1/pr < 1) —
        // the vocoder's region.
        s.select(1.0, 1.5, 0.0);
        assert!(s.using_vocoder);
        // Speed above the pitch ratio flips back to the WSOLA.
        s.select(2.0, 1.5, 0.0);
        assert!(!s.using_vocoder);
    }
}
