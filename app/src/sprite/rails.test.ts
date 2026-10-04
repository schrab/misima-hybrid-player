/**
 * Wire-rail particle flow: geometry, density and the fade envelope.
 * Run: npm test
 */
import { flowAlpha, parsePath, railAt, RAIL_PATHS, RAIL_VIEWBOX, WireFlow } from "./rails";

let failed = 0;
function assert(cond: boolean, msg: string) {
  if (!cond) {
    console.error("FAIL:", msg);
    failed = 1;
  } else {
    console.log("ok:", msg);
  }
}

const pt = { x: 0, y: 0 };

// --- path parsing -----------------------------------------------------------

const line = parsePath("M0,0 L10,0", 1);
assert(line.xs.length === 2, "line has 2 points");
assert(Math.abs(line.length - 10) < 1e-9, "line length is 10");
railAt(line, 0, pt);
assert(pt.x === 0 && pt.y === 0, "railAt(0) is the start");
railAt(line, 5, pt);
assert(Math.abs(pt.x - 5) < 1e-9, "railAt(5) is the midpoint");
railAt(line, 10, pt);
assert(Math.abs(pt.x - 10) < 1e-9, "railAt(length) is the end");
railAt(line, -5, pt);
assert(pt.x === 0, "railAt clamps below the start");
railAt(line, 999, pt);
assert(Math.abs(pt.x - 10) < 1e-9, "railAt clamps past the end");

// H/V and relative forms.
const hv = parsePath("M10,10 h10 v10", 1);
assert(hv.xs.length === 3, "h/v produce 3 points");
assert(
  hv.xs[2] === 20 && hv.ys[2] === 20,
  `relative h/v land at (20,20) — got (${hv.xs[2]},${hv.ys[2]})`,
);

// Scaling is applied to the flattened points.
const scaled = parsePath("M0,0 L10,0", 2);
assert(Math.abs(scaled.length - 20) < 1e-9, "scale multiplies arc length");

// --- the real rails ---------------------------------------------------------

assert(RAIL_PATHS.length === 21, `21 traced rails (got ${RAIL_PATHS.length})`);
const rails = RAIL_PATHS.map((d) => parsePath(d, 1500 / RAIL_VIEWBOX.w));
assert(
  rails.every((r) => r.xs.length > 1 && r.length > 0),
  "every rail flattens to a positive-length polyline",
);

// Cross-check against an independent measurement of the source vector: the
// rails total roughly 9.8k artboard px. A large drift means the parser or the
// viewBox scale is wrong, which would silently slide every bead off its wire.
const total = rails.reduce((a, r) => a + r.length, 0);
assert(
  total > 9500 && total < 10100,
  `total rail length ≈ 9.8k artboard px (got ${Math.round(total)})`,
);

// Every rail stays inside the artboard.
assert(
  rails.every((r) => r.xs.every((x, i) => x >= 0 && x <= 1500 && r.ys[i] >= 0 && r.ys[i] <= 2060)),
  "every rail lies within the 1500x2060 artboard",
);

// Arc-length parameterisation: equal steps of `s` must give equal chords. A
// parser that sampled raw curve `t` would visibly surge through the corners.
{
  const curve = parsePath("M0,0 C0,100 100,0 100,100", 1);
  const n = 12;
  const chords: number[] = [];
  let prev = railAt(curve, 0, { x: 0, y: 0 });
  let px = prev.x;
  let py = prev.y;
  for (let k = 1; k <= n; k++) {
    railAt(curve, (k / n) * curve.length, prev);
    chords.push(Math.hypot(prev.x - px, prev.y - py));
    px = prev.x;
    py = prev.y;
  }
  const min = Math.min(...chords);
  const max = Math.max(...chords);
  assert(
    max / min < 1.02,
    `even arc-length steps give even chords (spread ${(max / min).toFixed(4)})`,
  );
}

// --- the fade envelope ------------------------------------------------------

