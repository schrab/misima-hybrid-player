export function faderValueToY(
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

