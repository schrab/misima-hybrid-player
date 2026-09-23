import type { SpectrumBand, SkinManifestV2, XY } from "./types";
import { loadImage } from "./layout";

/**
 * Irregular spectrum pieces (not a rectangular stack).
 * Segments may overlap vertically and differ in size.
 * Each piece has `reveal` (0..1) — it is drawn when band energy >= reveal.
 */
export function drawSpectrumSegments(
  ctx: CanvasRenderingContext2D,
  images: Map<string, HTMLImageElement>,
  bands: SpectrumBand[],
  energies: Float32Array | number[],
) {
  for (let b = 0; b < bands.length; b++) {
    const band = bands[b];
    const e = Math.min(1, Math.max(0, energies[b] ?? 0));
    // Sort by reveal so drawing order is deterministic (bottom-most energy first)
    const segs = [...band.segments].sort((s1, s2) => (s1.reveal ?? 0) - (s2.reveal ?? 0));
    for (const seg of segs) {
      const t = seg.reveal ?? 0;
      if (e < t) continue;
      const img = images.get(seg.image);
      if (!img) continue;
      const w = seg.size?.w ?? img.width;
      const h = seg.size?.h ?? img.height;
      ctx.drawImage(img, seg.origin.x, seg.origin.y, w, h);
    }
  }
}

/** Clip helper if a band region must stay clean. */
export function clipSpectrumRegion(
  ctx: CanvasRenderingContext2D,
  origin: XY,
  size: { w: number; h: number },
  fn: () => void,
) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(origin.x, origin.y, size.w, size.h);
  ctx.clip();
  fn();
  ctx.restore();
}

export function drawWaterfall(
  ctx: CanvasRenderingContext2D,
  spec: SkinManifestV2["visuals"]["waterfall"],
  bands: number[],
  time: number,
) {
  const { x, y } = spec.origin;
  const { w, h } = spec.size;
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.fillStyle = "rgba(5,12,14,0.55)";
  ctx.fillRect(x, y, w, h);

  const rows = 18;
  const cols = 24;
  for (let r = rows - 1; r >= 0; r--) {
    const depth = r / (rows - 1);
    const yy = y + 18 + depth * (h - 36);
    const scale = 0.45 + depth * 0.55;
    const wob = Math.sin(time * 0.002 + r * 0.35) * 4 * (1 - depth);
    ctx.beginPath();
    for (let c = 0; c <= cols; c++) {
      const t = c / cols;
      const bi = Math.floor(t * Math.max(1, bands.length - 1));
      const e = bands[bi] ?? 0;
      const px = x + 12 + t * (w - 24) * scale + (w * (1 - scale)) / 2;
      const py = yy - e * 36 * scale + wob;
      if (c === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    const a = 0.15 + depth * 0.55;
    ctx.strokeStyle = spec.color
      ? hexToRgba(spec.color, a)
      : `rgba(61,255,181,${a})`;
    ctx.lineWidth = 1;
    ctx.stroke();
  }
  ctx.restore();
}

function hexToRgba(hex: string, a: number): string {
  const h = hex.replace("#", "");
  const r = parseInt(h.slice(0, 2), 16);
  const g = parseInt(h.slice(2, 4), 16);
  const b = parseInt(h.slice(4, 6), 16);
  return `rgba(${r},${g},${b},${a})`;
}

export async function loadSpectrumSheet(url: string): Promise<HTMLImageElement> {
  return loadImage(url);
}

export function mapWaterfallOrigin(spec: SkinManifestV2["visuals"]["waterfall"]): XY {
  return spec.origin;
}