{
  const fade = 0.12;
  assert(flowAlpha(0, fade) === 0, "alpha is 0 at the rail start");
  assert(flowAlpha(1, fade) === 0, "alpha is 0 at the rail end");
  assert(flowAlpha(0.5, fade) === 1, "alpha peaks at mid-rail");
  // The bead is still fully lit until the last `fade` of the rail, then gone
  // by the time it arrives — it never reaches the end still visible.
  assert(flowAlpha(1 - 2 * fade, fade) === 1, "fully lit well before the rail end");
  assert(flowAlpha(1 - fade, fade) === 1, "lit at the start of the fade-out");
  assert(flowAlpha(1 - fade / 2, fade) < 1, "fading out over the final stretch");
  assert(flowAlpha(0.995, fade) < 0.01, "still invisible right up to the rail end");
  assert(flowAlpha(0.02, fade) > 0 && flowAlpha(0.02, fade) < 1, "fades in over distance");
  assert(flowAlpha(0.5, 0) === 1, "fade 0 means no envelope");
  // Rises monotonically across the entry half only — past mid-rail the other
  // factor takes over and pulls it back down.
  let monotone = true;
  for (let k = 1; k <= 20; k++) {
    const a = flowAlpha(k / 40 - 1 / 40, fade);
    const b = flowAlpha(k / 40, fade);
    if (b < a - 1e-12) monotone = false;
  }
  assert(monotone, "the fade-in half rises monotonically");
  assert(flowAlpha(0.5, fade, 0.5) === 0.5, "pulse scales the envelope");
}

// --- the flow ---------------------------------------------------------------

const flow = new WireFlow();
assert(flow.rails.length === 21, "flow builds all 21 rails");
assert(flow.count > 25 && flow.count < 80, `sane particle count (${flow.count})`);

// Density scales with rail length, so the long bottom rails carry more beads
// than the short top ones instead of every rail looking the same.
{
  const byLength = flow.rails
    .map((st) => ({ len: st.rail.length, n: st.to - st.from }))
    .sort((a, b) => a.len - b.len);
  const shortest = byLength[0];
  const longest = byLength[byLength.length - 1];
  assert(
    longest.n > shortest.n,
    `longest rail (${Math.round(longest.len)}px) has more beads (${longest.n}) than the shortest (${shortest.n})`,
  );
  // Roughly one bead per 175 px of rail.
  const perPx = flow.count / flow.rails.reduce((a, r) => a + r.rail.length, 0);
  assert(perPx > 1 / 220 && perPx < 1 / 140, `~1 bead per 175 px (got 1 per ${Math.round(1 / perPx)})`);
}

// Directions vary across rails, and never flip at runtime.
{
  const dirs = new Set(flow.rails.map((st) => st.dir));
  assert(dirs.size === 2, "rails run in both directions");
  assert(flow.rails.every((st) => st.dir === 1 || st.dir === -1), "direction is ±1");
  const before = flow.rails.map((st) => st.dir);
  for (let k = 0; k < 200; k++) flow.update(1 / 60);
  assert(
    flow.rails.every((st, i) => st.dir === before[i]),
    "direction is fixed for a rail's lifetime",
  );
}

// Bead sizes land in the requested 4..6 artboard px.
{
  const sizes = flow.particles().map((p) => p.size);
  assert(sizes.length > 0, "some beads are visible at t=0");
  assert(
    sizes.every((s) => s >= 4 && s <= 6),
    "bead diameter stays within 4..6 artboard px",
  );
}

// --- simulation -------------------------------------------------------------

{
  let finite = true;
  let inRange = true;
  for (let frame = 0; frame < 600; frame++) {
    flow.update(1 / 60);
    for (const p of flow.particles()) {
      if (!Number.isFinite(p.x) || !Number.isFinite(p.y)) finite = false;
      if (p.x < -50 || p.x > 1550 || p.y < -50 || p.y > 2110) inRange = false;
      if (p.alpha < 0 || p.alpha > 1) inRange = false;
    }
  }
  assert(finite, "10s of stepping never produces NaN (wrap is a positive modulo)");
  assert(inRange, "beads stay finite, in range, with alpha in 0..1");

  // Beads must actually be spread along the rails, not clumped at one end.
  const list = flow.particles();
  assert(list.length > flow.count * 0.4, `most beads are visible mid-run (${list.length}/${flow.count})`);
}

// A stalled tab must not teleport beads: dt is clamped by the caller, and a
// single huge step still has to land them on their rails.
{
  const f2 = new WireFlow();
  f2.update(5);
  assert(
    f2.particles().every((p) => Number.isFinite(p.x) && Number.isFinite(p.y)),
    "a large dt keeps beads finite",
  );
}

// artW drives the scale, so a different artboard rescales the whole flow.
{
  const small = new WireFlow({ artW: 750 });
  const big = new WireFlow({ artW: 1500 });
  const ls = small.rails[0].rail.length;
  const lb = big.rails[0].rail.length;
  assert(Math.abs(ls * 2 - lb) < 1e-6, `artW scales rail length (${Math.round(ls)} vs ${Math.round(lb)})`);
}

console.log(failed ? "SMOKE FAILED" : "SMOKE PASSED");
if (failed) {
  throw new Error("rails smoke failed");
}