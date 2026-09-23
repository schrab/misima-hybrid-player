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
  /** Vertical nest factor 0..1 (default 0.5). */
  overlap?: number;
};

/**
 * Build 10 bands from a chip pool.
 *
 * Naming (as created by artist):
 *   chip_0 = BOTTOM / large base (first to light)
 *   chip_9 = TOP / small tip (last to light)
 * Pieces nest with vertical overlap (not a grid).
 */
export function layoutSpectrumFromPool(
  auto: SpectrumAutoLayout,
  overlap = 0.5,
): SpectrumBand[] {
  const n = auto.segmentsPerBand ?? auto.chips.length ?? 10;
  void auto.maxHeight;
  const jitter = auto.jitterX ?? [0, 2, -2, 3, -1, 1, -3, 2, -1, 0];
  const bands: SpectrumBand[] = [];
  const chips = auto.chips;

  for (let b = 0; b < auto.bandLeftX.length; b++) {
    const leftX = auto.bandLeftX[b];
    const segments: SpectrumSegment[] = [];
    let cursorY = auto.bottomY;
    // s=0 → chip_0 base at bottomY; s=n-1 → tip at top
    for (let s = 0; s < n; s++) {
      const chipIndex = s % chips.length;
      const path = chips[chipIndex];
      const size = auto.chipSizes?.[chipIndex];
      const h = size?.h ?? 28;
      const w = size?.w ?? 72;
      const topY = cursorY - h;
      const jx = jitter[s % jitter.length] ?? 0;
      segments.push({
        image: path,
        origin: { x: leftX + jx, y: topY },
        size: { w, h },
        reveal: Math.min(1, s / Math.max(1, n - 1)),
      });
      cursorY = topY + h * overlap;
    }
    bands.push({ id: b, segments });
  }
  return bands;
}
