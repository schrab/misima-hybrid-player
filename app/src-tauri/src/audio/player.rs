//! Playback engine: cpal output + EQ + spectrum tap.

use crate::audio::decoder::decode_file;
use crate::audio::eq::EqState;
use crate::audio::spectrum::SpectrumAnalyzer;
use parking_lot::{Mutex, RwLock};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stopped,
    Playing,
    Paused,
}

#[derive(Debug)]
pub struct PlayerState {
    pub transport: Transport,
    pub volume: f32,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            transport: Transport::Stopped,
            volume: 0.8,
        }
    }
}

pub struct SharedPlay {
    pub samples: Arc<RwLock<Vec<f32>>>,
    pub sample_rate: AtomicUsize,
    pub channels: AtomicUsize,
    pub cursor: AtomicUsize,
    pub playing: AtomicBool,
    pub volume: Mutex<f32>,
    pub eq: Mutex<EqState>,
    pub eq_gains: Mutex<[f32; 10]>,
    pub spectrum_tx: Mutex<Vec<f32>>,
    pub spectrum_ready: AtomicBool,
    /// 226-point mono waveform for echo scope (last block, decimated).
    pub wave_tx: Mutex<Vec<f32>>,
    pub wave_ready: AtomicBool,
    pub spectrum: Mutex<Option<SpectrumAnalyzer>>,
    pub ended: AtomicBool,
    pub device_rate: AtomicUsize,
    pub pitch_semitones: Mutex<f32>,
    pub speed: Mutex<f32>,
    pub reverb: Mutex<Reverb>,
    pub reverb_mix: Mutex<f32>,
    /// Fractional source frame index (device-rate buffer).
    pub play_pos: Mutex<f64>,
    /// Bumped on stop so late decode must not autoplay.
    pub load_gen: std::sync::atomic::AtomicUsize,
}

