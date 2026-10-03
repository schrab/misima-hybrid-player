//! `wasm_bindgen` surface consumed by `src/worklet/dspWorklet.js`.
//!
//! Kept separate from `processor.rs` so the DSP logic stays plain Rust and
//! testable on the host target — these bindings only exist on `wasm32`.

use wasm_bindgen::prelude::*;

use crate::processor::{DspProcessor as Core, Params};

/// One stereo frame of output.
pub const FRAME_LEN: usize = 2;

#[wasm_bindgen]
pub struct DspProcessor {
    core: Core,
    /// Reused every `process()` so the audio thread never allocates.
    out: Vec<f32>,
}

#[wasm_bindgen]
impl DspProcessor {
    /// `sample_rate` comes from the `AudioContext`, so the EQ's biquads and
    /// the reverb's delay scaling are correct for whatever the browser picked.
    #[wasm_bindgen(constructor)]
    pub fn new(sample_rate: f32) -> DspProcessor {
        DspProcessor {
            out: vec![0.0; crate::QUANTUM * FRAME_LEN],
            core: Core::new(sample_rate),
        }
    }

    /// Install a decoded track. `data` is interleaved f32 at the context rate;
    /// the main thread has already resampled it via `decodeAudioData`.
    pub fn load_track(&mut self, data: Vec<f32>, channels: u32) {
        self.core.load_track(data, channels as usize);
    }

    pub fn track_frames(&self) -> usize {
        self.core.track_frames()
    }

    pub fn duration_secs(&self) -> f64 {
        self.core.duration_secs()
    }

    pub fn set_params(&mut self, volume: f32, speed: f32, pitch: f32, reverb: f32, eq: Vec<f32>) {
        let mut bands = [0.0f32; 10];
        for (i, slot) in bands.iter_mut().enumerate() {
            // A short `eq` must not panic on the audio thread.
            *slot = eq.get(i).copied().unwrap_or(0.0);
        }
        self.core.set_params(Params {
            volume,
            speed,
            pitch_semitones: pitch,
            reverb,
            eq: bands,
        });
    }

    pub fn set_playing(&mut self, playing: bool) {
        self.core.set_playing(playing);
    }

    pub fn is_playing(&self) -> bool {
        self.core.is_playing()
    }

    pub fn seek_secs(&mut self, secs: f64) {
        self.core.seek_secs(secs);
    }

    pub fn position_secs(&self) -> f64 {
        self.core.position_secs()
    }

    /// True once, after the track runs out.
    pub fn take_ended(&mut self) -> bool {
        self.core.take_ended()
    }

    /// Render one render quantum and return it, interleaved L,R.
    ///
    /// **Returns the samples rather than writing through a `&mut [f32]`
    /// out-parameter.** wasm-bindgen treats a `&mut [f32]` argument as a return
    /// pointer: the generated JS calls
    /// `wasm.f(in_ptr, len, out)` and never copies the result back, so the
    /// caller's array is left untouched. The DSP then plays a stale buffer —
    /// audible as a metallic, bit-reduced mess — and the visualizer taps read
    /// as permanent silence. Returning a `Vec<f32>` makes wasm-bindgen copy
    /// the data into a fresh `Float32Array` that JS actually receives.
    ///
    /// The cost is one small allocation per 128-frame quantum (~375/s, 1 KB
    /// each), which is far below the worklet's time budget.
    pub fn process(&mut self) -> Vec<f32> {
        let want = crate::QUANTUM * FRAME_LEN;
        self.core.process(&mut self.out[..want]);
        self.out[..want].to_vec()
    }

    /// Spectrum bins from the last tap. See `process` for why this returns
    /// instead of writing through an out-parameter.
    pub fn spectrum(&self) -> Vec<f32> {
        self.core.spectrum().to_vec()
    }

    /// Scope points from the last tap. See `process` for why this returns
    /// instead of writing through an out-parameter.
    pub fn waveform(&self) -> Vec<f32> {
        self.core.waveform().to_vec()
    }
}
