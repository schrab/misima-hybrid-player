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
    /// Bumped on seek or track stop/load so DSP buffers flush.
    pub seek_gen: std::sync::atomic::AtomicUsize,
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
            seek_gen: std::sync::atomic::AtomicUsize::new(0),
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
        Self::new(44_100.0)
    }
}

impl Reverb {
    pub fn new(sample_rate: f32) -> Self {
        let sr_scale = (sample_rate / 44_100.0).max(0.5);
        let comb_lens = [1557, 1617, 1491, 1422];
        let ap_lens = [225, 556];
        Self {
            combs: comb_lens.map(|n| (vec![0.0; ((n as f32) * sr_scale).round() as usize], 0, 0.82)),
            allpass: ap_lens.map(|n| (vec![0.0; ((n as f32) * sr_scale).round() as usize], 0, 0.5)),
            damp: 0.25,
        }
    }
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

pub fn prepare_load() -> usize {
    let shared = shared();
    shared.playing.store(false, Ordering::SeqCst);
    shared.load_gen.fetch_add(1, Ordering::SeqCst) + 1
}

#[allow(dead_code)]
pub fn load_and_play(path: &Path) -> anyhow::Result<()> {
    let gen = prepare_load();
    load_and_play_gen(path, gen)
}

/// Decode and start playback only if `expected_gen` is still current.
pub fn load_and_play_gen(path: &Path, expected_gen: usize) -> anyhow::Result<()> {
    let shared = shared();
    let audio = decode_file(path)?;
    if shared.load_gen.load(Ordering::SeqCst) != expected_gen {
        return Ok(()); // user stopped or changed track — do not autoplay
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
        let mut rv = shared.reverb.lock();
        *rv = Reverb::new(device_rate as f32);
    }
    shared.seek_gen.fetch_add(1, Ordering::SeqCst);
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
    shared.seek_gen.fetch_add(1, Ordering::SeqCst);
}

pub fn seek_secs(secs: f64) {
    let shared = shared();
    let sr = shared.sample_rate.load(Ordering::SeqCst) as f64;
    let frame = secs.max(0.0) * sr;
    *shared.play_pos.lock() = frame;
    let ch = shared.channels.load(Ordering::SeqCst).max(1);
    shared.cursor.store((frame as usize) * ch, Ordering::SeqCst);
    shared.ended.store(false, Ordering::SeqCst);
    shared.seek_gen.fetch_add(1, Ordering::SeqCst);
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

/// Commands for the stream-owner thread — the only thread that ever creates,
/// holds, or drops the `cpal::Stream` (`cpal::Stream` is `!Send + !Sync`, so
/// it cannot live in shared state; see agents.md 3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StreamCmd {
    /// Build + play the output stream if none is live (idempotent).
    Start,
    /// Pause, drop the stream (CoreAudio HAL teardown), and exit the thread.
    Shutdown,
}

static OWNER_TX: OnceLock<crossbeam_channel::Sender<StreamCmd>> = OnceLock::new();
/// Set by the owner thread: true while a live stream exists.
static STREAM_LIVE: AtomicBool = AtomicBool::new(false);

fn ensure_stream() {
    let tx = OWNER_TX.get_or_init(|| {
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::Builder::new()
            .name("misima-stream-owner".into())
            .spawn(move || stream_owner(rx))
            .expect("spawn stream owner thread");
        tx
    });
    // Retry if the first attempt failed (device not ready, etc.): a failed
    // Start leaves STREAM_LIVE false, so the next play re-sends it.
    if !STREAM_LIVE.load(Ordering::SeqCst) {
        let _ = tx.send(StreamCmd::Start);
    }
}

/// Runs on its own thread for the life of the process (or until Shutdown).
/// All device lookup, stream construction, and teardown happen here so the
/// `!Send` stream object never crosses a thread boundary.
fn stream_owner(rx: crossbeam_channel::Receiver<StreamCmd>) {
    let mut stream: Option<cpal::Stream> = None;
    for cmd in rx {
        match cmd {
            StreamCmd::Start => {
                if stream.is_some() {
                    continue;
                }
                match start_output_stream() {
                    Ok(s) => {
                        stream = Some(s);
                        STREAM_LIVE.store(true, Ordering::SeqCst);
                    }
                    Err(e) => {
                        log::error!("audio device error (will retry): {e}");
                    }
                }
            }
            StreamCmd::Shutdown => {
                shared().playing.store(false, Ordering::SeqCst);
                SHUTDOWN.store(true, Ordering::SeqCst);
                if let Some(s) = stream.take() {
                    use cpal::traits::StreamTrait;
                    if let Err(e) = s.pause() {
                        log::warn!("error pausing stream during shutdown: {e}");
                    }
                    // Drop runs CoreAudio's AudioOutputUnitStop +
                    // AudioComponentInstanceDispose: the HAL device is
                    // released cleanly instead of being reclaimed by death.
                    drop(s);
                }
                STREAM_LIVE.store(false, Ordering::SeqCst);
                return;
            }
        }
    }
}

fn start_output_stream() -> anyhow::Result<cpal::Stream> {
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
    let block = block_frames(&config);
    shared().device_rate.store(rate as usize, Ordering::SeqCst);
    let shared = shared();

    let stream = match sample_format {
        cpal::SampleFormat::F32 => {
            build_stream::<f32>(&device, &stream_config, out_channels, &shared, block)?
        }
        cpal::SampleFormat::I16 => {
            build_stream::<i16>(&device, &stream_config, out_channels, &shared, block)?
        }
        cpal::SampleFormat::U16 => {
            build_stream::<u16>(&device, &stream_config, out_channels, &shared, block)?
        }
        _ => anyhow::bail!("unsupported sample format {sample_format:?}"),
    };
    stream.play()?;
    Ok(stream)
}

/// Mark UI/audio idle and release the output device.
///
/// Hands Shutdown to the owner thread and waits (bounded) for it to drop the
/// stream, so CoreAudio tears the HAL unit down properly. `lib.rs` still
/// forces `process::exit` afterwards as a belt-and-braces net for the dev
/// watcher.
pub fn shutdown() {
    shared().playing.store(false, Ordering::SeqCst);
    SHUTDOWN.store(true, Ordering::SeqCst);
    if let Some(tx) = OWNER_TX.get() {
        let _ = tx.send(StreamCmd::Shutdown);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while STREAM_LIVE.load(Ordering::SeqCst) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Upper bound on frames per output buffer, taken from the device's supported
/// range so the real-time callback can never hit the allocator (agents.md 3.1).
/// We deliberately take `max`, not `min`: the stream is opened with
/// `BufferSize::Default`, so the backend may pick anywhere in the range.
fn block_frames(config: &cpal::SupportedStreamConfig) -> usize {
    const FALLBACK: usize = 2048;
    match config.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => {
            (*max as usize).max(*min as usize).max(512)
        }
        cpal::SupportedBufferSize::Unknown => FALLBACK,
    }
}


fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    out_channels: usize,
    shared: &Arc<SharedPlay>,
    block: usize,
) -> anyhow::Result<cpal::Stream>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    use cpal::traits::DeviceTrait;
    let shared = shared.clone();
    let mut wsola = crate::audio::wsola::WsolaProcessor::new();
    // Pre-sized from the device's buffer range so the callback only resizes
    // within existing capacity — never allocates on the RT thread.
    let mut bl: Vec<f32> = Vec::with_capacity(block);
    let mut br: Vec<f32> = Vec::with_capacity(block);
    let mut mono_scratch: Vec<f32> = Vec::with_capacity(block);
    let mut last_seek_gen = usize::MAX;
    // Bypass never feeds WSOLA, so its cursor goes stale while bypass plays.
    // Track the last path to re-seed it on bypass→DSP transitions.
    let mut was_bypass = true;

