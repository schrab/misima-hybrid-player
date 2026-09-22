import type { SkinManifestV2, XY } from "./types";

/** Map fader value (in range) to knob origin Y for vertical travel. */
export function faderValueToY(
  origin: XY,
  travel: number,
  range: [number, number],
  value: number,
): number {
  const [lo, hi] = range;
  const n = hi === lo ? 0 : (value - lo) / (hi - lo);
  return origin.y + (1 - n) * travel;
}

export function faderYToValue(
  origin: XY,
  travel: number,
  range: [number, number],
  y: number,
): number {
  const [lo, hi] = range;
  let n = 1 - (y - origin.y) / travel;
  n = Math.min(1, Math.max(0, n));
  return lo + n * (hi - lo);
}

export function canvasPointFrom(
  ev: MouseEvent | PointerEvent,
  canvas: HTMLCanvasElement,
): XY {
  const rect = canvas.getBoundingClientRect();
  const sx = canvas.width / rect.width;
  const sy = canvas.height / rect.height;
  return { x: (ev.clientX - rect.left) * sx, y: (ev.clientY - rect.top) * sy };
}

export function hitRect(
  x: number,
  y: number,
  origin: XY,
  size: { w: number; h: number },
): boolean {
  return x >= origin.x && y >= origin.y && x < origin.x + size.w && y < origin.y + size.h;
}

export function hitBox(x: number, y: number, box: { x: number; y: number; w: number; h: number }): boolean {
  return x >= box.x && y >= box.y && x < box.x + box.w && y < box.y + box.h;
}

export function faderHit(
  x: number,
  y: number,
  origin: XY,
  travel: number,
  knobW = 22,
  knobH = 28,
): boolean {
  return (
    x >= origin.x - knobW / 2 &&
    x <= origin.x + knobW / 2 &&
    y >= origin.y - knobH / 2 &&
    y <= origin.y + travel + knobH / 2
  );
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

export async function loadJson(url: string): Promise<SkinManifestV2> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`skin load failed: ${url}`);
  return (await res.json()) as SkinManifestV2;
}

export function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`image ${url}`));
    img.src = url;
  });
}
