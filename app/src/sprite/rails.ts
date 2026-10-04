/**
 * Light particles running along the wire rails traced from `gfx/bg_wires.svg`.
 *
 * Two facts make this much simpler than the sprite-sheet animations:
 *
 *  - The rails are code, not skin assets. They belong to one piece of artwork,
 *    so they live here rather than in `public/sprite/` and need no zip entry,
 *    no manifest asset list, and no load-path plumbing.
 *  - Every rail lies entirely inside the player silhouette (checked against the
 *    plate alpha over 3,793 sample points). That is why there is no
 *    `destination-in` plate mask here the way `drawAnimations()` needs one —
 *    a plain `screen` draw over the plate cannot leak past the edge. If a
 *    future skin's rails ever cross the silhouette, that mask has to be added.
 *
 * Everything except `makeSpotCanvas` is DOM-free so `rails.test.ts` can run it
 * under plain node.
 */

/** viewBox of the traced source vector; the rails below are in these units. */
export const RAIL_VIEWBOX = { w: 818.182, h: 1123.636 };

/** Sprite drawn per particle, as a multiple of its bead diameter. */
const SPOT_SCALE = 3.6;

/** Rail path data, verbatim from `gfx/bg_wires.svg` (viewBox units). */
export const RAIL_PATHS: readonly string[] = [
  `M304.404,408.234v-22.874c0-4.451,2.497-8.526,6.463-10.547l57.364-29.232c1.725-.879,2.811-2.652,2.811-4.588v-.674`,
  `M318.002,408.234v-22.689c0-3.626,2.028-6.948,5.254-8.604l61.374-31.51c1.533-.787,2.497-2.366,2.497-4.089v-1.023`,
  `M330.281,408.234v-19.528c0-3.52,1.998-6.735,5.155-8.293l63.948-31.563c2.032-1.003,3.318-3.072,3.318-5.338v-3.193`,
  `M343.878,408.234v-17.039c0-3.155,1.829-6.025,4.69-7.356l63.943-29.757c2.31-1.075,3.787-3.392,3.787-5.94v-7.823`,
  `M356.831,408.234v-12.483c0-2.517,1.432-4.815,3.693-5.923l67.209-32.957c2.494-1.223,4.075-3.759,4.075-6.537v-10.014`,
  `M477.511,402.617v-17.602c0-2.613-1.097-5.105-3.023-6.87l-25.38-23.254c-1.611-1.476-2.528-3.56-2.528-5.745v-8.827`,
  `M214.532,382.191l8.073,5.958c1.667,1.23,2.651,3.179,2.651,5.251v7.301c0,2.891,2.343,5.234,5.234,5.234h4.979c3.243,0,5.872-2.629,5.872-5.872v-22.734c0-1.96.751-3.846,2.1-5.269l13.831-14.6c1.398-1.475,3.679-1.668,5.304-.449l1.17.877c1.739,1.304,2.118,3.757.855,5.526l-3.804,5.326c-.914,1.279-.769,3.032.343,4.144l2.138,2.138c1.273,1.273,3.344,1.253,4.592-.044l15.154-15.752c1.269-1.319,1.977-3.077,1.977-4.907v-5.574`,
  `M178.617,591.809l5.362,6.531c1.065,1.298,1.59,2.956,1.465,4.63l-2.129,28.53c-.138,1.849.886,3.59,2.569,4.368l3.914,1.809c5.634,2.604,11.548,4.553,17.627,5.809l3.691.763c1.644.34,5.171.363,6.438-.738l2.043-2.17c1.166-1.013,1.915-4.206,1.915-5.752v-34.844`,
  `M660.319,409.723v-10.342c0-1.457.51-2.867,1.441-3.987l12.856-15.458h20.668c1.682,0,3.281.736,4.374,2.015l8.05,9.412c.824.963,2.008,1.546,3.274,1.61l6.238.317c1.18.06,2.306.513,3.198,1.288l7.644,6.635`,
  `M653.511,409.723v-12.028c0-1.612.573-3.171,1.618-4.398l14.664-17.238c1.153-1.356,2.843-2.137,4.623-2.137h22.518c1.726,0,3.376.713,4.559,1.97l9.815,10.425c.707.751,1.663,1.219,2.69,1.317l6.651.634c1.353.129,2.626.704,3.617,1.634l7.883,7.396`,
  `M646.702,409.723v-13.903c0-1.64.616-3.221,1.726-4.428l19.605-21.322c1.267-1.378,3.053-2.162,4.925-2.162h27.029c2.37,0,4.622,1.039,6.159,2.844l8.811,10.342c.907,1.065,2.201,1.725,3.596,1.834l6.103.477c1.084.085,2.102.552,2.873,1.319l8.704,8.66`,
  `M639.894,409.723v-16.116c0-1.461.529-2.873,1.49-3.974l21.073-24.143c1.995-2.285,4.88-3.596,7.913-3.596h33.2c1.933,0,3.769.849,5.022,2.321l9.054,10.644c.994,1.169,2.452,1.843,3.987,1.843h4.091c1.496,0,2.927.61,3.962,1.689l10.634,11.077`,
  `M633.085,409.723v-19.787c0-1.726.642-3.389,1.802-4.667l24.869-27.401c1.117-1.231,2.702-1.933,4.364-1.933h2.208c2.042,0,4.005-.789,5.479-2.203l4.184-4.013c1.188-1.14,1.86-2.715,1.86-4.361v-8.487c0-1.974,1.6-3.574,3.574-3.574h3.234c1.974,0,3.574,1.6,3.574,3.574v12.17c0,2.867,2.324,5.191,5.191,5.191h2.553c2.867,0,5.191-2.324,5.191-5.191v-12.17c0-1.974,1.6-3.574,3.574-3.574h4.596c1.974,0,3.574,1.6,3.574,3.574v21.589c0,1.227.437,2.413,1.233,3.347l6.057,7.105c.991,1.162,2.441,1.831,3.968,1.831h4.014c1.556,0,3.048.618,4.149,1.719l12.069,12.069`,
  `M626.447,409.723v-25.156c0-1.555.586-3.053,1.642-4.195l27.439-29.688c1.318-1.426,3.172-2.238,5.115-2.238h3.74c2.057,0,4.001-.94,5.28-2.551l.567-.714c.858-1.082,1.325-2.422,1.325-3.803v-7.238c0-1.611.724-3.136,1.972-4.154l2.799-2.284c1.097-.895,2.47-1.384,3.886-1.384h28.852c1.486,0,2.91.59,3.961,1.641l4.872,4.872c1.061,1.061,1.657,2.5,1.657,4.001v18.447c0,1.409.518,2.769,1.456,3.821l2.651,2.972c.961,1.077,2.336,1.693,3.779,1.693h1.858c1.828,0,3.584.716,4.89,1.996l14.473,14.174`,
  `M726.447,219.17v27.375c0,2.972,1.126,5.834,3.152,8.009l72.942,78.333c1.417,1.521,2.204,3.523,2.204,5.602v33.35c0,2.207-.771,4.345-2.179,6.045l-2.757,3.328`,
  `M545.468,726.234v27.474c0,2.778-1.153,5.432-3.184,7.327l-25.696,21.806c-3.487,2.959-5.481,7.313-5.444,11.886l1.105,157.636c.018,2.603-.921,5.121-2.638,7.077l-26.554,30.23c-6.43,7.321-5.903,18.42,1.192,25.099l45.441,42.771c3.16,2.975,7.371,4.575,11.709,4.45l38.42-1.106c3.107-.089,6.059-1.375,8.241-3.589l8.345-8.466`,
  `M625.596,1072.915l-6.428,7.714c-.796.955-1.231,2.158-1.231,3.401v17.098c0,8.249-6.687,14.936-14.936,14.936h-50.746c-2.379,0-4.629-1.083-6.114-2.942l-6.718-8.414c-1.218-1.525-3.099-2.366-5.047-2.255l-8.812.5c-1.042.059-2.084-.126-3.042-.54l-5.484-2.372c-1.609-.696-3.423-.75-5.071-.152l-28.922,10.498c-2.759,1.001-5.845.394-8.018-1.578l-27.167-24.649c-1.441-1.307-2.263-3.163-2.263-5.108v-5.103c0-3.178-1.658-6.127-4.373-7.779l-4.27-3.77c-1.6-1.413-2.485-3.467-2.411-5.601l.424-12.237c.086-2.486,1.262-4.808,3.216-6.348l7.932-6.254c2.427-1.913,3.33-5.181,2.232-8.069l-2.621-6.89c-6.846-17.998-.422-37.697,13.914-50.554l18.39-18.174c2.119-2.094,3.312-4.95,3.312-7.929v-158.512c0-6.499,2.944-12.649,8.006-16.725l22.32-17.971c1.812-1.458,2.865-3.659,2.865-5.985v-14.918`,
  `M619.34,1067.456l-6.313,7.262c-1.28,1.472-1.985,3.357-1.985,5.307v16.56c0,6.544-5.275,11.866-11.819,11.923l-42.388.372c-2.491.022-4.815-1.247-6.145-3.353l-4.635-7.343c-1.745-2.765-4.82-4.403-8.088-4.309l-10.562.303c-1.29.037-2.569-.239-3.729-.805l-3.684-1.798c-1.519-.741-3.266-.865-4.875-.345l-27.397,8.853c-3.731,1.206-7.82.402-10.817-2.127l-20.977-17.699c-1.776-1.499-2.801-3.704-2.801-6.028v-5.123c0-1.695-.82-3.286-2.201-4.269l-6.147-4.375c-2.008-1.429-3.052-3.859-2.707-6.299l1.302-9.204c.278-1.963,1.322-3.737,2.903-4.933l9.659-7.305c1.803-1.364,2.455-3.774,1.587-5.861l-4.581-11.001c-5.793-13.912-2.722-29.939,7.803-40.724l24.531-25.137c1.61-1.65,2.517-3.86,2.531-6.166l.924-161.244c.043-5.45,2.487-10.603,6.679-14.085l22.805-18.946c1.586-1.318,2.503-3.272,2.503-5.334v-17.988`,
  `M613.085,1061.996l-6.828,7.562c-1.357,1.503-2.108,3.456-2.108,5.481v19.512c0,3.442-2.751,6.253-6.192,6.328l-35.819.779c-3.03.066-5.824-1.631-7.163-4.35l-4.49-9.121c-.908-1.843-2.793-3.002-4.848-2.978l-16.464.19c-1.468.017-2.897-.468-4.051-1.375l-1.96-1.54c-1.721-1.352-4.008-1.746-6.082-1.049l-29.983,10.075c-3.565,1.021-7.404.073-10.084-2.491l-13.901-13.299c-1.554-1.292-2.453-3.209-2.453-5.231v-3.914c0-1.643-.742-3.197-2.02-4.23l-6.823-5.516c-1.938-1.567-2.834-4.087-2.321-6.526l.553-2.628c.413-1.963,1.684-3.637,3.463-4.564l7.489-3.9c2.106-1.097,3.525-3.173,3.783-5.534l.752-6.901c.151-1.381-.1-2.776-.723-4.019l-5.775-11.531c-4.566-9.116-3.003-20.108,3.923-27.59l29.093-31.429c1.402-1.515,2.187-3.5,2.2-5.565l1.007-161.459c.035-5.085,2.273-9.905,6.136-13.213l23.375-20.019c1.355-1.16,2.134-2.854,2.134-4.638v-21.079`,
  `M607.128,1057.936l-8.309,9.476c-.898,1.024-1.393,2.339-1.393,3.701v20.526c0,1.974-1.6,3.574-3.574,3.574h-1.049c-1.932,0-3.664-1.19-4.357-2.992l-5.305-13.793c-.719-1.869-2.552-3.069-4.553-2.98h0c-2.588.115-4.828,1.836-5.606,4.307l-3.479,11.051c-.603,1.914-2.378,3.216-4.384,3.216h-1.231c-1.852,0-3.522-1.111-4.239-2.818l-4.367-10.411c-1.064-2.537-3.59-4.149-6.34-4.045l-14.72.557c-.636.024-1.27-.086-1.862-.322l-6.893-2.757c-1.57-.628-3.305-.709-4.927-.23l-34.356,10.14c-2.367.699-4.926.104-6.743-1.565l-9.304-8.548c-1.915-1.759-2.861-4.335-2.541-6.915l5.182-41.751c.251-2.024-.319-4.065-1.583-5.666l-2.445-3.097c-7.098-8.991-6.676-21.791.999-30.296l27.967-30.988c1.292-1.432,2.009-3.291,2.011-5.22l.175-159.918c0-5.165,2.219-10.082,6.093-13.498l24.707-21.793c1.521-1.341,2.392-3.271,2.392-5.299v-23.347`,
  `M603.213,1053l-11.995,11.128c-3.364,3.121-7.772,4.874-12.361,4.916l-36.976.338c-6.84.062-13.437-2.531-18.404-7.234l-45.287-42.889c-8.892-8.421-9.456-22.394-1.272-31.505l28.11-31.292c.967-1.077,1.497-2.477,1.484-3.924l-1.404-159.899c-.04-5.73,2.52-11.169,6.961-14.79l24.604-20.064c1.661-1.508,2.608-3.647,2.608-5.89v-25.66`,
];

