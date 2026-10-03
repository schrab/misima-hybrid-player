//! Misima Hybrid DSP chain, compiled to WASM for an `AudioWorklet`.
//!
//! This crate contains no DSP of its own. Every algorithm lives in
//! `app/src-tauri/src/audio/` and is `#[path]`-included below, so the web and
//! desktop builds run literally the same code and a DSP fix lands on both
//! platforms at once. `player.rs` and `decoder.rs` are deliberately absent:
//! both are platform-bound (cpal, tauri, parking_lot, Symphonia) and their
//! browser equivalents are the `AudioContext` and `decodeAudioData`.
//!
//! The module tree shape must match `src-tauri`'s, because the shared files
//! refer to each other through `use crate::audio::…` paths.

mod audio {
    #[path = "../../../src-tauri/src/audio/clouds_reverb.rs"]
    pub mod clouds_reverb;
    #[path = "../../../src-tauri/src/audio/dsp_utils.rs"]
    pub mod dsp_utils;
    #[path = "../../../src-tauri/src/audio/eq.rs"]
    pub mod eq;
    #[path = "../../../src-tauri/src/audio/phase_vocoder.rs"]
    pub mod phase_vocoder;
    #[path = "../../../src-tauri/src/audio/spectrum.rs"]
    pub mod spectrum;
    #[path = "../../../src-tauri/src/audio/wsola.rs"]
    pub mod wsola;
    #[path = "../../../src-tauri/src/audio/reverb_mix.rs"]
    pub mod reverb_mix;
    #[path = "../../../src-tauri/src/audio/stretcher.rs"]
    pub mod stretcher;
}

mod processor;

#[cfg(target_arch = "wasm32")]
mod bindings;

pub use processor::{DspProcessor, Params, SPECTRUM_BINS, WAVEFORM_POINTS};

/// Frames the worklet asks for per `process()` call — the spec's fixed render
/// quantum. The processor's scratch is sized well above this so a refill or a
/// tail from the stretcher always has somewhere to land.
pub const QUANTUM: usize = 128;