impl Default for SharedPlay {
    fn default() -> Self {
        Self {
            samples: Arc::new(RwLock::new(Vec::new())),
            sample_rate: AtomicUsize::new(44_100),
            channels: AtomicUsize::new(2),
            cursor: AtomicUsize::new(0),
            playing: AtomicBool::new(false),
            volume: Mutex::new(0.8),
            eq: Mutex::new(EqState::default()),
            eq_gains: Mutex::new([0.0; 10]),
            spectrum_tx: Mutex::new(vec![0.0; 48]),
            spectrum_ready: AtomicBool::new(false),
            wave_tx: Mutex::new(vec![0.0; 226]),
            wave_ready: AtomicBool::new(false),
            spectrum: Mutex::new(None),
            ended: AtomicBool::new(false),
            device_rate: AtomicUsize::new(44_100),
            pitch_semitones: Mutex::new(0.0),
            speed: Mutex::new(1.0),
            reverb: Mutex::new(Reverb::default()),
            reverb_mix: Mutex::new(0.15),
            play_pos: Mutex::new(0.0),
            load_gen: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

/// Simple Schroeder reverb (4 combs + 2 allpass), stereo-safe via mono mix path.
#[derive(Debug, Clone)]
pub struct Reverb {
    combs: [(Vec<f32>, usize, f32); 4],
    allpass: [(Vec<f32>, usize, f32); 2],
    damp: f32,
}

impl Default for Reverb {
    fn default() -> Self {
        let comb_lens = [1557, 1617, 1491, 1422];
        let ap_lens = [225, 556];
        Self {
            combs: comb_lens.map(|n| (vec![0.0; n], 0, 0.82)),
            allpass: ap_lens.map(|n| (vec![0.0; n], 0, 0.5)),
            damp: 0.25,
        }
    }
}

impl Reverb {
    pub fn process_mono(&mut self, x: f32) -> f32 {
        let mut acc = 0.0f32;
        for (buf, idx, fb) in self.combs.iter_mut() {
            let y = buf[*idx];
            acc += y;
            let mut store = y * (1.0 - self.damp) + buf[(*idx + buf.len() - 1) % buf.len()] * self.damp;
            store = store * *fb + x;
            buf[*idx] = store.clamp(-2.0, 2.0);
            *idx = (*idx + 1) % buf.len();
        }
        let mut out = acc * 0.25;
        for (buf, idx, g) in self.allpass.iter_mut() {
            out = crate::audio::eq::allpass_tick(buf, idx, *g, out);
        }
        out
    }
}

fn shared_cell() -> &'static Arc<SharedPlay> {
    static SHARED: OnceLock<Arc<SharedPlay>> = OnceLock::new();
    SHARED.get_or_init(|| Arc::new(SharedPlay::default()))
}

pub fn shared() -> Arc<SharedPlay> {
    shared_cell().clone()
}

/// Linear resample interleaved PCM from `from_rate` to `to_rate`.
pub fn resample_interleaved(
    input: &[f32],
    channels: usize,
    from_rate: u32,
    to_rate: u32,
) -> Vec<f32> {
    if channels == 0 || from_rate == 0 || to_rate == 0 {
        return input.to_vec();
    }
    if from_rate == to_rate {
        return input.to_vec();
    }
    let in_frames = input.len() / channels;
    if in_frames == 0 {
        return Vec::new();
    }
    let ratio = from_rate as f64 / to_rate as f64;
    let out_frames = ((in_frames as f64) * (to_rate as f64 / from_rate as f64)).floor() as usize;
    let mut out = Vec::with_capacity(out_frames * channels);
    for of in 0..out_frames {
        let src = of as f64 * ratio;
        let i0 = src.floor() as usize;
        let i1 = (i0 + 1).min(in_frames - 1);
        let t = (src - i0 as f64) as f32;
        for c in 0..channels {
            let s0 = input[i0 * channels + c];
            let s1 = input[i1 * channels + c];
            out.push(s0 + (s1 - s0) * t);
        }
    }
    out
}

fn device_sample_rate() -> u32 {
    use cpal::traits::{DeviceTrait, HostTrait};
    let host = cpal::default_host();
    if let Some(device) = host.default_output_device() {
        if let Ok(cfg) = device.default_output_config() {
            return cfg.sample_rate().0;
        }
    }
    44_100
}

/// Decode and start playback only if `gen` is still current (None = current at call).
pub fn load_and_play(path: &Path) -> anyhow::Result<()> {
    let shared = shared();
    let gen = shared.load_gen.load(Ordering::SeqCst);
    let audio = decode_file(path)?;
    if shared.load_gen.load(Ordering::SeqCst) != gen {
        return Ok(()); // user stopped — do not autoplay
    }
    let device_rate = device_sample_rate();
    shared.device_rate.store(device_rate as usize, Ordering::SeqCst);

    let samples = resample_interleaved(
        &audio.samples,
        audio.channels.max(1),
        audio.sample_rate,
        device_rate,
    );

    {
        let mut buf = shared.samples.write();
        *buf = samples;
    }
    shared
        .sample_rate
        .store(device_rate as usize, Ordering::SeqCst);
    shared.channels.store(audio.channels.max(1), Ordering::SeqCst);
    shared.cursor.store(0, Ordering::SeqCst);
    *shared.play_pos.lock() = 0.0;
    shared.ended.store(false, Ordering::SeqCst);
    {
        let gains = *shared.eq_gains.lock();
        let mut eq = shared.eq.lock();
        *eq = EqState::new(device_rate as f32, &gains);
    }
    shared.playing.store(true, Ordering::SeqCst);
    ensure_stream();
    Ok(())
}

pub fn play() {
    let shared = shared();
    if !shared.samples.read().is_empty() {
        shared.ended.store(false, Ordering::SeqCst);
        shared.playing.store(true, Ordering::SeqCst);
        ensure_stream();
    }
}

pub fn pause() {
    shared().playing.store(false, Ordering::SeqCst);
}

pub fn stop() {
    let shared = shared();
    shared.playing.store(false, Ordering::SeqCst);
    shared.cursor.store(0, Ordering::SeqCst);
    *shared.play_pos.lock() = 0.0;
    shared.ended.store(false, Ordering::SeqCst);
    shared.load_gen.fetch_add(1, Ordering::SeqCst);
}

pub fn seek_secs(secs: f64) {
    let shared = shared();
    let sr = shared.sample_rate.load(Ordering::SeqCst) as f64;
    let frame = (secs.max(0.0) * sr) as f64;
    *shared.play_pos.lock() = frame;
    let ch = shared.channels.load(Ordering::SeqCst).max(1);
    shared.cursor.store((frame as usize) * ch, Ordering::SeqCst);
    shared.ended.store(false, Ordering::SeqCst);
}

pub fn set_volume(v: f32) {
    *shared().volume.lock() = v.clamp(0.0, 1.0);
}

pub fn set_eq(gains: [f32; 10]) {
    let shared = shared();
    *shared.eq_gains.lock() = gains;
    let sr = shared.device_rate.load(Ordering::SeqCst) as f32;
    let sr = if sr > 0.0 { sr } else { 44_100.0 };
    shared.eq.lock().set_gains(sr, &gains);
}

pub fn set_params(volume: f32, pitch_st: f32, reverb: f32, eq: [f32; 10], speed: f32) {
    set_volume(volume);
    *shared().pitch_semitones.lock() = pitch_st.clamp(-24.0, 24.0);
    *shared().speed.lock() = speed.clamp(0.5, 2.0);
    *shared().reverb_mix.lock() = reverb.clamp(0.0, 1.0);
    set_eq(eq);
}

/// Combined playback rate: speed (tape) * pitch semitones.
/// Tempo: how fast music plays. Does NOT change pitch (WSOLA/OLA time-stretch).
fn tempo_factor() -> f32 {
    shared().speed.lock().clamp(0.5, 2.0)
}

/// Pitch: semitone tone shift. Does NOT change speed (OLA pitch-shifter).
fn pitch_ratio() -> f32 {
    let pitch = *shared().pitch_semitones.lock();
    2f32.powf(pitch / 12.0)
}

/// OLA time-stretch: same grain (no resample) -> pitch unchanged, speed changes.
/// ipos advances by hop*speed per output hop.
fn time_stretch_ola(
    src: &[f32],
    ch: usize,
    n_out: usize,
    speed: f32,
    hann: &[f32],
    ipos: &mut f64,
) -> (Vec<f32>, Vec<f32>) {
    let win = hann.len();
    let hop = win / 2;
    let frames_src = src.len() / ch.max(1);
    let mut ol = vec![0.0f32; n_out];
    let mut or = vec![0.0f32; n_out];
    let mut opos = 0usize;
    while opos < n_out {
        let base = *ipos;
        for i in 0..win {
            let fi = base as usize + i;
            if fi >= frames_src {
                break;
            }
            let w = hann[i];
            let a = fi * ch;
            ol[opos.min(n_out - 1)] += 0.0; // keep types; fill below
            if opos + i < n_out {
                ol[opos + i] += src[a] * w;
                or[opos + i] += if ch > 1 { src[a + 1] * w } else { src[a] * w };
            }
        }
        // normalize hop overlap later; advance analysis by hop*speed (tempo)
        *ipos += (hop as f64) * (speed as f64);
        opos += hop;
        if *ipos as usize >= frames_src {
            break;
        }
    }
    // crude OLA gain
    let g = 2.0 / 3.0f32;
    for i in 0..n_out {
        ol[i] *= g;
        or[i] *= g;
    }
    (ol, or)
}

pub fn position_secs() -> f64 {
    let shared = shared();
    let sr = shared.sample_rate.load(Ordering::SeqCst) as f64;
    if sr <= 0.0 {
        return 0.0;
    }
    let pos = *shared.play_pos.lock();
    pos / sr
}

pub fn take_ended() -> bool {
    shared().ended.swap(false, Ordering::SeqCst)
}

fn ensure_stream() {
    static STARTED: OnceLock<()> = OnceLock::new();
    // Retry if the first attempt failed (device not ready, etc.)
    if STARTED.get().is_some() {
        return;
    }
    match start_output_stream() {
        Ok(()) => {
            let _ = STARTED.set(());
        }
        Err(e) => {
            log::error!("audio device error (will retry): {e}");
        }
    }
}

fn start_output_stream() -> anyhow::Result<()> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no output device"))?;
    let config = device.default_output_config()?;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.clone().into();
    let out_channels = stream_config.channels as usize;
    let rate = config.sample_rate().0;
    shared().device_rate.store(rate as usize, Ordering::SeqCst);
    let shared = shared();

    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            build_stream::<f32>(&device, &stream_config, out_channels, &shared)?
        }
        cpal::SampleFormat::I16 => {
            build_stream::<i16>(&device, &stream_config, out_channels, &shared)?
        }
        cpal::SampleFormat::U16 => {
            build_stream::<u16>(&device, &stream_config, out_channels, &shared)?
        }
        _ => anyhow::bail!("unsupported sample format {sample_format:?}"),
    };
    stream.play()?;
    // Stream is !Sync — leak it; exit() tears the process down.
    std::mem::forget(stream);
    Ok(())
}

