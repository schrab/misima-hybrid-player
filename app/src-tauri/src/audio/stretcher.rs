//! Engine selector between the WSOLA, phase-vocoder, and Paulstretch
//! time-stretchers.
//!
//! Platform-free: depends only on the three stretch engines, so both
//! `src-tauri` and the web `wasm-dsp` crate can `#[path]`-include it.

use crate::audio::paulstretch::Paulstretch;
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
///
/// Tempo-down playback (`speed < 1.0`) belongs to the Paulstretch engine, at
/// any pitch ratio: it stays musical at expansions the WSOLA would turn
/// granular (up to 20x at the fader floor of 0.1 with pitch ratio 2.0). At
/// `speed >= 1.0` the original split is unchanged — vocoder for
/// `stretch <= 1`, WSOLA for `stretch > 1` — and neither of those engines
/// ever sees a stretch beyond 4 (`speed 2.0 / pr 0.5`). Bit-perfect bypass
/// at speed 1.0 / pitch 0 is decided upstream (`player.rs` / `processor.rs`)
/// and never reaches `select` as a DSP request.
pub struct Stretcher {
    wsola: WsolaProcessor,
    vocoder: PhaseVocoder,
    paulstretch: Paulstretch,
    using_vocoder: bool,
    using_paul: bool,
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
            paulstretch: Paulstretch::new(),
            using_vocoder: false,
            using_paul: false,
        }
    }

    /// The vocoder's region of the `speed >= 1.0` split: pitch-shifted and
    /// compressing (or at unity), where its overlap-add stays exact.
    fn wants_vocoder(speed: f32, pr: f32) -> bool {
        let stretch = speed / pr;
        (pr - 1.0).abs() >= 0.002 && stretch <= 1.0
    }

    /// Choose the engine for these parameters, re-seeding it at `pos` when
    /// the choice changes so a fader crossing never resumes from stale state.
    pub fn select(&mut self, speed: f32, pr: f32, pos: f64) {
        let want_paul = speed < 1.0;
        if want_paul != self.using_paul {
            self.using_paul = want_paul;
            self.reset(pos);
            if !want_paul {
                // Leaving the Paulstretch region: `using_vocoder` is stale
                // from whenever tempo last went under 1.0, so re-evaluate
                // the vocoder/WSOLA split for the current parameters.
                self.using_vocoder = Self::wants_vocoder(speed, pr);
            }
            return;
        }
        if want_paul {
            return;
        }
        let want = Self::wants_vocoder(speed, pr);
        if want != self.using_vocoder {
            self.using_vocoder = want;
            self.reset(pos);
        }
    }

    pub fn reset(&mut self, pos: f64) {
        self.wsola.reset(pos);
        self.vocoder.reset(pos);
        self.paulstretch.reset(pos);
    }

    /// Top up the Paulstretch FIFOs for a device block larger than the
    /// engine's assumed 8192-frame ceiling. Call once at stream construction
    /// — never on the audio thread (AGENTS.md 3.1).
    pub fn reserve_block(&mut self, block_frames: usize) {
        self.paulstretch.reserve_for_block(block_frames);
    }

    /// Test only: the Paulstretch engine's raw source cursor, so the
    /// selector's tests can observe that a fader move inside the
    /// Paulstretch region does not re-seed the engine.
    #[cfg(test)]
    fn paulstretch_cursor(&self) -> f64 {
        self.paulstretch.cursor_for_test()
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
        if self.using_paul {
            self.paulstretch.process(
                samples, ch, total_frames, frames, speed, pr, sample_rate, bl, br, finished,
            );
        } else if self.using_vocoder {
            self.vocoder
                .process(samples, ch, total_frames, frames, speed, pr, bl, br, finished);
        } else {
            self.wsola.process(
                samples, ch, total_frames, frames, speed, pr, sample_rate, bl, br, finished,
            );
        }
    }

    pub fn get_play_pos(&self, speed: f32, pr: f32) -> f64 {
        if self.using_paul {
            self.paulstretch.get_play_pos(speed, pr)
        } else if self.using_vocoder {
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

    #[test]
    fn select_routes_tempo_down_to_paulstretch() {
        let mut s = Stretcher::new();
        // Any speed < 1.0 is Paulstretch's, at any pitch.
        s.select(0.9, 1.0, 0.0);
        assert!(s.using_paul);
        assert!(!s.using_vocoder);
        s.select(0.9, 0.5, 1000.0);
        assert!(s.using_paul);

        // Fader moves that stay under 1.0 must not re-seed. Push real audio
        // through first so the engine's cursor is meaningfully advanced, then
        // move the fader and require the cursor to be untouched — a select
        // that reset on every call would otherwise pass on the flag alone.
        let samples = vec![0.5f32; 2 * 44_100];
        let (mut out_l, mut out_r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        let mut finished = false;
        for _ in 0..40 {
            s.process(
                &samples, 2, 44_100, 512, 0.9, 1.0, 44_100.0, &mut out_l, &mut out_r,
                &mut finished,
            );
        }
        let cursor = s.paulstretch_cursor();
        assert!(cursor > 0.0, "cursor never advanced; nothing to protect");
        s.select(0.4, 1.0, 2000.0);
        assert!(s.using_paul);
        assert_eq!(
            s.paulstretch_cursor(),
            cursor,
            "a fader move that stays under 1.0 re-seeded the engine"
        );

        // Exactly 1.0 stays with the original split (bypass is decided
        // upstream; here pr 1.0 means the WSOLA, which bypass never reaches).
        s.select(1.0, 1.0, 3000.0);
        assert!(!s.using_paul);
        assert!(!s.using_vocoder);
        // Crossing back up into pitch-up compression re-evaluates the
        // vocoder/WSOLA split from stale state.
        s.select(1.1, 1.5, 4000.0);
        assert!(!s.using_paul);
        assert!(s.using_vocoder);
        s.select(1.5, 0.5, 5000.0);
        assert!(!s.using_paul);
        assert!(!s.using_vocoder);
    }

    #[test]
    fn paulstretch_pipeline_integration() {
        // Tempo-down through the selector: speed 0.25 -> S = 4. Broadband
        // material per AGENTS.md 3.1.11; assert finite, bounded, non-silent
        // output, the ~4x length, and a sane play position.
        let mut s = Stretcher::new();
        let sr = 44100.0f32;
        let num_frames = 16384usize;
        let mut samples = Vec::with_capacity(num_frames * 2);
        let (mut sl, mut srng) = (0x1234_5678u32, 0x9ABC_DEF0u32);
        for _ in 0..num_frames {
            sl = sl.wrapping_mul(1664525).wrapping_add(1013904223);
            srng = srng.wrapping_mul(1664525).wrapping_add(1013904223);
            let l = (((sl >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 0.8;
            let r = (((srng >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 0.8;
            samples.push(l);
            samples.push(r);
        }

        s.select(0.25, 1.0, 0.0);
        assert!(s.using_paul);

        let (mut out_l, mut out_r) = (vec![0.0f32; 512], vec![0.0f32; 512]);
        let mut total_out = 0usize;
        let mut finished = false;
        for _ in 0..(num_frames * 4 / 512 + 32) {
            s.process(
                &samples, 2, num_frames, 512, 0.25, 1.0, sr, &mut out_l, &mut out_r, &mut finished,
            );
            for v in out_l.iter().chain(out_r.iter()) {
                assert!(v.is_finite(), "non-finite output at speed 0.25");
                assert!(v.abs() <= 4.0, "output diverged to {v}");
            }
            assert!(
                out_l.iter().any(|v| v.abs() > 1e-3),
                "output went silent mid-stream"
            );
            total_out += 512;
            if finished {
                break;
            }
        }
        // ~4x expansion within a window of tolerance.
        let expected = num_frames as f64 / 0.25;
        assert!(
            (total_out as f64 - expected).abs() <= 8192.0,
            "output {total_out} vs expected {expected}"
        );
        let pos = s.get_play_pos(0.25, 1.0);
        assert!(pos.is_finite());
        // At the drain the cursor sits one window past the source (the last
        // flush read), so allow up to total + N.
        assert!(
            (0.0..=(num_frames as f64 + 8192.0)).contains(&pos),
            "play pos {pos} escaped the source"
        );
    }
}
