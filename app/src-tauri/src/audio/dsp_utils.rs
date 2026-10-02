//! Shared DSP utility functions used by both the WSOLA and phase-vocoder engines.

/// 4-point Catmull-Rom (cubic Hermite) interpolation.
/// Exact at t=0 and t=1, C¹ smooth continuous derivative across intervals.
#[inline(always)]
pub fn cubic_hermite(y_m1: f32, y0: f32, y1: f32, y2: f32, t: f32) -> f32 {
    let c0 = y0;
    let c1 = 0.5 * (y1 - y_m1);
    let c2 = y_m1 - 2.5 * y0 + 2.0 * y1 - 0.5 * y2;
    let c3 = 0.5 * (y2 - y_m1) + 1.5 * (y0 - y1);
    ((c3 * t + c2) * t + c1) * t + c0
}

/// Read a stereo frame from interleaved samples at integer position `f`.
/// Returns `(0.0, 0.0)` for out-of-range positions, mono→stereo duplication
/// for single-channel sources.
#[inline(always)]
pub fn read_stereo_isize(samples: &[f32], ch: usize, total_frames: usize, f: isize) -> (f32, f32) {
    if f < 0 || f as usize >= total_frames {
        (0.0, 0.0)
    } else {
        let idx = (f as usize) * ch;
        let l = samples[idx];
        let r = if ch > 1 { samples[idx + 1] } else { l };
        (l, r)
    }
}

/// Read a stereo frame at fractional position `f` (truncated to integer).
/// Same semantics as `read_stereo_isize` but accepts `f64`.
#[inline(always)]
pub fn read_stereo_f64(samples: &[f32], ch: usize, total_frames: usize, f: f64) -> (f32, f32) {
    if f < 0.0 || f as usize >= total_frames {
        return (0.0, 0.0);
    }
    let idx = f as usize * ch;
    let l = samples[idx];
    let r = if ch > 1 { samples[idx + 1] } else { l };
    (l, r)
}

/// Read a mono-downmixed frame at integer position `f`.
#[inline(always)]
pub fn read_mono(samples: &[f32], ch: usize, total_frames: usize, f: isize) -> f32 {
    if f < 0 || f as usize >= total_frames {
        0.0
    } else {
        let idx = (f as usize) * ch;
        if ch > 1 {
            (samples[idx] + samples[idx + 1]) * 0.5
        } else {
            samples[idx]
        }
    }
}