/// Mark UI/audio idle; process exit is forced from lib.rs so dev watcher unblocks.
pub fn shutdown() {
    shared().playing.store(false, Ordering::SeqCst);
    SHUTDOWN.store(true, Ordering::SeqCst);
}

static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);


/// Block OLA pitch shift (tone only). ratio > 1 raises pitch; length preserved.
fn pitch_shift_block(x: &mut [f32], ratio: f32, hann: &[f32], work: &mut Vec<f32>) {
    if (ratio - 1.0).abs() < 0.002 || x.is_empty() {
        return;
    }
    let win = hann.len();
    let hop = win / 2;
    work.clear();
    work.resize(x.len(), 0.0f32);
    let mut ipos = 0.0f32;
    let mut opos = 0usize;
    while opos + hop <= x.len() {
        for i in 0..win {
            let src = ipos + i as f32 * ratio;
            let i0 = src as usize;
            let frac = src - i0 as f32;
            let s0 = x.get(i0).copied().unwrap_or(0.0);
            let s1 = x.get(i0 + 1).copied().unwrap_or(0.0);
            let v = (s0 + (s1 - s0) * frac) * hann[i];
            if opos + i < work.len() {
                work[opos + i] += v;
            }
        }
        ipos += hop as f32;
        opos += hop;
    }
    x.copy_from_slice(work);
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    out_channels: usize,
    shared: &Arc<SharedPlay>,
) -> anyhow::Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;
    let shared = shared.clone();
    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let frames = data.len() / out_channels.max(1);
            let playing = shared.playing.load(Ordering::SeqCst);
            let ch_in = shared.channels.load(Ordering::SeqCst).max(1);
            let volume = *shared.volume.lock();
            let speed = tempo_factor();
            let pr = pitch_ratio();
            let samples = shared.samples.read();
            let total_frames = samples.len() / ch_in.max(1);
            let mut finished = false;
            let mut pos = *shared.play_pos.lock();

            static HANN: std::sync::OnceLock<Vec<f32>> = std::sync::OnceLock::new();
            let hann = HANN.get_or_init(|| {
                (0..512)
                    .map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / 511.0).cos())
                    .collect::<Vec<f32>>()
            });

            // 1) Tempo: OLA time-stretch (pitch preserved, speed changes)
            let (mut bl, mut br) = if playing && total_frames > 0 {
                time_stretch_ola(&samples, ch_in, frames, speed, hann, &mut pos)
            } else {
                (vec![0.0f32; frames], vec![0.0f32; frames])
            };
            if pos as usize >= total_frames {
                finished = playing;
            }
            *shared.play_pos.lock() = pos;
            shared
                .cursor
                .store((pos as usize) * ch_in, Ordering::SeqCst);

            // 2) Pitch: OLA pitch-shift (tone only, speed unchanged)
            if (pr - 1.0).abs() > 0.002 {
                let mut w1 = Vec::new();
                let mut w2 = Vec::new();
                pitch_shift_block(&mut bl, pr, hann, &mut w1);
                pitch_shift_block(&mut br, pr, hann, &mut w2);
            }

            let mut mono_scratch = Vec::with_capacity(frames);
            for f in 0..frames {
                let mut frame = [bl[f], br[f]];
                mono_scratch.push((frame[0] + frame[1]) * 0.5);
                {
                    let mut eq = shared.eq.lock();
                    eq.process_interleaved(&mut frame, 2);
                }
                let mix = *shared.reverb_mix.lock();
                if mix > 0.001 {
                    let mono = (frame[0] + frame[1]) * 0.5;
                    let wet = shared.reverb.lock().process_mono(mono) * 0.35;
                    let wet = wet / (1.0 + wet.abs());
                    frame[0] = frame[0] * (1.0 - mix) + wet * mix;
                    frame[1] = frame[1] * (1.0 - mix) + wet * mix;
                }
                let l = (frame[0] * volume).clamp(-1.0, 1.0);
                let r = (frame[1] * volume).clamp(-1.0, 1.0);
                let o = f * out_channels;
                if out_channels >= 1 {
                    data[o] = T::from_sample(l);
                }
                if out_channels >= 2 {
                    data[o + 1] = T::from_sample(r);
                }
                for c in 2..out_channels {
                    data[o + c] = T::from_sample(0.0f32);
                }
            }

            if playing && !mono_scratch.is_empty() {
                let mut guard = shared.spectrum.lock();
                if guard.is_none() {
                    *guard = Some(SpectrumAnalyzer::new(1024, 48));
                }
                if let Some(analyzer) = guard.as_mut() {
                    let bins = analyzer.analyze(&mono_scratch).to_vec();
                    *shared.spectrum_tx.lock() = bins;
                    shared.spectrum_ready.store(true, Ordering::Relaxed);
                }
                const W: usize = 226;
                let n = mono_scratch.len();
                let mut wave = shared.wave_tx.lock();
                if wave.len() != W {
                    *wave = vec![0.0; W];
                }
                for i in 0..W {
                    let s = i * n / W;
                    let e = ((i + 1) * n / W).max(s + 1);
                    let mut acc = 0.0f32;
                    for v in &mono_scratch[s..e.min(n)] {
                        acc += *v;
                    }
                    wave[i] = (acc / (e - s) as f32).clamp(-1.0, 1.0);
                }
                shared.wave_ready.store(true, Ordering::Relaxed);
            }

            if finished {
                shared.playing.store(false, Ordering::SeqCst);
                shared.ended.store(true, Ordering::SeqCst);
            }
        },
        |err| log::error!("stream error: {err}"),
        None,
    )?;
    Ok(stream)
}

