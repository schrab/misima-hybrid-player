/**
 * Layout smoke checks. Run: npx --yes tsx src/sprite/spectrumLayout.test.ts
 */
import { layoutSpectrumFromPool } from "./spectrumLayout";

let failed = 0;
function assert(cond: boolean, msg: string) {
  if (!cond) {
    console.error("FAIL:", msg);
    failed = 1;
  } else {
    console.log("ok:", msg);
  }
}

const bandLeftX = [370, 411, 451, 496, 546, 602, 655, 703, 750, 805];
const bottomY = 585;
const chips = Array.from({ length: 10 }, (_, i) => `spectrum/chip_${i}.png`);
const chipSizes = chips.map((_, i) => ({ w: 40, h: 20 + i }));

const bands = layoutSpectrumFromPool(
  { bandLeftX, bottomY, chips, chipSizes, segmentsPerBand: 10, overlap: 0.5 },
  0.5,
);

assert(bands.length === 10, "10 bands");
assert(bands[0].segments.length === 10, "10 segments per band");
assert(bands[0].segments[0].image.endsWith("chip_0.png"), "segment 0 is chip_0 (bottom)");
assert(bands[0].segments[9].image.endsWith("chip_9.png"), "segment 9 is chip_9 (top)");
const base = bands[0].segments[0];
const baseBottom = base.origin.y + (base.size?.h ?? 0);
assert(Math.abs(baseBottom - bottomY) < 1.5, `base bottom ≈ ${bottomY} (got ${baseBottom})`);
assert(
  bands[0].segments[0].reveal < bands[0].segments[9].reveal,
  "reveal increases bottom→top",
);
assert(
  Math.abs(base.origin.x - bandLeftX[0]) < 8,
  `x near ${bandLeftX[0]} (got ${base.origin.x})`,
);
for (let b = 0; b < 10; b++) {
  assert(
    Math.abs(bands[b].segments[0].origin.x - bandLeftX[b]) < 8,
    `band ${b} x near ${bandLeftX[b]}`,
  );
}

console.log(failed ? "SMOKE FAILED" : "SMOKE PASSED");
if (failed) {
  throw new Error("spectrum layout smoke failed");
}