    let stream = device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            let frames = data.len() / out_channels.max(1);
            let playing = shared.playing.load(Ordering::SeqCst);
            let ch_in = shared.channels.load(Ordering::SeqCst).max(1);
            let volume = *shared.volume.lock();
            let speed = tempo_factor();
            let pr = pitch_ratio();
            let device_sr = shared.device_rate.load(Ordering::SeqCst) as f32;
            let device_sr = if device_sr > 0.0 { device_sr } else { 44_100.0 };

            bl.resize(frames, 0.0);
            br.resize(frames, 0.0);

            let samples = shared.samples.read();
            let total_frames = samples.len() / ch_in.max(1);
            let mut finished = false;

            let cur_seek_gen = shared.seek_gen.load(Ordering::SeqCst);
            if cur_seek_gen != last_seek_gen {
                last_seek_gen = cur_seek_gen;
                let cur_pos = *shared.play_pos.lock();
                wsola.reset(cur_pos);
            }

            let is_bypass = (speed - 1.0).abs() < 0.002 && (pr - 1.0).abs() < 0.002;

            if playing && !is_bypass && was_bypass {
                // Engaging DSP after bypass: the WSOLA cursor is stale
                // (frozen since the last reset while bypass advanced play_pos),
                // so re-seed it at the live position. Without this the track
                // audibly restarts from the stale cursor. reset() re-enters
                // via the fade-in init grain path, so engagement stays clean.
                wsola.reset(*shared.play_pos.lock());
            }
            if playing {
                was_bypass = is_bypass;
            }

