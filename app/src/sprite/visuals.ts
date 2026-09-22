import type { SkinManifestV2, XY } from "./types";
import { loadImage } from "./layout";

/** 10-band spectrum using a cell sheet: row 0 empty … row frames-1 full. */
export function drawSpectrum(
  ctx: CanvasRenderingContext2D,
  sheet: HTMLImageElement,
  spec: SkinManifestV2["visuals"]["spectrum"],
  bands: Float32Array | number[],
) {
  const n = spec.bands;
  const { w: cw, h: ch } = spec.cell;
  const frames = spec.frames;
  const bandW = spec.size.w / n;
  const maxStack = Math.floor(spec.size.h / ch);
  ctx.save();
  ctx.beginPath();
  ctx.rect(spec.origin.x, spec.origin.y, spec.size.w, spec.size.h);
  ctx.clip();
  for (let i = 0; i < n; i++) {
    const energy = Math.min(1, Math.max(0, bands[i] ?? 0));
    const lit = Math.max(energy > 0.02 ? 1 : 0, Math.round(energy * maxStack));
    for (let k = 0; k < lit; k++) {
      const dx = spec.origin.x + i * bandW + (bandW - cw) / 2;
      const dy = spec.origin.y + spec.size.h - (k + 1) * ch;
      const cellE = (k + 1) / maxStack;
      const cellFrame = Math.max(1, Math.floor(cellE * (frames - 1)));
      ctx.drawImage(sheet, i * cw, cellFrame * ch, cw, ch, dx, dy, cw, ch);
    }
  }
  ctx.restore();
}

/** Generative phase / pseudo-3D waterfall inside a rect. */
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
      const bi = Math.floor(t * (bands.length - 1));
      const e = bands[bi] ?? 0;
      const px = x + 12 + t * (w - 24) * scale + (w * (1 - scale)) / 2;
      const py = yy - e * 36 * scale + wob;
      if (c === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    const a = 0.15 + depth * 0.55;
    ctx.strokeStyle = `rgba(61,255,181,${a})`;
    ctx.lineWidth = 1;
    ctx.stroke();
  }
  ctx.strokeStyle = "rgba(94,200,255,0.35)";
  ctx.beginPath();
  ctx.moveTo(x + w / 2, y + 8);
  ctx.lineTo(x + w / 2, y + h - 8);
  ctx.moveTo(x + 8, y + h / 2);
  ctx.lineTo(x + w - 8, y + h / 2);
  ctx.stroke();
  ctx.restore();
}

export async function loadSpectrumSheet(url: string): Promise<HTMLImageElement> {
  return loadImage(url);
}

export function mapWaterfallOrigin(spec: SkinManifestV2["visuals"]["waterfall"]): XY {
  return spec.origin;
}
