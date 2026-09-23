/**
 * Spectrum piece placement helpers.
 *
 * Segments are freeform and may overlap. For large libraries, prefer a small
 * chip pool + auto-stack along known band left-X edges and a shared bottom Y.
 */
import type { SpectrumBand, SpectrumSegment } from "./types";

export type SpectrumAutoLayout = {
  /** Left border X of each band (2× artboard), e.g. 10 values. */
  bandLeftX: number[];
  /** Shared bottom Y for all band stacks (artboard). */
  bottomY: number;
  /** Optional per-band max stack height (defaults to 220). */
  maxHeight?: number;
  /**
   * Pool of chip images (paths). Auto-layout reuses these across bands
   * so you only export a handful of unique organic pieces.
   */
  chips: string[];
  /** Chip natural sizes, same order as chips (or read from images at runtime). */
  chipSizes?: Array<{ w: number; h: number }>;
  /** Optional per-band total segments count (default 10). */
  segmentsPerBand?: number;
  /** Horizontal nudge per segment (organic scatter), default 0. */
  jitterX?: number[];
};

/**
 * Build 10 bands of overlapping pieces from a chip pool.
 * Stack from `bottomY` upward; reveal thresholds are cumulative height / maxHeight.
 * Chips can overlap: next piece y is decreased by less than chip height (`overlap`).
 */
export function layoutSpectrumFromPool(
  auto: SpectrumAutoLayout,
  overlap = 0.35,
): SpectrumBand[] {
  const perBand = auto.segmentsPerBand ?? 10;
  void auto.maxHeight;
  const jitter = auto.jitterX ?? [0, 4, -3, 6, -5, 2, -2, 5, -4, 1];
  const bands: SpectrumBand[] = [];

  for (let b = 0; b < auto.bandLeftX.length; b++) {
    const leftX = auto.bandLeftX[b];
    const segments: SpectrumSegment[] = [];
    // Accumulate upward from bottomY (artboard Y grows downward)
    let cursorY = auto.bottomY;
    for (let s = 0; s < perBand; s++) {
      const chipIndex = (b * 3 + s) % auto.chips.length;
      const path = auto.chips[chipIndex];
      const size = auto.chipSizes?.[chipIndex];
      const h = size?.h ?? 28;
      const w = size?.w ?? 72;
      // Place so the piece's BOTTOM sits on cursorY, then step up with overlap
      const topY = cursorY - h;
      const jx = jitter[s % jitter.length] ?? 0;
      // slight per-band lateral offset so columns aren't identical
      const bandNudge = (b % 3) * 2;
      segments.push({
        image: path,
        origin: { x: leftX + jx + bandNudge, y: topY },
        size: { w, h },
        reveal: Math.min(1, (s / perBand) * 0.95),
      });
      cursorY = topY + h * overlap; // allow overlap into previous piece
    }
    bands.push({ id: b, segments });
  }
  return bands;
}