            if !playing || total_frames == 0 {
                bl[..frames].fill(0.0);
                br[..frames].fill(0.0);
            } else if is_bypass {
                // Bit-perfect 1:1 playback bypass for normal speed & pitch
                let mut pos = *shared.play_pos.lock();
                let start = (pos as usize).min(total_frames);
                let to_copy = (total_frames - start).min(frames);
                for f in 0..to_copy {
                    let idx = (start + f) * ch_in;
                    bl[f] = samples[idx];
                    br[f] = if ch_in > 1 { samples[idx + 1] } else { samples[idx] };
                }
                if to_copy < frames {
                    bl[to_copy..frames].fill(0.0);
                    br[to_copy..frames].fill(0.0);
                }
                pos += frames as f64;
                if pos as usize >= total_frames {
                    finished = true;
                }
                *shared.play_pos.lock() = pos;
                shared.cursor.store((pos as usize) * ch_in, Ordering::SeqCst);
            } else {
                // High-quality real-time WSOLA time-stretch + Cubic Hermite pitch shift
                wsola.process(
                    &samples,
                    ch_in,
                    total_frames,
                    frames,
                    speed,
                    pr,
                    device_sr,
                    &mut bl,
                    &mut br,
                    &mut finished,
                );
                let pos = wsola.get_play_pos(speed, pr);
                *shared.play_pos.lock() = pos;
                shared.cursor.store((pos as usize) * ch_in, Ordering::SeqCst);
            }

            mono_scratch.clear();
            let mut eq = shared.eq.lock();
            let mix = *shared.reverb_mix.lock();
            let mut reverb_guard = if mix > 0.001 {
                Some(shared.reverb.lock())
            } else {
                None
            };

