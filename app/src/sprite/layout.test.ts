/**
 * Unit tests for the fader travel math, especially the log curve the cutoff
 * fader depends on: linear travel across 30..20000 Hz would pile the whole
 * audible band into the last few pixels of the slot, and the legacy [0.5, 2]
 * tempo range must keep its log travel for skins that never set `curve`.
 *
 * Plain `.ts` run by `tsx` (see the `test` script in package.json) — no test
 * framework.
 */

import { faderStepValue, faderValueToY, faderYToValue } from "./layout";
import type { XY } from "./types";

let passed = 0;
let failed = 0;
function check(label: string, cond: boolean) {
  if (!cond) {
    console.error(`FAIL: ${label}`);
    failed++;
    return;
  }
  console.log(`ok: ${label}`);
  passed++;
}

const ORIGIN = { x: 0, y: 100 };
const TRAVEL = 200;
const CUTOFF: [number, number] = [30, 20000];

function main() {
  // Top of travel = max of range = fully open.
  check(
    "top of travel maps to the open cutoff",
    faderYToValue(ORIGIN, TRAVEL, CUTOFF, ORIGIN.y, "log") === 20000,
  );
  check(
    "bottom of travel maps to the closed cutoff",
    faderYToValue(ORIGIN, TRAVEL, CUTOFF, ORIGIN.y + TRAVEL, "log") === 30,
  );

  // Mid travel is the geometric mean, and the mapping round-trips.
  const mid = faderYToValue(ORIGIN, TRAVEL, CUTOFF, ORIGIN.y + TRAVEL / 2, "log");
  check(
    "mid travel is the geometric mean of the range",
    Math.abs(mid - Math.sqrt(30 * 20000)) < 1,
  );
  check(
    "value-to-y round-trips at mid",
    Math.abs(faderValueToY(ORIGIN, TRAVEL, CUTOFF, mid, "log") - (ORIGIN.y + TRAVEL / 2)) < 0.01,
  );

  // Clamping outside travel.
  check(
    "y above origin clamps to max",
    faderYToValue(ORIGIN, TRAVEL, CUTOFF, ORIGIN.y - 50, "log") === 20000,
  );
  check(
    "y below travel clamps to min",
    faderYToValue(ORIGIN, TRAVEL, CUTOFF, ORIGIN.y + TRAVEL + 50, "log") === 30,
  );

  // Legacy: the tempo fader's [0.5, 2] stays log without a curve field.
  const TEMPO: [number, number] = [0.5, 2];
  check(
    "legacy [0.5,2] range keeps log travel",
    faderYToValue(ORIGIN, TRAVEL, TEMPO, ORIGIN.y + TRAVEL / 2) === 1.0,
  );

  // Everything else stays linear.
  const LINEAR: [number, number] = [-12, 12];
  check(
    "linear range is unaffected",
    faderYToValue(ORIGIN, TRAVEL, LINEAR, ORIGIN.y + TRAVEL / 2) === 0,
  );

  // Wheel steps: linear faders step by the span fraction, log faders by the
  // same fraction multiplicatively.
  check(
    "linear wheel steps by span fraction",
    Math.abs(faderStepValue(LINEAR, 0, 0.04, true) - 24 * 0.04) < 1e-9,
  );
  const stepped = faderStepValue(CUTOFF, 1000, 0.04, true, "log");
  check(
    "log wheel steps multiplicatively",
    Math.abs(stepped - 1000 * Math.pow(20000 / 30, 0.04)) < 0.01,
  );
  check(
    "log wheel clamps at the top",
    faderStepValue(CUTOFF, 20000, 0.04, true, "log") === 20000,
  );
  check(
    "log wheel clamps at the floor",
    faderStepValue(CUTOFF, 30, 0.04, false, "log") === 30,
  );

  // Stops quantization (the shimmer's shift fader: -12, 7, 12, 19, 24
  // semitones — octave down, fifth, octave, octave+fifth, two octaves).
  const STOPS = [-12, 7, 12, 19, 24];
  const SHIFT: [number, number] = [-12, 24];
  const SLOT: XY = { x: 0, y: 0 };
  const T = 100;
  check("top of travel is the top stop", faderYToValue(SLOT, T, SHIFT, 0, undefined, STOPS) === 24);
  check(
    "bottom of travel is the bottom stop",
    faderYToValue(SLOT, T, SHIFT, T, undefined, STOPS) === -12,
  );
  check(
    "mid travel snaps to the nearest stop",
    faderYToValue(SLOT, T, SHIFT, T * 0.75, undefined, STOPS) === 7,
  );
  check(
    "a stop maps to its own travel slot",
    faderValueToY(SLOT, T, SHIFT, 19, undefined, STOPS) === T * 0.25,
  );
  check(
    "wheel moves exactly one stop up",
    faderStepValue(SHIFT, 12, 0.1, true, undefined, STOPS) === 19,
  );
  check(
    "wheel moves exactly one stop down",
    faderStepValue(SHIFT, 12, 0.1, false, undefined, STOPS) === 7,
  );
  check(
    "wheel clamps at the top stop",
    faderStepValue(SHIFT, 24, 0.1, true, undefined, STOPS) === 24,
  );
  check(
    "wheel clamps at the bottom stop",
    faderStepValue(SHIFT, -12, 0.1, false, undefined, STOPS) === -12,
  );
  // The knob and the dragged value agree at every stop, not just the ends.
  check(
    "stops round-trip through value and y",
    STOPS.every((v) => {
      const y = faderValueToY(SLOT, T, SHIFT, v, undefined, STOPS);
      return faderYToValue(SLOT, T, SHIFT, y, undefined, STOPS) === v;
    }),
  );

  // No stops: an unstopped fader behaves exactly as before.
  check("no stops keeps linear travel", faderYToValue(SLOT, T, SHIFT, T * 0.75) === -3);
  check(
    "no stops keeps fractional wheel steps",
    Math.abs(faderStepValue(SHIFT, 0, 0.04, true) - 36 * 0.04) < 1e-9,
  );

  console.log(`\n${passed} passed, ${failed} failed`);
  if (failed > 0) {
    throw new Error("fader layout smoke failed");
  }
}

main();
