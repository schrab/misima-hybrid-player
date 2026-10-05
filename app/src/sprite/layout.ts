export type { XY, Size, FaderDef, ButtonDef, SkinManifestV2, PlaylistRow, AudioParams, BackgroundDef } from "./types";

import type { FaderDef, XY } from "./types";

/** Map fader value (in range) to knob TOP-LEFT Y (origin = max value). */
export function faderValueToY(
  origin: XY,
  travel: number,
  range: [number, number],
  value: number,
  curve?: "log",
  stops?: number[],
): number {
  const idx = nearestStop(stops, value);
  // A stopped fader's travel is index-spaced, so the log/linear norm math is
  // skipped entirely — computing it first would be a per-frame dead store.
  const n = idx >= 0 && stops ? idx / (stops.length - 1) : valueToNorm(range, value, curve);
  return origin.y + (1 - n) * travel;
}

/**
 * Index of the stop nearest `value`, or -1 when the fader is unstopped.
 *
 * Stops are spaced evenly by index, not by value: the shimmer's shift fader
 * runs -12, 7, 12, 19, 24 semitones, and a value-proportional map would cramp
 * the octave-down stop into the bottom 4% of the slot.
 */
function nearestStop(stops: number[] | undefined, value: number): number {
  if (!stops || stops.length < 2) return -1;
  let best = 0;
  let bestDist = Infinity;
  for (let i = 0; i < stops.length; i++) {
    const d = Math.abs(stops[i] - value);
    if (d < bestDist) {
      bestDist = d;
      best = i;
    }
  }
  return best;
}

/** The stop at a travel slot, or null when the fader is unstopped. */
function normToStop(stops: number[] | undefined, n: number): number | null {
  if (!stops || stops.length < 2) return null;
  return stops[Math.min(stops.length - 1, Math.max(0, Math.round(n * (stops.length - 1))))];
}

/**
 * A log fader needs lo > 0; anything else silently falls back to linear.
 * Legacy: the tempo fader's [0.5, 2] range predates the `curve` field and was
 * always log — keep matching it so old skins that never set `curve` keep
 * their travel.
 */
function isLog(curve: "log" | undefined, lo: number, hi: number): boolean {
  if (curve === "log") return lo > 0;
  return lo > 0 && Math.abs(hi - 2) < 0.01 && Math.abs(lo - 0.5) < 0.01;
}

function valueToNorm(range: [number, number], value: number, curve?: "log"): number {
  const [lo, hi] = range;
  if (isLog(curve, lo, hi)) {
    return Math.min(1, Math.max(0, Math.log(value / lo) / Math.log(hi / lo)));
  }
  return hi === lo ? 1 : (value - lo) / (hi - lo);
}

function normToValue(range: [number, number], n: number, curve?: "log"): number {
  const [lo, hi] = range;
  if (isLog(curve, lo, hi)) {
    return lo * Math.pow(hi / lo, n);
  }
  return lo + n * (hi - lo);
}

export function faderYToValue(
  origin: XY,
  travel: number,
  range: [number, number],
  y: number,
  curve?: "log",
  stops?: number[],
): number {
  let n = 1 - (y - origin.y) / travel;
  n = Math.min(1, Math.max(0, n));
  // A stopped fader never returns a between-stops value: the DSP gets exactly
  // the interval the knob is sitting on.
  return normToStop(stops, n) ?? normToValue(range, n, curve);
}

/**
 * One wheel tick: the same fraction of the range, linear or multiplicative
 * depending on the curve, clamped. Over a 30 Hz..20 kHz range a linear step
 * of the span would jump ~800 Hz per tick and make the bottom half of the
 * sweep unreachable by wheel.
 */
export function faderStepValue(
  range: [number, number],
  value: number,
  fraction: number,
  up: boolean,
  curve?: "log",
  stops?: number[],
): number {
  // A stopped fader moves exactly one stop per tick — a shifter landing
  // between intervals is not a musical interval, it is a tuning error.
  const idx = nearestStop(stops, value);
  if (idx >= 0 && stops) {
    const next = Math.min(stops.length - 1, Math.max(0, idx + (up ? 1 : -1)));
    return stops[next];
  }
  const [lo, hi] = range;
  if (isLog(curve, lo, hi)) {
    const next = value * Math.pow(hi / lo, up ? fraction : -fraction);
    return Math.min(hi, Math.max(lo, next));
  }
  const step = (hi - lo) * fraction * (up ? 1 : -1);
  return Math.min(hi, Math.max(lo, value + step));
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
  // `cache: "reload"` forces revalidation. Without it the browser's
  // heuristic freshness (10% of age since Last-Modified) can serve a
  // days-old manifest against fresh code, and the skin splits in half:
  // new DSP, old faders — the tempo fader silently reverts to its
  // pre-Paulstretch [0.5, 2] range and the shift/tone faders vanish.
  const res = await fetch(url, { cache: "reload" });
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
