pub mod clouds_reverb;
pub mod decoder;
pub mod eq;
pub mod lpf;
pub mod paulstretch;
pub mod phase_vocoder;
pub mod player;
pub mod spectrum;
pub mod wsola;
pub mod dsp_utils;
pub mod reverb_mix;
// The shimmer shifter lands ahead of its consumer — the `reverb_mix` wrapper
// that feeds the cascade loop. `mod audio` is private, so until that wiring
// exists the whole module reads as dead code; drop the allow with it.
#[allow(dead_code)]
pub mod shimmer;
pub mod stretcher;

pub use player::PlayerState;
pub use player::spawn_spectrum_task;
