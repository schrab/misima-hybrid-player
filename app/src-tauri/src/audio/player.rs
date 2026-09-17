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
    pub spectrum: Mutex<Option<SpectrumAnalyzer>>,
    pub ended: AtomicBool,
    pub device_rate: AtomicUsize,
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
            spectrum: Mutex::new(None),
            ended: AtomicBool::new(false),
            device_rate: AtomicUsize::new(44_100),
        }
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

pub fn load_and_play(path: &Path) -> anyhow::Result<()> {
    let audio = decode_file(path)?;
    let shared = shared();
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

pub fn seek_secs(secs: f64) {
    let shared = shared();
    let sr = shared.sample_rate.load(Ordering::SeqCst);
    let ch = shared.channels.load(Ordering::SeqCst).max(1);
    let frame = (secs.max(0.0) * sr as f64) as usize;
    shared.cursor.store(frame * ch, Ordering::SeqCst);
    shared.ended.store(false, Ordering::SeqCst);
}

pub fn position_secs() -> f64 {
    let shared = shared();
    let sr = shared.sample_rate.load(Ordering::SeqCst) as f64;
    let ch = shared.channels.load(Ordering::SeqCst).max(1) as f64;
    let cursor = shared.cursor.load(Ordering::SeqCst) as f64;
    if sr <= 0.0 {
        return 0.0;
    }
    cursor / (sr * ch)
}

pub fn take_ended() -> bool {
    shared().ended.swap(false, Ordering::SeqCst)
}

fn ensure_stream() {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.get().is_some() {
        return;
    }
    let _ = STARTED.set(());
    if let Err(e) = start_output_stream() {
        log::error!("audio device error: {e}");
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
    std::mem::forget(stream);
    Ok(())
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
            let mut mono_scratch: Vec<f32> = Vec::with_capacity(frames);
            let playing = shared.playing.load(Ordering::SeqCst);
            let ch_in = shared.channels.load(Ordering::SeqCst).max(1);
            let volume = *shared.volume.lock();
            let samples = shared.samples.read();
            let total = samples.len();
            let mut finished = false;

            for f in 0..frames {
                let mut sample_l = 0.0f32;
                let mut sample_r = 0.0f32;
                if playing {
                    let cursor = shared.cursor.load(Ordering::SeqCst);
                    if cursor + ch_in <= total {
                        sample_l = samples[cursor];
                        sample_r = if ch_in > 1 {
                            samples[cursor + 1]
                        } else {
                            samples[cursor]
                        };
                        mono_scratch.push((sample_l + sample_r) * 0.5);
                        shared.cursor.store(cursor + ch_in, Ordering::SeqCst);
                    } else {
                        finished = true;
                        mono_scratch.push(0.0);
                    }
                } else {
                    mono_scratch.push(0.0);
                }

                let mut frame = [sample_l, sample_r];
                {
                    let mut eq = shared.eq.lock();
                    eq.process_interleaved(&mut frame, 2);
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
            drop(samples);

            if finished {
                shared.playing.store(false, Ordering::SeqCst);
                shared.ended.store(true, Ordering::SeqCst);
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
        loop {
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
