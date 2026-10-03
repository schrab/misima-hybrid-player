//! Misima Hybrid DSP chain, compiled to WASM for an `AudioWorklet`.
//!
//! This crate contains no DSP of its own. Every algorithm lives in
//! `app/src-tauri/src/audio/` and is `#[path]`-included below, so the web and
//! desktop builds run literally the same code and a DSP fix lands on both
//! platforms at once. `player.rs` and `decoder.rs` are deliberately absent:
//! both are platform-bound (cpal, tauri, parking_lot, Symphonia) and their
//! browser equivalents are the `AudioContext` and `decodeAudioData`.
//!
//! ## Why the modules are declared at the crate root
//!
//! The shared sources refer to each other as `crate::audio::eq::Biquad`, so the
//! module tree has to *look* like `src-tauri`'s. That shape used to be built
//! with an inline `mod audio { … }` block plus
//! `#[path = "../../../src-tauri/src/audio/…"]`.
//!
//! That built fine on Windows and failed on the Linux Pages runner with:
//!
//! ```text
//! couldn't find file `src/audio/../../../src-tauri/src/audio/clouds_reverb.rs`
//! ```
//!
//! An inline module anchors a nested `#[path]` at `src/audio/` rather than at
//! the directory of this file, so the `..` count is only right under one of the
//! two plausible readings of where the anchor is — and the other reading points
//! at a path that does not exist. Declaring each module at the crate root makes
//! the anchor the directory of this file, unambiguously, and the `pub mod audio`
//! shim below restores the `crate::audio::…` paths for the shared code.

#[path = "../../src-tauri/src/audio/clouds_reverb.rs"]
pub mod clouds_reverb;
#[path = "../../src-tauri/src/audio/dsp_utils.rs"]
pub mod dsp_utils;
#[path = "../../src-tauri/src/audio/eq.rs"]
pub mod eq;
#[path = "../../src-tauri/src/audio/phase_vocoder.rs"]
pub mod phase_vocoder;
#[path = "../../src-tauri/src/audio/spectrum.rs"]
pub mod spectrum;
#[path = "../../src-tauri/src/audio/wsola.rs"]
pub mod wsola;
#[path = "../../src-tauri/src/audio/reverb_mix.rs"]
pub mod reverb_mix;
#[path = "../../src-tauri/src/audio/stretcher.rs"]
pub mod stretcher;

/// The module tree shape `src-tauri` has, so the shared sources' own
/// `use crate::audio::…` paths resolve unchanged.
pub mod audio {
    pub use super::{
        clouds_reverb, dsp_utils, eq, phase_vocoder, reverb_mix, spectrum, stretcher, wsola,
    };
}

mod processor;

#[cfg(target_arch = "wasm32")]
mod bindings;

pub use processor::{DspProcessor, Params, SPECTRUM_BINS, WAVEFORM_POINTS};

/// Frames the worklet asks for per `process()` call — the spec's fixed render
/// quantum. The processor's scratch is sized well above this so a refill or a
/// tail from the stretcher always has somewhere to land.
pub const QUANTUM: usize = 128;
