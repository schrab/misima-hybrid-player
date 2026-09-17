//! Decode audio files to interleaved f32 PCM using Symphonia.

use std::fs::File;
use std::path::Path;
use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

#[derive(Debug, Clone)]
pub struct DecodedAudio {
    pub sample_rate: u32,
    pub channels: usize,
    /// interleaved f32
    pub samples: Vec<f32>,
}

impl DecodedAudio {
    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 || self.channels == 0 {
            return 0.0;
        }
        self.samples.len() as f64 / (self.sample_rate as f64 * self.channels as f64)
    }
}

pub fn decode_file(path: &Path) -> anyhow::Result<DecodedAudio> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| anyhow::anyhow!("probe failed: {e}"))?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| anyhow::anyhow!("no audio track"))?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())?;

    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(44_100);
    let mut channels = track
        .codec_params
        .channels
        .map(|c| c.count())
        .unwrap_or(2)
        .max(1);
    let mut samples: Vec<f32> = Vec::new();

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break
            }
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(e.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                if let AudioBufferRef::F32(buf) = &audio_buf {
                    sample_rate = buf.spec().rate;
                    channels = buf.spec().channels.count();
                    append_planar_f32(buf, &mut samples);
                } else {
                    // convert via spec-independent path
                    append_any(&audio_buf, &mut samples);
                    sample_rate = audio_buf.spec().rate;
                    channels = audio_buf.spec().channels.count();
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(e.into()),
        }
    }

    if samples.is_empty() {
        return Err(anyhow::anyhow!("no samples decoded"));
    }
    Ok(DecodedAudio {
        sample_rate,
        channels,
        samples,
    })
}

fn append_planar_f32(
    buf: &symphonia::core::audio::AudioBuffer<f32>,
    out: &mut Vec<f32>,
) {
    let channels = buf.spec().channels.count();
    let frames = buf.frames();
    out.reserve(frames * channels);
    let planes: Vec<&[f32]> = (0..channels).map(|c| buf.chan(c)).collect();
    for i in 0..frames {
        for plane in &planes {
            out.push(plane[i]);
        }
    }
}

fn append_any(buf: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    // Convert to f32 samples using symphonia's typed buffers.
    match buf {
        AudioBufferRef::U8(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i] as f32 / 128.0 - 1.0
        }, out),
        AudioBufferRef::U16(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i] as f32 / 32768.0 - 1.0
        }, out),
        AudioBufferRef::U24(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            let s = b.chan(c)[i].inner() as f32;
            s / 8_388_608.0 - 1.0
        }, out),
        AudioBufferRef::U32(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i] as f32 / 2_147_483_648.0 - 1.0
        }, out),
        AudioBufferRef::S8(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i] as f32 / 128.0
        }, out),
        AudioBufferRef::S16(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i] as f32 / 32768.0
        }, out),
        AudioBufferRef::S24(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i].inner() as f32 / 8_388_608.0
        }, out),
        AudioBufferRef::S32(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i] as f32 / 2_147_483_648.0
        }, out),
        AudioBufferRef::F32(b) => {
            let channels = b.spec().channels.count();
            let frames = b.frames();
            for i in 0..frames {
                for c in 0..channels {
                    out.push(b.chan(c)[i]);
                }
            }
        }
        AudioBufferRef::F64(b) => planar_to_interleaved(b.frames(), b.spec().channels.count(), |c, i| {
            b.chan(c)[i] as f32
        }, out),
    }
}

fn planar_to_interleaved(
    frames: usize,
    channels: usize,
    mut get: impl FnMut(usize, usize) -> f32,
    out: &mut Vec<f32>,
) {
    out.reserve(frames * channels);
    for i in 0..frames {
        for c in 0..channels {
            out.push(get(c, i));
        }
    }
}

/// Generate a short mono sine WAV for tests.
#[cfg(test)]
pub fn write_test_wav(path: &Path, freq: f32, secs: f32, sample_rate: u32) -> std::io::Result<()> {
    let n = (secs * sample_rate as f32) as usize;
    let mut data = Vec::with_capacity(n * 2);
    for i in 0..n {
        let s = (2.0 * std::f32::consts::PI * freq * i as f32 / sample_rate as f32).sin();
        let v = (s * 0.5 * i16::MAX as f32) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }
    let data_len = data.len() as u32;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    let byte_rate = sample_rate * 2;
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(&data);
    std::fs::write(path, wav)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn decodes_generated_wav() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("tone.wav");
        write_test_wav(&path, 440.0, 0.25, 44100).unwrap();
        let audio = decode_file(&path).expect("decode");
        assert_eq!(audio.sample_rate, 44100);
        assert_eq!(audio.channels, 1);
        assert!(audio.samples.len() > 1000);
        assert!(audio.duration_secs() > 0.2);
    }
}