export type Pt = { x: number; y: number };

/** A flattened rail plus its cumulative arc length, so `s` means distance. */
export type Rail = {
  xs: number[];
  ys: number[];
  /** `cum[i]` is the arc length from the start to point `i`; `cum[0] === 0`. */
  cum: number[];
  length: number;
};

export type FlowParticle = { x: number; y: number; alpha: number; size: number };

export type FlowOptions = {
  /** Artboard width the rails scale into (skin.json canvas width). */
  artW?: number;
  /** Artboard px of rail per particle — this is what makes density vary. */
  pxPerParticle?: number;
  /** Artboard px/s, randomised per rail. */
  speed?: [number, number];
  /** Artboard px over which a particle fades in and out at a rail end. */
  fadePx?: number;
  /** Bead diameter in artboard px. */
  corePx?: [number, number];
  /** Pulsation rate in Hz. */
  pulseHz?: [number, number];
  /** Peak alpha of a fully faded-in particle. */
  gain?: number;
  seed?: number;
};

const TOKEN = /[a-zA-Z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g;

function isCommand(t: string): boolean {
  return t.length === 1 && t >= "A" && t <= "z";
}

/**
 * Flatten an SVG path into an arc-length-parameterised polyline.
 *
 * Only the commands the traced file actually uses are supported: `M/m L/l
 * H/h V/v C/c`. There are no arcs, quadratics or multiple subpaths in it, and
 * adding the rest would be dead code for artwork that does not exist.
 *
 * Sampling raw curve parameter `t` would make a particle visibly surge through
 * tight corners, because `t` is not proportional to distance. Every consumer
 * therefore addresses a rail by arc length, via `railAt`.
 */
export function parsePath(d: string, scale = 1): Rail {
  const tokens = d.match(TOKEN) ?? [];
  const xs: number[] = [];
  const ys: number[] = [];
  let i = 0;
  let cx = 0;
  let cy = 0;
  let cmd = "";

  const num = (): number => Number(tokens[i++]);
  const push = (x: number, y: number) => {
    xs.push(x * scale);
    ys.push(y * scale);
  };

  while (i < tokens.length) {
    if (isCommand(tokens[i])) cmd = tokens[i++];
    switch (cmd) {
      case "M":
      case "m": {
        const x = num();
        const y = num();
        cx = cmd === "M" ? x : cx + x;
        cy = cmd === "M" ? y : cy + y;
        push(cx, cy);
        // A repeated coordinate pair after a moveto is an implicit lineto.
        cmd = cmd === "M" ? "L" : "l";
        break;
      }
      case "L":
      case "l": {
        const x = num();
        const y = num();
        cx = cmd === "L" ? x : cx + x;
        cy = cmd === "L" ? y : cy + y;
        push(cx, cy);
        break;
      }
      case "H":
      case "h": {
        const x = num();
        cx = cmd === "H" ? x : cx + x;
        push(cx, cy);
        break;
      }
      case "V":
      case "v": {
        const y = num();
        cy = cmd === "V" ? y : cy + y;
        push(cx, cy);
        break;
      }
      case "C":
      case "c": {
        const rel = cmd === "c";
        const x1 = num();
        const y1 = num();
        const x2 = num();
        const y2 = num();
        const x3 = num();
        const y3 = num();
        const p1x = rel ? cx + x1 : x1;
        const p1y = rel ? cy + y1 : y1;
        const p2x = rel ? cx + x2 : x2;
        const p2y = rel ? cy + y2 : y2;
        const p3x = rel ? cx + x3 : x3;
        const p3y = rel ? cy + y3 : y3;
        // Scale the flattening with the curve so short elbows cost nothing and
        // long sweeps stay smooth enough for a moving bead.
        const poly =
          Math.hypot(p1x - cx, p1y - cy) +
          Math.hypot(p2x - p1x, p2y - p1y) +
          Math.hypot(p3x - p2x, p3y - p2y);
        const steps = Math.max(2, Math.min(48, Math.round(poly / 6)));
        for (let k = 1; k <= steps; k++) {
          const t = k / steps;
          const m = 1 - t;
          const a = m * m * m;
          const b = 3 * m * m * t;
          const c = 3 * m * t * t;
          const e = t * t * t;
          push(a * cx + b * p1x + c * p2x + e * p3x, a * cy + b * p1y + c * p2y + e * p3y);
        }
        cx = p3x;
        cy = p3y;
        break;
      }
      default:
        // Unknown command: stop rather than spin on unparsed numbers.
        i = tokens.length;
        break;
    }
  }

  const cum: number[] = new Array(xs.length);
  let acc = 0;
  cum[0] = 0;
  for (let k = 1; k < xs.length; k++) {
    acc += Math.hypot(xs[k] - xs[k - 1], ys[k] - ys[k - 1]);
    cum[k] = acc;
  }
  return { xs, ys, cum, length: acc };
}

/** Point at arc length `s` along the rail, written into `out`. */
export function railAt(rail: Rail, s: number, out: Pt): Pt {
  const { xs, ys, cum, length } = rail;
  const t = s <= 0 ? 0 : s >= length ? length : s;
  // Largest index whose cumulative length is still at or below `t`.
  let lo = 0;
  let hi = cum.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (cum[mid] <= t) lo = mid;
    else hi = mid - 1;
  }
  const j = Math.min(lo + 1, xs.length - 1);
  const span = cum[j] - cum[lo];
  const f = span > 0 ? (t - cum[lo]) / span : 0;
  out.x = xs[lo] + (xs[j] - xs[lo]) * f;
  out.y = ys[lo] + (ys[j] - ys[lo]) * f;
  return out;
}

