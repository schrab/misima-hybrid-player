export type { XY, Size, FaderDef, ButtonDef, SkinManifestV2, PlaylistRow, AudioParams, BackgroundDef } from "./types";

import type { FaderDef, XY } from "./types";

/** Map fader value (in range) to knob TOP-LEFT Y (origin = max value). */
﻿export function faderValueToY(
  origin: XY,
  travel: number,
  range: [number, number],
  value: number,
): number {
  const n = valueToNorm(range, value);
  return origin.y + (1 - n) * travel;
}

function valueToNorm(range: [number, number], value: number): number {
  const [lo, hi] = range;
  if (lo > 0 && Math.abs(hi - 2) < 0.01 && Math.abs(lo - 0.5) < 0.01) {
    return Math.min(1, Math.max(0, Math.log(value / lo) / Math.log(hi / lo)));
  }
  return hi === lo ? 1 : (value - lo) / (hi - lo);
}

function normToValue(range: [number, number], n: number): number {
  const [lo, hi] = range;
  if (lo > 0 && Math.abs(hi - 2) < 0.01 && Math.abs(lo - 0.5) < 0.01) {
    return lo * Math.pow(hi / lo, n);
  }
  return lo + n * (hi - lo);
}

export function faderYToValue(
  origin: XY,
  travel: number,
  range: [number, number],
  y: number,
): number {
  let n = 1 - (y - origin.y) / travel;
  n = Math.min(1, Math.max(0, n));
  return normToValue(range, n);
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

export function faderHit(x: number, y: number, fader: FaderDef): boolean {
  const HIT_W = 48;
  const HIT_H = 36;
  const cx = fader.origin.x + (fader.knobSize?.w ?? 24) / 2;
  return (
    x >= cx - HIT_W / 2 &&
    x <= cx + HIT_W / 2 &&
    y >= fader.origin.y - HIT_H / 2 &&
    y <= fader.origin.y + fader.travel + HIT_H / 2
  );
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, v));
}

export async function loadJson(url: string): Promise<import("./types").SkinManifestV2> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`skin load failed: ${url}`);
  return (await res.json()) as import("./types").SkinManifestV2;
}

export function loadImage(url: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => reject(new Error(`image ${url}`));
    img.src = url;
  });
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
