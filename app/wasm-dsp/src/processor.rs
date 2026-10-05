//! The worklet-side DSP state: everything `player.rs` keeps in `SharedPlay`
//! and its audio callback, minus the parts that belong to cpal.
//!
//! Deliberately written as plain Rust with no `wasm_bindgen` in it, so the
//! whole chain can be unit-tested on the host target. `lib.rs` wraps it.

use crate::audio::eq::EqState;
use crate::audio::lpf::{Lowpass4, OPEN_CUTOFF};
use crate::audio::reverb_mix::Reverb;
use crate::audio::spectrum::SpectrumAnalyzer;
use crate::audio::stretcher::Stretcher;

/// Bins in the spectrum tap. Matches the desktop's 48-bin emit.
pub const SPECTRUM_BINS: usize = 48;
/// Points in the echo scope. Matches the desktop's 226-point emit.
pub const WAVEFORM_POINTS: usize = 226;
/// FFT size for the spectrum tap. Matches the desktop.
const FFT_SIZE: usize = 1024;
/// Capacity of the per-block output scratch. The worklet only ever asks for
/// 128 frames, but sizing well above it means a fader or seek that changes
/// engine mid-block still has somewhere to land without reallocating.
const SCRATCH_FRAMES: usize = 4096;
/// Run the visualizer taps at ~30 fps rather than once per 128-frame quantum
/// (~375 Hz at 48 kHz). The port only redraws that fast anyway, and the port
/// would otherwise be flooded with messages it cannot drain.
const TAP_INTERVAL_FRAMES: usize = 1584;

/// Player parameters, mirrored from `player::set_params`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Master lowpass cutoff in Hz; `OPEN_CUTOFF` = fully open.
    pub cutoff: f32,
    /// Combined playback rate (tape speed), 0.1..2.0. The bottom half is
    /// owned by the Paulstretch engine via the selector.
    pub speed: f32,
    /// Pitch in semitones, -12..+12 (±1 octave).
    pub pitch_semitones: f32,
    /// Reverb wet/dry, 0..1.
    pub reverb: f32,
    /// Eight peaking bands in dB.
    pub eq: [f32; 8],
    /// Shimmer shift interval in semitones (UI sends stop values).
    pub shift: f32,
    /// Shimmer loop damping, 0..1.
    pub tone: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            cutoff: OPEN_CUTOFF,
            speed: 1.0,
            pitch_semitones: 0.0,
            reverb: 0.0,
            eq: [0.0; 8],
            // The shimmer's musical rest position: an octave up, medium damping.
            shift: 12.0,
            tone: 0.65,
        }
    }
}

impl Params {
    /// Pitch as a frequency ratio. Mirrors `player::pitch_ratio`.
    fn pitch_ratio(&self) -> f32 {
        2f32.powf(self.pitch_semitones.clamp(-12.0, 12.0) / 12.0)
    }

    /// True at 1.0x speed and 0 semitones — the bit-perfect path.
    fn is_bypass(&self) -> bool {
        (self.speed - 1.0).abs() < 0.002 && (self.pitch_ratio() - 1.0).abs() < 0.002
    }
}

/// One loaded track: interleaved f32 at the context sample rate.
///
/// `decodeAudioData` already resamples to `AudioContext.sampleRate`, so — as
/// on the desktop after `resample_interleaved` — the source rate here always
/// equals the device rate and no resampler is needed in the worklet.
#[derive(Default)]
pub struct Track {
    samples: Vec<f32>,
    channels: usize,
}

impl Track {
    pub fn total_frames(&self) -> usize {
        self.samples.len() / self.channels.max(1)
    }
}

/// The whole web-side audio engine.
///
/// Mirrors the desktop's callback state exactly: the same bypass/dsp split,
/// the same engine crossover, the same post-EQ tap point for the visualizer,
/// and the same post-EQ reverb ordering.
pub struct DspProcessor {
    sample_rate: f32,
    track: Track,
    params: Params,