            for f in 0..frames {
                let mut frame = [bl[f], br[f]];
                mono_scratch.push((frame[0] + frame[1]) * 0.5);
                eq.process_interleaved(&mut frame, 2);
                if let Some(reverb) = reverb_guard.as_deref_mut() {
                    let mono = (frame[0] + frame[1]) * 0.5;
                    let wet = reverb.process_mono(mono) * 0.35;
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
            drop(eq);
            drop(reverb_guard);

            if playing && !mono_scratch.is_empty() {
                let mut guard = shared.spectrum.lock();
                if guard.is_none() {
                    *guard = Some(SpectrumAnalyzer::new(1024, 48));
                }
                if let Some(analyzer) = guard.as_mut() {
                    let bins = analyzer.analyze(&mono_scratch);
                    let mut tx = shared.spectrum_tx.lock();
                    if tx.len() != bins.len() {
                        *tx = bins.to_vec();
                    } else {
                        tx.copy_from_slice(bins);
                    }
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

    /// Live CoreAudio smoke test. `#[ignore]`d because it opens the real output
    /// device and permanently flips the process-wide SHUTDOWN latch, which would
    /// poison the other tests sharing the `shared()` singleton.
    ///
    /// Run with: cargo test coreaudio_smoke -- --ignored --nocapture
    #[test]
    #[ignore = "requires a live CoreAudio output device"]
    fn coreaudio_smoke() {
        use cpal::traits::{DeviceTrait, HostTrait};

        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .expect("no default CoreAudio output device");
        let cfg = device.default_output_config().expect("default_output_config");
        let rate = cfg.sample_rate().0;
        let ch = cfg.channels();
        println!(
            "device: {:?} rate={} ch={} format={:?}",
            device.name(),
            rate,
            ch,
            cfg.sample_format()
        );
        assert!(
            rate == 44_100 || rate == 48_000,
            "expected 44.1k/48k hardware rate, got {rate}"
        );

        // 44.1k source must be resampled up to the hardware rate.
        let dir = tempdir().unwrap();
        let path = dir.path().join("smoke.wav");
        write_test_wav(&path, 440.0, 3.0, 44_100).unwrap();
        load_and_play(&path).expect("load_and_play must open CoreAudio stream");

        let shared = shared();
        assert_eq!(
            shared.sample_rate.load(Ordering::SeqCst),
            shared.device_rate.load(Ordering::SeqCst),
            "buffer must be resampled to the device rate"
        );
        assert_eq!(shared.device_rate.load(Ordering::SeqCst), rate as usize);

        // Let the real IOProc pull several buffers.
        std::thread::sleep(std::time::Duration::from_millis(1200));
        let bypass_pos = *shared.play_pos.lock();
        assert!(
            bypass_pos > 0.0,
            "bypass playback did not advance the play cursor"
        );
        assert!(
            shared.spectrum_ready.load(Ordering::Relaxed),
            "spectrum tap never became ready"
        );
        let tap_peak = shared.spectrum_tx.lock().iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(tap_peak.is_finite(), "spectrum tap produced non-finite data");
        println!("bypass: pos={bypass_pos:.1} frames  spectrum peak={tap_peak:.4}");

        // Now force the WSOLA + Cubic Hermite path (speed != 1.0, pitch != 0).
        // NOTE: only a short burst — engaging DSP must continue from the live
        // cursor, so after 0.3 s the position must be *ahead* of bypass_pos.
        // (A stale WSOLA cursor restarts the track: pos would fall to ~15k.)
        set_params(0.8, 3.0, 0.35, [4.0; 10], 1.25);
        std::thread::sleep(std::time::Duration::from_millis(300));
        let wsola_pos = *shared.play_pos.lock();
        assert!(
            wsola_pos.is_finite() && wsola_pos > bypass_pos,
            "WSOLA engagement jumped backward ({bypass_pos:.0} -> {wsola_pos:.0}): stale cursor restarted the track"
        );
        let wsola_peak = shared.spectrum_tx.lock().iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(wsola_peak.is_finite() && wsola_peak < 1e6, "WSOLA output diverged");
        println!("wsola: pos={wsola_pos:.1} frames  spectrum peak={wsola_peak:.4}");

        // Teardown must be panic-free and must not block.
        set_params(1.0, 0.0, 0.0, [0.0; 10], 1.0);
        stop();
        shutdown();
        assert!(
            !STREAM_LIVE.load(Ordering::SeqCst),
            "owner thread did not drop the stream within shutdown()'s wait bound"
        );
        println!("shutdown() returned cleanly (owner thread dropped the stream)");
    }

    #[test]
    fn wsola_pipeline_integration() {
        let mut wsola = crate::audio::wsola::WsolaProcessor::new();
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
}