pub fn spawn_spectrum_task(handle: tauri::AppHandle) {
    std::thread::spawn(move || {
        let shared = shared();
        while !SHUTDOWN.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(33));
            use tauri::Emitter;
            if take_ended() {
                let _ = handle.emit("track_ended", ());
            }
            if !shared.spectrum_ready.swap(false, Ordering::Relaxed) {
                continue;
            }
            let bins = shared.spectrum_tx.lock().clone();
            let _ = handle.emit("spectrum", bins);
            if shared.wave_ready.swap(false, Ordering::Relaxed) {
                let wave = shared.wave_tx.lock().clone();
                let _ = handle.emit("waveform", wave);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::decoder::write_test_wav;
    use tempfile::tempdir;

    #[test]
    fn resample_halves_rate() {
        // 4 frames mono 22050 → 11025 yields ~2 frames
        let input = vec![0.0, 1.0, 0.0, -1.0];
        let out = resample_interleaved(&input, 1, 22050, 11025);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn resample_identity_when_same_rate() {
        let input = vec![0.1, 0.2, 0.3, 0.4];
        let out = resample_interleaved(&input, 2, 44100, 44100);
        assert_eq!(out, input);
    }

    #[test]
    fn decode_load_sets_shared_buffer() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t.wav");
        write_test_wav(&path, 440.0, 0.2, 22050).unwrap();
        load_and_play(&path).expect("load");
        let shared = shared();
        assert!(!shared.samples.read().is_empty());
        // stored at device rate (default 44100 when no device)
        assert_eq!(shared.sample_rate.load(Ordering::SeqCst), shared.device_rate.load(Ordering::SeqCst));
        assert!(shared.playing.load(Ordering::SeqCst));
        stop();
        assert!(!shared.playing.load(Ordering::SeqCst));
    }

    #[test]
    fn reverb_dry_ish_stability() {
        let mut rv = Reverb::default();
        let mut peak = 0.0f32;
        for i in 0..2000 {
            let x = if i < 50 { 1.0 } else { 0.0 };
            let y = rv.process_mono(x);
            peak = peak.max(y.abs());
        }
        assert!(peak.is_finite() && peak < 5.0);
    }

    #[test]
    fn eq_gains_persist_across_set() {
        set_eq([6.0; 10]);
        assert_eq!(*shared().eq_gains.lock(), [6.0; 10]);
        set_eq([0.0; 10]);
    }

    #[test]
    fn process_eq_on_decoded_frames() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("t2.wav");
        write_test_wav(&path, 1000.0, 0.15, 44100).unwrap();
        let audio = decode_file(&path).unwrap();
        let mut buf = audio.samples.clone();
        let mut eq = EqState::new(44100.0, &[0.0, 0.0, 0.0, 0.0, 8.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        eq.process_interleaved(&mut buf, audio.channels.max(1));
        assert!(buf.iter().any(|s| s.abs() > 0.01));
    }
}