    eq: EqState,
    reverb: Reverb,
    lpf: Lowpass4,
    stretcher: Stretcher,

    playing: bool,
    /// Fractional source frame index, the analogue of `SharedPlay::play_pos`.
    play_pos: f64,
    /// Set when the track runs out; the worklet reports it to the main thread.
    ended: bool,
    /// Bumped on seek / track load so DSP buffers flush, like `seek_gen`.
    seek_gen: u64,

    // Bypass never feeds the stretcher, so its cursor goes stale while bypass
    // plays. Track the last path so a bypass->DSP transition can re-seed it.
    was_bypass: bool,
    last_seek_gen: u64,

    // Scratch, all sized once at construction and reused forever.
    bl: Vec<f32>,
    br: Vec<f32>,
    mono: Vec<f32>,
    spectrum: SpectrumAnalyzer,
    spectrum_out: Vec<f32>,
    wave_out: Vec<f32>,
    tap_frames: usize,
}

impl DspProcessor {
    /// `sample_rate` is the `AudioContext` rate — 48000 or 44100 in practice.
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate > 0.0 { sample_rate } else { 44_100.0 };
        Self {
            sample_rate,
            track: Track::default(),
            params: Params::default(),
            eq: EqState::new(sample_rate, &[0.0; 8]),
            reverb: Reverb::new(sample_rate),
            lpf: Lowpass4::new(sample_rate),
            stretcher: Stretcher::new(),
            playing: false,
            play_pos: 0.0,
            ended: false,
            seek_gen: 0,
            was_bypass: true,
            last_seek_gen: u64::MAX,
            bl: vec![0.0; SCRATCH_FRAMES],
            br: vec![0.0; SCRATCH_FRAMES],
            mono: Vec::with_capacity(FFT_SIZE + TAP_INTERVAL_FRAMES),
            spectrum: SpectrumAnalyzer::new(FFT_SIZE, SPECTRUM_BINS),
            spectrum_out: vec![0.0; SPECTRUM_BINS],
            wave_out: vec![0.0; WAVEFORM_POINTS],
            tap_frames: 0,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Install a decoded track and rewind to its start.
    ///
    /// The sample data is interleaved stereo (or mono, which the stretcher
    /// duplicates to both channels) at `self.sample_rate`.
    pub fn load_track(&mut self, samples: Vec<f32>, channels: usize) {
        let channels = channels.max(1);
        self.track = Track { samples, channels };
        self.play_pos = 0.0;
        self.ended = false;
        self.playing = false;
        self.eq = EqState::new(self.sample_rate, &self.params.eq);
        self.reverb = Reverb::new(self.sample_rate);
        // `Reverb::new` starts the cascade at its own defaults, so re-apply the
        // live shift/tone — otherwise every track load silently resets the
        // shimmer faders.
        self.reverb.set_shift(self.params.shift.clamp(-12.0, 24.0));
        self.reverb.set_tone(self.params.tone.clamp(0.0, 1.0));
        self.seek_gen += 1;
    }

    pub fn track_frames(&self) -> usize {
        self.track.total_frames()
    }

    /// Duration in seconds, 0.0 when nothing is loaded.
    pub fn duration_secs(&self) -> f64 {
        self.track.total_frames() as f64 / self.sample_rate as f64
    }

    pub fn set_params(&mut self, params: Params) {
        let eq_changed = params.eq != self.params.eq;
        self.params = params;
        if eq_changed {
            // Coefficients update in place; the delay registers are untouched,
            // which is what keeps a fader sweep free of clicks (AGENTS.md 3.1).
            self.eq.set_gains(self.sample_rate, &self.params.eq);
        }
        // No-ops when the cutoff (or the rate, fixed per context) is unchanged.
        self.lpf.set_cutoff(self.sample_rate, self.params.cutoff);
        self.reverb.set_shift(self.params.shift.clamp(-12.0, 24.0));
        self.reverb.set_tone(self.params.tone.clamp(0.0, 1.0));
    }

    pub fn params(&self) -> Params {
        self.params
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        if playing {
            self.ended = false;
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Seek to a source frame, clamped into the loaded track.
    pub fn seek_frame(&mut self, frame: f64) {
        let total = self.track.total_frames();
        if total == 0 {
            self.play_pos = 0.0;
        } else {
            let last = (total - 1) as f64;
            self.play_pos = if frame.is_finite() { frame.clamp(0.0, last) } else { 0.0 };
        }
        self.ended = false;
        self.seek_gen += 1;
    }

    pub fn seek_secs(&mut self, secs: f64) {
        self.seek_frame(secs * self.sample_rate as f64);
    }

    /// Current source position in seconds. Reported to the UI from here, never
    /// derived from `AudioContext.currentTime` — that is wall time and lies at
    /// any speed other than 1.0.
    pub fn position_secs(&self) -> f64 {
        self.play_pos / self.sample_rate as f64
    }

    pub fn play_pos(&self) -> f64 {
        self.play_pos
    }

    /// True once, after the track has run out. Mirrors `player::take_ended`.
    pub fn take_ended(&mut self) -> bool {
        std::mem::take(&mut self.ended)
    }

    /// Spectrum bins from the last tap.
    pub fn spectrum(&self) -> &[f32] {
        &self.spectrum_out
    }

    /// Decimated scope points from the last tap.
    pub fn waveform(&self) -> &[f32] {
        &self.wave_out
    }

    /// Render `out.len() / 2` stereo frames of interleaved output.
    ///
    /// `out` is interleaved L,R so it lines up with an `AudioWorkletNode`'s
    /// two output channels without a deinterleave in between.
    pub fn process(&mut self, out: &mut [f32]) {
        let frames = out.len() / 2;
        if frames == 0 {
            return;
        }
        // Scratch is sized well above the 128-frame worklet quantum; a caller
        // asking for more than that falls back to silence rather than
        // reallocating on the audio thread.
        if frames > SCRATCH_FRAMES {
            out.fill(0.0);
            return;
        }
        // Move the scratch out of `self` for the duration of the block so the
        // source renderer can take `&mut self` while the output loop reads it.
        // `mem::take` leaves an empty Vec — no allocation, and the buffers go
        // back in with their capacity intact.
        let mut bl = std::mem::take(&mut self.bl);
        let mut br = std::mem::take(&mut self.br);
        bl.resize(frames, 0.0);
        br.resize(frames, 0.0);

        self.render_source(frames, &mut bl, &mut br);

        let mix = self.params.reverb.clamp(0.0, 1.0);
        self.reverb.set_mix(mix);
        let wg = self.reverb.wet_gain();
        self.mono.clear();

        for f in 0..frames {
            let mut frame = [bl[f], br[f]];
            self.eq.process_frame(&mut frame);
            // Tap point: post-EQ, pre-reverb — same place the desktop taps.
            self.mono.push((frame[0] + frame[1]) * 0.5);
            self.reverb.process_with_gain(&mut frame, wg);
            self.lpf.process_frame(&mut frame);
            // Unity master gain; the clamp stays as the safety net.
            out[f * 2] = frame[0].clamp(-1.0, 1.0);
            out[f * 2 + 1] = frame[1].clamp(-1.0, 1.0);
        }

        self.run_taps();
        self.bl = bl;
        self.br = br;
    }

    /// Fill `bl`/`br` with `frames` frames from the source: bit-perfect bypass,
    /// or the stretcher. Mirrors the desktop callback's three branches.
    fn render_source(&mut self, frames: usize, bl: &mut [f32], br: &mut [f32]) {
        let total_frames = self.track.total_frames();
        let ch = self.track.channels.max(1);
        let speed = self.params.speed.clamp(0.05, 2.0);
        let pr = self.params.pitch_ratio();
        let bypass = self.params.is_bypass();

        // A seek (or a fresh track) flushes the stretcher and the reverb tail,
        // and the master lowpass's ringing registers with them.
        let seek_gen = self.seek_gen;
        if seek_gen != self.last_seek_gen {
            self.last_seek_gen = seek_gen;
            self.stretcher.reset(self.play_pos);
            self.reverb.clear();
            self.lpf.clear();
        }

        if self.playing && !bypass && self.was_bypass {
            // Bypass never advances the stretcher's cursor, so engaging DSP
            // after a bypass must re-seed it at the live position or the track
            // audibly restarts from the stale cursor.
            self.stretcher.reset(self.play_pos);
        }
        if self.playing {
            self.was_bypass = bypass;
        }
        self.stretcher.select(speed, pr, self.play_pos);

        if !self.playing || total_frames == 0 {
            bl[..frames].fill(0.0);
            br[..frames].fill(0.0);
            return;
        }

        if bypass {
            let start = (self.play_pos.max(0.0) as usize).min(total_frames);
            let to_copy = (total_frames - start).min(frames);
            let samples = &self.track.samples;
            for f in 0..to_copy {
                let idx = (start + f) * ch;
                bl[f] = samples[idx];
                br[f] = if ch > 1 { samples[idx + 1] } else { samples[idx] };
            }
            if to_copy < frames {
                bl[to_copy..frames].fill(0.0);
                br[to_copy..frames].fill(0.0);
            }
            self.play_pos += frames as f64;
            if self.play_pos as usize >= total_frames {
                self.ended = true;
                self.playing = false;
            }
            return;
        }

        let mut finished = false;
        self.stretcher.process(
            &self.track.samples,
            ch,
            total_frames,
            frames,
            speed,
            pr,
            self.sample_rate,
            bl,
            br,
            &mut finished,
        );
        self.play_pos = self.stretcher.get_play_pos(speed, pr);
        if finished {
            self.playing = false;
            self.ended = true;
        }
    }

    /// FFT + scope decimation, throttled to ~30 fps.
    fn run_taps(&mut self) {
        self.tap_frames += self.mono.len();
        if self.tap_frames < TAP_INTERVAL_FRAMES {
            return;
        }
        self.tap_frames = 0;

        // The analyzer pads short input with zeros, so an interval's worth of
        // mono is exactly what it wants.
        self.spectrum_out
            .copy_from_slice(self.spectrum.analyze(&self.mono));

        let n = self.mono.len();
        if n == 0 {
            self.wave_out.iter_mut().for_each(|v| *v = 0.0);
            return;
        }
        for i in 0..WAVEFORM_POINTS {
            let s = i * n / WAVEFORM_POINTS;
            let e = ((i + 1) * n / WAVEFORM_POINTS).max(s + 1);
            let mut acc = 0.0f32;
            for v in &self.mono[s..e.min(n)] {
                acc += *v;
            }
            self.wave_out[i] = (acc / (e - s) as f32).clamp(-1.0, 1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(frames: usize, freq: f32, sr: f32) -> (Vec<f32>, usize) {
        let mut v = Vec::with_capacity(frames * 2);
        for i in 0..frames {
            let s = (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin() * 0.5;
            v.push(s);
            v.push(s);
        }
        (v, 2)
    }

    /// Stereo white noise, uncorrelated between channels — broadband
    /// material per AGENTS.md 3.1.11 (never sine-only for stretch tests).
    fn noise(frames: usize, amp: f32) -> (Vec<f32>, usize) {
        let mut v = Vec::with_capacity(frames * 2);
        let (mut sl, mut sr) = (0x1234_5678u32, 0x9ABC_DEF0u32);
        for _ in 0..frames {
            sl = sl.wrapping_mul(1664525).wrapping_add(1013904223);
            sr = sr.wrapping_mul(1664525).wrapping_add(1013904223);
            v.push((((sl >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * amp);
            v.push((((sr >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * 2.0 * amp);
        }
        (v, 2)
    }

    #[test]
    fn bypass_is_bit_exact() {
        // Speed 1.0 / pitch 0 must pass samples through untouched — that is
        // the whole point of the bypass branch, and the plan's parity check.
        let sr = 48_000.0;
        let (samples, ch) = tone(4096, 440.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples.clone(), ch);
        p.set_playing(true);

        let mut out = vec![0.0f32; 128 * 2];
        p.process(&mut out);
        assert_eq!(&out[..256], &samples[..256], "bypass altered samples");

        p.process(&mut out);
        assert_eq!(&out[..256], &samples[256..512], "bypass altered samples");
    }

    #[test]
    fn empty_processor_emits_silence_and_is_finite() {
        let mut p = DspProcessor::new(48_000.0);
        let mut out = vec![0.0f32; 128 * 2];
        p.process(&mut out);
        assert!(out.iter().all(|v| *v == 0.0));
        assert!(p.spectrum().iter().all(|v| v.is_finite()));
        assert!(p.waveform().iter().all(|v| v.is_finite()));
    }

    #[test]
    fn position_advances_in_bypass_and_clamps_at_end() {
        let sr = 48_000.0;
        let (samples, ch) = tone(1024, 440.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        p.set_playing(true);
        let mut out = vec![0.0f32; 128 * 2];
        p.process(&mut out);
        assert_eq!(p.play_pos(), 128.0);
        // Run to the end.
        for _ in 0..32 {
            p.process(&mut out);
        }
        assert!(p.take_ended(), "track end was not reported");
        assert!(!p.is_playing());
    }

    #[test]
    fn seek_clamps_into_track_and_rewinds_state() {
        let sr = 48_000.0;
        let (samples, ch) = tone(2048, 440.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        p.set_playing(true);

        p.seek_secs(0.01); // frame 480
        assert!((p.play_pos() - 480.0).abs() < 0.001);

        p.seek_secs(999.0); // past the end -> last frame
        assert!(p.play_pos() as usize <= 2047);

        p.seek_secs(-5.0); // negative -> start
        assert_eq!(p.play_pos(), 0.0);
    }

    #[test]
    fn eq_boost_changes_level_in_bypass() {
        let sr = 48_000.0;
        let (samples, ch) = tone(8192, 1000.0, sr);
        let rms = |d: &[f32]| d.iter().map(|v| v * v).sum::<f32>() / d.len() as f32;

        let mut flat = DspProcessor::new(sr);
        flat.load_track(samples.clone(), ch);
        flat.set_playing(true);

        let mut cut = DspProcessor::new(sr);
        cut.load_track(samples, ch);
        let mut params = Params::default();
        params.eq[2] = -12.0; // 1000 Hz band
        cut.set_params(params);
        cut.set_playing(true);

        let mut a = vec![0.0f32; 128 * 2];
        let mut b = vec![0.0f32; 128 * 2];
        for _ in 0..16 {
            flat.process(&mut a);
            cut.process(&mut b);
        }
        assert!(
            rms(&b) < rms(&a) * 0.8,
            "EQ cut did not reduce level: {} vs {}",
            rms(&b),
            rms(&a)
        );
    }

    #[test]
    fn stretched_playback_stays_finite_and_advances() {
        let sr = 48_000.0;
        let (samples, ch) = tone(48_000, 440.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        let mut params = Params::default();
        params.speed = 1.5;
        params.pitch_semitones = 4.0;
        p.set_params(params);
        p.set_playing(true);

        let mut out = vec![0.0f32; 128 * 2];
        let start = p.play_pos();
        for _ in 0..64 {
            p.process(&mut out);
            assert!(out.iter().all(|v| v.is_finite()), "stretcher diverged");
        }
        assert!(
            p.play_pos() > start,
            "play position did not advance under time-stretch"
        );
    }

    #[test]
    fn reverb_mix_stays_finite_across_a_full_sweep() {
        let sr = 48_000.0;
        let (samples, ch) = tone(48_000, 440.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        p.set_playing(true);
        let mut out = vec![0.0f32; 128 * 2];
        for step in 0..=10 {
            let mut params = Params::default();
            params.reverb = step as f32 / 10.0;
            p.set_params(params);
            for _ in 0..8 {
                p.process(&mut out);
                assert!(out.iter().all(|v| v.is_finite()), "reverb diverged at {step}");
            }
        }
    }

    #[test]
    fn engine_crossover_at_speed_one_is_glitch_free() {
        // Drive speed across the 1.0 line with pitch up, so the selector flips
        // between the WSOLA and the vocoder. Nothing may produce NaN.
        let sr = 48_000.0;
        let (samples, ch) = tone(48_000, 440.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        p.set_playing(true);
        let mut out = vec![0.0f32; 128 * 2];
        for step in 0..=20 {
            let mut params = Params::default();
            params.speed = 0.8 + (step as f32) * 0.04; // 0.8 .. 1.6, crosses 1.0
            params.pitch_semitones = 3.0;
            p.set_params(params);
            p.process(&mut out);
            assert!(
                out.iter().all(|v| v.is_finite()),
                "crossover produced non-finite output at speed {}",
                params.speed
            );
        }
    }

    #[test]
    fn taps_report_energy_only_while_signal_present() {
        let sr = 48_000.0;
        let (samples, ch) = tone(48_000, 1000.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        p.set_playing(true);
        let mut out = vec![0.0f32; 128 * 2];
        for _ in 0..64 {
            p.process(&mut out);
        }
        assert!(
            p.spectrum().iter().any(|v| *v > 0.01),
            "spectrum tap saw no energy"
        );
        assert!(
            p.waveform().iter().any(|v| v.abs() > 0.01),
            "scope tap saw no energy"
        );
    }

    #[test]
    fn paused_processor_reports_zero_spectrum() {
        let sr = 48_000.0;
        let (samples, ch) = tone(48_000, 1000.0, sr);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        p.set_playing(true);
        let mut out = vec![0.0f32; 128 * 2];
        for _ in 0..64 {
            p.process(&mut out);
        }
        // Stop, and let the throttled tap run again on silence.
        p.set_playing(false);
        for _ in 0..64 {
            p.process(&mut out);
        }
        assert!(
            p.spectrum().iter().all(|v| *v <= 0.02),
            "spectrum did not settle to zero when paused"
        );
    }

    #[test]
    fn paulstretch_crossover_at_speed_one_is_glitch_free() {
        // Sweep speed down through 1.0 (vocoder -> Paulstretch) and back up,
        // in both directions, with pitch up so the >= 1.0 side is the
        // vocoder's. Every block must be finite and bounded, and the render
        // must carry real signal — a flipped engine that emits silence or
        // NaN fails here.
        let sr = 48_000.0;
        let (samples, ch) = noise(48_000, 0.4);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        p.set_playing(true);
        let mut out = vec![0.0f32; 128 * 2];
        let mut peak = 0.0f32;
        for speed in [1.05f32, 1.03, 1.01, 0.99, 0.97, 0.95, 0.97, 0.99, 1.05] {
            let mut params = Params::default();
            params.speed = speed;
            params.pitch_semitones = 3.0;
            p.set_params(params);
            for _ in 0..8 {
                p.process(&mut out);
                for v in &out {
                    assert!(
                        v.is_finite(),
                        "crossover produced non-finite output at speed {speed}"
                    );
                    assert!(v.abs() <= 4.0, "crossover diverged to {v} at speed {speed}");
                    peak = peak.max(v.abs());
                }
            }
        }
        assert!(peak > 1e-2, "crossover sweep rendered silence (peak {peak})");
    }

    #[test]
    fn paulstretch_end_to_end_at_speed_point_one() {
        // Deep in the tempo-down region: speed 0.1 through the whole
        // processor (the fader floor is 0.05; 0.1 keeps the fixture's
        // 240k-sample tail drain cheap).
        // Output length ~10x input (+-window), play_pos monotonic and
        // advancing ~0.1 source frames per output frame, `ended` reported
        // after the tail drains.
        let sr = 48_000.0;
        let total = 12_000usize;
        let (samples, ch) = noise(total, 0.4);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        let mut params = Params::default();
        params.speed = 0.1;
        p.set_params(params);
        p.set_playing(true);

        let mut out = vec![0.0f32; 128 * 2];
        let mut collected: Vec<f32> = Vec::new();
        let mut prev_pos = 0.0f64;
        let mut blocks = 0usize;
        let cap = (total as f64 / 0.1) as usize / 128 + 128;
        let mut ended = false;
        while blocks < cap {
            p.process(&mut out);
            assert!(out.iter().all(|v| v.is_finite()), "NaN at block {blocks}");
            collected.extend_from_slice(&out);
            let pos = p.play_pos();
            assert!(pos >= prev_pos, "play_pos went backwards at block {blocks}");
            if (50..150).contains(&blocks) {
                // Mid-track: ~0.1 source frames per output frame, i.e. ~12.8
                // per 128-frame block (loose tolerance: the position lags by
                // the FIFO depth estimate, which moves in whole frames).
                let adv = pos - prev_pos;
                assert!(
                    (0.5 * 12.8..=1.5 * 12.8).contains(&adv),
                    "block {blocks} advanced {adv:.2}, expected ~12.8"
                );
            }
            prev_pos = pos;
            blocks += 1;
            if p.take_ended() {
                ended = true;
                break;
            }
        }
        assert!(ended, "finished never fired at speed 0.1");

        let out_frames = collected.len() / 2;
        let expected = total as f64 / 0.1;
        // Overshoot is one Paulstretch synthesis flush (H = 8192) plus the
        // last partial 128-frame block, so the tolerance is the engine's
        // full analysis window N = 16384.
        const PAUL_WINDOW: f64 = 16_384.0;
        assert!(
            (out_frames as f64 - expected).abs() <= PAUL_WINDOW,
            "output {out_frames} frames vs expected {expected}"
        );
        assert!(
            collected.iter().any(|v| v.abs() > 1e-2),
            "paulstretch end-to-end rendered silence"
        );
    }

    #[test]
    fn seek_during_paulstretch_is_clean() {
        // Seek mid-stream at speed 0.5 (Paulstretch's region): the seek_gen
        // flush must reset the engine with no NaN and land the position at
        // the requested frame.
        let sr = 48_000.0;
        let (samples, ch) = noise(48_000, 0.4);
        let mut p = DspProcessor::new(sr);
        p.load_track(samples, ch);
        let mut params = Params::default();
        params.speed = 0.5;
        p.set_params(params);
        p.set_playing(true);
        let mut out = vec![0.0f32; 128 * 2];
        for _ in 0..32 {
            p.process(&mut out);
        }

        p.seek_secs(0.5); // frame 24000
        let mut prev = p.play_pos();
        for _ in 0..16 {
            p.process(&mut out);
            for v in &out {
                assert!(v.is_finite(), "NaN after seek during paulstretch");
                assert!(v.abs() <= 4.0, "diverged to {v} after seek");
            }
            let pos = p.play_pos();
            assert!(pos >= prev, "play_pos went backwards after seek");
            prev = pos;
        }
        // 16 blocks at speed 0.5 consume ~1024 source frames past the cue.
        assert!(
            (prev - 24_000.0).abs() < 2048.0,
            "play_pos {prev} did not settle at the seek target 24000"
        );
    }
}