function smoothstep(t: number): number {
  if (t <= 0) return 0;
  if (t >= 1) return 1;
  return t * t * (3 - 2 * t);
}

/**
 * Visibility envelope of a bead at normalised position `s` along its rail.
 *
 * `fade` is the fade length as a *fraction* of the rail. It is zero at both
 * ends and one in the middle, so a bead fades in while already moving and is
 * fully dissolved strictly before the rail end — it never arrives and then
 * fades. Exported so the envelope can be tested without a canvas.
 */
export function flowAlpha(s: number, fade: number, pulse = 1): number {
  if (fade <= 0) return pulse;
  // Both factors rise 0→1: one as the bead enters, one as the distance to the
  // exit end grows. Their product is 0 at both ends and 1 in the middle.
  return smoothstep(s / fade) * smoothstep((1 - s) / fade) * pulse;
}

/** Small deterministic PRNG, so the layout is reproducible and testable. */
function mulberry32(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export type RailState = {
  rail: Rail;
  /** Fixed for the rail's lifetime: each rail runs one way only. */
  dir: 1 | -1;
  speed: number;
  /** Fade length as a fraction of this rail — a constant *distance*, so a
   *  short rail does not lose most of itself to the fade. */
  fade: number;
  /** Half-open range of this rail's particles in `WireFlow.slots`. */
  from: number;
  to: number;
};

type Slot = {
  rail: number;
  /** Normalised position along the rail, 0..1. */
  s: number;
  size: number;
  phase: number;
  rate: number;
};

/**
 * The particle flow: several beads per rail, each rail running in one fixed
 * direction, all independent of playback (this is ambient artwork, not a
 * meter).
 */
export class WireFlow {
  readonly rails: RailState[];
  private slots: Slot[] = [];
  private clock = 0;
  private gain: number;
  private pt: Pt = { x: 0, y: 0 };
  private buf: FlowParticle[] = [];

  constructor(opts: FlowOptions = {}) {
    const artW = opts.artW ?? 1500;
    const scale = artW / RAIL_VIEWBOX.w;
    const perParticle = opts.pxPerParticle ?? 175;
    const [sLo, sHi] = opts.speed ?? [45, 95];
    const fadePx = opts.fadePx ?? 45;
    const [cLo, cHi] = opts.corePx ?? [4, 6];
    const [pLo, pHi] = opts.pulseHz ?? [0.12, 0.4];
    this.gain = opts.gain ?? 0.85;
    const rng = mulberry32(opts.seed ?? 0x5eed);

    this.rails = RAIL_PATHS.map((d, idx) => {
      const rail = parsePath(d, scale);
      const count = Math.max(1, Math.round(rail.length / perParticle));
      const st: RailState = {
        rail,
        dir: rng() < 0.5 ? -1 : 1,
        speed: sLo + rng() * (sHi - sLo),
        fade: rail.length > 0 ? Math.min(0.35, fadePx / rail.length) : 0,
        from: this.slots.length,
        to: 0,
      };
      for (let i = 0; i < count; i++) {
        // Even spacing plus jitter: irregular gaps without bunching at the ends.
        const jitter = (rng() - 0.5) * (0.7 / count);
        this.slots.push({
          rail: idx,
          s: (i + 0.5) / count + jitter,
          size: cLo + rng() * (cHi - cLo),
          phase: rng() * Math.PI * 2,
          rate: pLo + rng() * (pHi - pLo),
        });
      }
      st.to = this.slots.length;
      return st;
    });
  }

  /** Number of live beads. */
  get count(): number {
    return this.slots.length;
  }

  update(dtSec: number): void {
    for (const st of this.rails) {
      const step = (st.speed * dtSec) / st.rail.length;
      for (let k = st.from; k < st.to; k++) {
        const s = this.slots[k].s + st.dir * step;
        // Positive modulo — `dir` is negative on half the rails.
        this.slots[k].s = s - Math.floor(s);
      }
    }
    this.clock += dtSec;
  }

  /**
   * Alpha of a bead at normalised position `s`.
   *
   * Derived from position, not from a spawn timer: that is what makes a bead
   * fade in *while already moving* and reach zero opacity strictly before the
   * rail end, rather than arriving, stopping, and dissolving. Both sides of the
   * wrap are transparent, so the seam is invisible.
   */
  alphaAt(st: RailState, s: number, slot: Slot): number {
    const pulse = 0.75 + 0.25 * Math.sin(this.clock * 2 * Math.PI * slot.rate + slot.phase);
    return flowAlpha(s, st.fade, pulse);
  }

  /** Fills and returns the currently visible beads, in draw order. */
  particles(): FlowParticle[] {
    const out = this.buf;
    out.length = 0;
    for (const slot of this.slots) {
      const st = this.rails[slot.rail];
      const alpha = this.alphaAt(st, slot.s, slot);
      if (alpha <= 0.004) continue;
      railAt(st.rail, slot.s * st.rail.length, this.pt);
      out.push({ x: this.pt.x, y: this.pt.y, alpha, size: slot.size });
    }
    return out;
  }

  draw(ctx: CanvasRenderingContext2D, spot: HTMLCanvasElement): void {
    const list = this.particles();
    if (list.length === 0) return;
    ctx.save();
    // The rails are mid-grey (~95/255); `screen` lifts them to ~230 at full
    // alpha instead of clipping to a hard white disc.
    ctx.globalCompositeOperation = "screen";
    for (const p of list) {
      const d = p.size * SPOT_SCALE;
      ctx.globalAlpha = p.alpha * this.gain;
      ctx.drawImage(spot, p.x - d / 2, p.y - d / 2, d, d);
    }
    ctx.restore();
  }
}

/**
 * The bead sprite: a white core inside a soft falloff, rendered once.
 *
 * Per particle this is a single `drawImage` with `globalAlpha`, which is what
 * keeps ~48 additive sprites free. Drawn at `SPOT_SCALE ×` the bead diameter,
 * so the solid centre is the requested size while the halo spills a few px past
 * the ~10 px wire.
 */
export function makeSpotCanvas(px = 64): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = px;
  c.height = px;
  const g = c.getContext("2d")!;
  const r = px / 2;
  const grad = g.createRadialGradient(r, r, 0, r, r, r);
  grad.addColorStop(0.0, "rgba(255,255,255,1)");
  grad.addColorStop(0.14, "rgba(255,255,255,0.95)");
  grad.addColorStop(0.26, "rgba(255,255,255,0.62)");
  grad.addColorStop(0.42, "rgba(255,255,255,0.28)");
  grad.addColorStop(0.62, "rgba(255,255,255,0.09)");
  grad.addColorStop(0.82, "rgba(255,255,255,0.02)");
  grad.addColorStop(1.0, "rgba(255,255,255,0)");
  g.fillStyle = grad;
  g.fillRect(0, 0, px, px);
  return c;
}