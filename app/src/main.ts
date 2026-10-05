/**
 * Misima Hybrid — sprite UI entry.
 * Coordinates are Photoshop 2x artboard pixels (see README "Coordinate system").
 */
import type {
  FaderDef,
  PlaylistRow,
  SkinAnimDef,
  SkinManifestV2,
} from "./sprite/types";
import {
  canvasPointFrom,
  faderHit,
  faderStepValue,
  faderValueToY,
  faderYToValue,
  hitRect,
  loadJson,
  loadImage,
} from "./sprite/layout";
import { loadFont, type BitmapFont } from "./sprite/font";
import { makeSpotCanvas, WireFlow } from "./sprite/rails";
import { drawSpectrumSegments } from "./sprite/visuals";
import { layoutSpectrumFromPool, type SpectrumAutoLayout } from "./sprite/spectrumLayout";
import type { AudioParamsInput, Transport } from "./transport";
import { TauriTransport } from "./transportTauri";
import { WebTransport } from "./transportWeb";
import { installDropTarget } from "./web/files";
import { assetUrl } from "./web/base";

// Base path from Vite. On GitHub Pages the app is served from a subpath, so a
// hardcoded "/sprite/" would 404 in production while working fine in dev.
const BASE = assetUrl("sprite/");

/**
 * The demo tracks the web build ships and loads on startup. Music by Wit Chu,
 * used with his permission; the credit and link live in index.html.
 *
 * Kept under `public/` so Vite copies them verbatim into `dist/` and they are
 * fetched same-origin — no CORS, no third-party host in the request path.
 *
 * Only the first is decoded eagerly (that is the one autoplayed). The rest are
 * fetched when the listener reaches them, so the page does not pull 73 MB on
 * every visit.
 */
const STARTUP_TRACKS = [
  { url: assetUrl("music/01 Wit Chu - Now.mp3"), title: "01 Wit Chu - Now" },
  { url: assetUrl("music/02 Wit Chu - In The Loop.mp3"), title: "02 Wit Chu - In The Loop" },
  { url: assetUrl("music/07 Wit Chu - The Joy.mp3"), title: "07 Wit Chu - The Joy" },
  { url: assetUrl("music/10 Wit Chu - Technical Problem.mp3"), title: "10 Wit Chu - Technical Problem" },
  { url: assetUrl("music/Wit Chu - backwards.mp3"), title: "Wit Chu - backwards" },
  { url: assetUrl("music/Wit Chu - da.mp3"), title: "Wit Chu - da" },
  { url: assetUrl("music/Wit Chu - life on bass.mp3"), title: "Wit Chu - life on bass" },
  { url: assetUrl("music/Wit Chu - theme.mp3"), title: "Wit Chu - theme" },
  { url: assetUrl("music/Wit Chu - tribal.mp3"), title: "Wit Chu - tribal" },
  { url: assetUrl("music/Wit Chu - voice of....mp3"), title: "Wit Chu - voice of..." },
];

const canvas = document.getElementById("ui") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

const ART_W = 1500;
const ART_H = 2060;

/** Artboard px the selected playlist row's highlight bar is nudged down. */
const HIGHLIGHT_DY = 2;

/**
 * Platform is chosen once, here. Everything below talks to `transport` and
 * never learns whether it is running inside Tauri or a browser tab.
 */
const isTauri = "__TAURI_INTERNALS__" in window;
const transport: Transport = isTauri ? new TauriTransport() : new WebTransport();

// The desktop shell is a transparent window over the wallpaper; a browser tab
// has nothing behind it, so transparent would render as white behind a dark
// skin. `styles.css` keys off this class.
if (!isTauri) document.documentElement.classList.add("web");

/** Zoom state. On the web this is a CSS size; on the desktop Rust owns it. */
let currentScale = 0.5;

function paintCanvasSize(info: { w: number; h: number }) {
  canvas.style.width = `${info.w}px`;
  canvas.style.height = `${info.h}px`;
}

/**
 * The largest scale at which the whole 1500x2060 artboard fits the viewport.
 *
 * The desktop build gets this for free — the native window resizes to fit, and
 * `get_ui_scale` reports the result. A browser tab has no such window, so
 * without this the canvas is laid out at a fixed 750x1030 CSS pixels. On a
 * 2x display that is ~2060 device pixels tall: far taller than any browser
 * viewport, so the player renders cropped with its top and bottom cut off.
 */
function fitScale(): number {
  const margin = 32;
  const availW = Math.max(240, window.innerWidth - margin);
  const availH = Math.max(240, window.innerHeight - margin);
  const fit = Math.min(availW / ART_W, availH / ART_H);
  // Never upscale past 1:1 artboard pixels — beyond that it is just blur.
  return Math.min(1, Math.max(0.15, fit));
}

function zoomStatusPct(): number {
  return Math.round((currentScale / fitScale()) * 100);
}

/**
 * Web zoom. Sizing the canvas in CSS pixels is the whole mechanism — the
 * backing store stays at the skin's 1500x2060, so the browser scales it and it
 * stays sharp. There is deliberately no CSS `transform` here: combining one
 * with a size change applies the zoom twice.
 */
function applyCssZoom(scale: number) {
  paintCanvasSize({ w: Math.round(ART_W * scale), h: Math.round(ART_H * scale) });
}

/**
 * Keep the player fitted to the viewport until the user zooms deliberately.
 *
 * Only CSS sizing is touched — no reload, so DSP and UI state survive
 * (AGENTS.md 3.2.3).
 */
let autoFit = true;

function refit() {
  currentScale = fitScale();
  applyCssZoom(currentScale);
}

/** User-driven zoom: stop auto-fitting until they ask to reset. */
function setZoom(scale: number, showStatus = true) {
  autoFit = false;
  applyScale(scale, showStatus);
}

/** Back to "exactly fills the window". */
function resetZoom(showStatus = true) {
  autoFit = true;
  refit();
  if (showStatus) status = `ZOOM ${zoomStatusPct()}%`;
}

export function applyScale(scale: number, showStatus = true) {
  if (transport.isWeb) {
    currentScale = scale;
    applyCssZoom(scale);
    if (showStatus) status = `ZOOM ${zoomStatusPct()}%`;
    return;
  }
  void transport
    .setUiScale(scale)
    .then((info) => {
      currentScale = info.scale;
      paintCanvasSize(info);
      if (showStatus) {
        status = `ZOOM ${zoomStatusPct()}%`;
      }
    })
    .catch((err) => {
      console.warn("set_ui_scale failed", err);
      const s = Math.max(0.25, Math.min(0.5, scale));
      currentScale = s;
      paintCanvasSize({ w: Math.round(ART_W * s), h: Math.round(ART_H * s) });
    });
}

let lastCycleAt = 0;

function cycleScale(direction: 1 | -1) {
  // Global-shortcut (Rust) and DOM keydown can both fire on Windows.
  const now = Date.now();
  if (now - lastCycleAt < 100) return;
  lastCycleAt = now;
  if (transport.isWeb) {
    // Step in 10% increments of the fitted scale, so "100%" always means
    // "exactly fills the window" rather than an arbitrary fixed size.
    const base = fitScale();
    const next = Math.min(1, Math.max(0.15, currentScale + direction * base * 0.1));
    setZoom(next);
    return;
  }
  void transport
    .cycleUiScale(direction)
    .then((info) => {
      currentScale = info.scale;
      paintCanvasSize(info);
      status = `ZOOM ${zoomStatusPct()}%`;
    })
    .catch((err) => console.warn("cycle_ui_scale failed", err));
}

/** Ctrl/Cmd + / - also handled in Rust via global-shortcut (WebView2-safe). */
function isZoomInKey(ev: KeyboardEvent): boolean {
  return (
    ev.key === "+" ||
    ev.key === "=" ||
    ev.key === "Add" ||
    ev.code === "Equal" ||
    ev.code === "NumpadAdd"
  );
}

function isZoomOutKey(ev: KeyboardEvent): boolean {
  return (
    ev.key === "-" ||
    ev.key === "_" ||
    ev.key === "Subtract" ||
    ev.code === "Minus" ||
    ev.code === "NumpadSubtract"
  );
}

// DOM backup for zoom (Rust global-shortcut is the primary path on Windows).
window.addEventListener(
  "keydown",
  (ev) => {
    const isCmdOrCtrl = ev.ctrlKey || ev.metaKey;
    if (!isCmdOrCtrl) return;

    if (isZoomInKey(ev)) {
      ev.preventDefault();
      ev.stopPropagation();
      cycleScale(1);
    } else if (isZoomOutKey(ev)) {
      ev.preventDefault();
      ev.stopPropagation();
      cycleScale(-1);
    } else if (ev.key === "0" || ev.code === "Digit0" || ev.code === "Numpad0") {
      ev.preventDefault();
      ev.stopPropagation();
      // "Reset" means "fill the window" on the web, not a fixed 0.5 that only
      // happens to be right on one particular monitor.
      if (transport.isWeb) {
        resetZoom();
      } else {
        applyScale(0.5);
      }
    } else if (ev.key.toLowerCase() === "d") {
      ev.preventDefault();
      ev.stopPropagation();
      if (transport.isWeb) {
        // Toggle between fitting and filling the window vertically.
        if (autoFit) {
          setZoom(fitScale());
        } else {
          resetZoom();
        }
        return;
      }
      void transport.getUiScale().then((info) => {
        applyScale(info.scale >= 0.9 ? 0.5 : Math.min(1.0, info.max_scale));
      });
    }
  },
  true,
);

/** m:ss for the status line. */
function fmtTime(secs: number): string {
  const s = Math.max(0, Math.floor(secs));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// Bare-key transport and cue hotkeys. Modified presses are left alone so the
// Ctrl/Cmd zoom combos above keep working, and so we never swallow a key the
// user meant for something else. Window focus only — not a global shortcut.
window.addEventListener("keydown", (ev) => {
  if (ev.ctrlKey || ev.metaKey || ev.altKey) return;

  // 0..9 → 0%..90% of the loaded track. ev.key matches numpad digits too.
  if (/^[0-9]$/.test(ev.key)) {
    ev.preventDefault();
    const pct = Number(ev.key) * 10;
    void transport
      .cuePercent(pct / 100)
      .then((target) => {
        status = target < 0 ? "No track" : `CUE ${pct}% ${fmtTime(target)}`;
      })
      .catch((err) => console.warn("cue_percent failed", err));
    return;
  }

  switch (ev.key) {
    case " ":
      ev.preventDefault();
      void transport
        .togglePlay()
        .then((playing) => {
          status = playing ? "Playing" : "Paused";
        })
        .catch((err) => console.warn("toggle_play failed", err));
      return;
    case "ArrowLeft":
    case "ArrowRight": {
      ev.preventDefault();
      const delta = ev.key === "ArrowRight" ? 10 : -10;
      void transport
        .getPosition()
        .then((pos) => {
          const target = Math.max(0, pos + delta);
          return transport.seek(target).then(() => target);
        })
        .then((target) => {
          status = `SEEK ${delta > 0 ? "+" : "-"}${Math.abs(delta)} ${fmtTime(target)}`;
        })
        .catch((err) => console.warn("seek failed", err));
      return;
    }
    case "z":
    case "Z":
      ev.preventDefault();
      void transport
        .prev()
        .then(() => {
          status = "Prev";
        })
        .catch((err) => console.warn("prev failed", err));
      return;
    case "x":
    case "X":
      ev.preventDefault();
      void transport
        .next()
        .then(() => {
          status = "Next";
        })
        .catch((err) => console.warn("next failed", err));
      return;
    default:
      return;
  }
});

let skin: SkinManifestV2;
/** Cryptic bitmap glyphs — decorative only; functional text uses canvas font. */
let font: BitmapFont | null = null;
const images = new Map<string, HTMLImageElement>();
/** Background + still overlays, flattened once; doubles as the anim mask. */
let plate: HTMLCanvasElement | null = null;
let bgOverlays: { img: HTMLImageElement; x: number; y: number }[] = [];
let anims: { def: SkinAnimDef; img: HTMLImageElement; cw: number; ch: number; start: number }[] = [];
/** Offscreen cell buffer for the plate mask, grown on demand and reused. */
let animMask: HTMLCanvasElement | null = null;
/** Beads running along the wire rails, plus the one sprite they all share. */
let flow: WireFlow | null = null;
let spot: HTMLCanvasElement | null = null;
/** Previous rAF timestamp, for the frame delta. */
let lastFrame = 0;

/** Master lowpass top: at this cutoff the filter is identity (bypass). */
const OPEN_CUTOFF_HZ = 20000;

const params: AudioParamsInput = {
  cutoff: OPEN_CUTOFF_HZ,
  pitch: 0,
  reverb: 0,
  eq: new Array(8).fill(0),
  speed: 1,
};

let playlist: PlaylistRow[] = [];
let activeId: number | null = null;
let status = "Ready";
let bins = new Float32Array(10);
let pressedButton: string | null = null;
let dragFader: string | null = null;
let playing = false;
/** fx_enable master: off = EQ + reverb bypass */
let fxOn = true;
/** playlist scroll (rows beyond 10) */
let playlistScroll = 0;
/** Echo scope (artboard 2×): X911 Y180 W226 H142 */
const SCOPE = { x: 911, y: 180, w: 226, h: 142 };
const SCOPE_N = 226;
const SCOPE_ECHO = 8;
const waveHistory: number[][] = Array.from({ length: SCOPE_ECHO + 1 }, () =>
  new Array(SCOPE_N).fill(0),
);
let waveIdx = 0;
/** Hann envelope: ~0 at ends, max at center (applied per sample). */
const scopeWindow = new Float32Array(SCOPE_N);
for (let i = 0; i < SCOPE_N; i++) {
  scopeWindow[i] = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (SCOPE_N - 1));
}

function setParam(key: string, value: number) {
  if (key.startsWith("eq")) {
    const i = Number(key.slice(2));
    params.eq[i] = value;
  } else if (key === "cutoff") params.cutoff = value;
  else if (key === "pitch") params.pitch = value;
  else if (key === "reverb") params.reverb = value;
  else if (key === "speed") params.speed = value;
  pushParams();
}

function pushParams() {
  // The sender owns the 8-band wire contract: a stale skin fader (`eq8` /
  // `eq9` / `eq10`, until Task 4 remaps them) writes past index 7 and would
  // make the whole payload the wrong length — the desktop guard then rejects
  // *every* param update, not just that fader, and the error is swallowed here.
  const eq = fxOn ? params.eq.slice(0, 8) : new Array(8).fill(0);
  const reverb = fxOn ? params.reverb : 0;
  void transport
    .setParams({
      cutoff: fxOn ? params.cutoff : OPEN_CUTOFF_HZ,
      pitch: fxOn ? params.pitch : 0,
      reverb,
      eq,
      speed: params.speed,
    })
    .catch(() => {});
}

function valueOf(param: string): number {
  if (param.startsWith("eq")) return params.eq[Number(param.slice(2))] ?? 0;
  if (param === "cutoff") return params.cutoff;
  if (param === "pitch") return params.pitch;
  if (param === "reverb") return params.reverb;
  if (param === "speed") return params.speed;
  return 0;
}

async function pushPlaylist() {
  const rows = await transport.getPlaylist();
  playlist = rows.map((r) => ({ id: r.id, title: r.title, duration: r.duration ?? "--:--" }));
}

async function action(name: string) {
  switch (name) {
    case "open": {
      // Desktop gets filesystem paths from the native dialog; the web gets
      // `File` objects it decodes in place. Both end at the same playlist.
      const picked = await transport.openFilePicker();
      if (picked.length === 0) return;
      status = "Loading…";
      if (transport.isWeb) {
        // The web picker already decoded the files as a side effect.
        await pushPlaylist();
      } else {
        await transport.openFiles(picked);
        await pushPlaylist();
      }
      status = `${picked.length} loaded`;
      break;
    }
    case "play": {
      if (playing) {
        await transport.pause();
        playing = false;
        status = "Paused";
      } else {
        try {
          await transport.play();
          playing = true;
          status = "Playing";
        } catch (err) {
          playing = false;
          status = String(err);
        }
      }
      await pushPlaylist();
      break;
    }
    case "pause":
      await transport.pause();
      playing = false;
      status = "Paused";
      break;
    case "stop":
      await transport.stop();
      playing = false;
      status = "Stopped";
      break;
    case "prev":
    case "next": {
      await (name === "next" ? transport.next() : transport.prev());
      playing = true;
      await pushPlaylist();
      const rows = await transport.getPlaylist();
      const cur = rows.find((r) => r.id === activeId);
      status = name === "next" ? "Next" : "Prev";
      if (cur) status += ` ${cur.title}`;
      break;
    }
    case "clear":
      await transport.clearPlaylist();
      activeId = null;
      playing = false;
      await pushPlaylist();
      status = "Cleared";
      break;
    case "reset_eq":
      for (let i = 0; i < 8; i++) params.eq[i] = 0;
      setParam("eq0", 0);
      status = "EQ reset";
      break;
    case "fx_enable":
    case "fx_reset": {
      if (name === "fx_enable") {
        fxOn = !fxOn;
        status = fxOn ? "FX on" : "FX off";
      } else {
        for (let i = 0; i < 8; i++) params.eq[i] = 0;
        params.reverb = 0;
        params.cutoff = OPEN_CUTOFF_HZ;
        params.pitch = 0; params.speed = 1;
        status = "FX reset";
      }
      pushParams();
      break;
    }
    case "minimize":
      // Deliberately NOT gated on isMinimizable(): tao reports false for a
      // decoration-less macOS window (its style mask never gets the
      // Miniaturizable bit) even though miniaturize still works, and Linux
      // hardcodes true whether or not the compositor honours the request.
      // Audio keeps playing while hidden — only the window goes away.
      // On the web this is a no-op; a tab has no window to minimize.
      await transport.minimize();
      break;
    case "power":
      await transport.close();
      break;
    default:
      break;
  }
}

function drawFader(f: FaderDef) {
  const knob = images.get(f.knob);
  // Draw nothing until the knob sprite lands. This used to fall back to a
  // magenta block, which is fine in a still and wrong in motion: the render
  // loop starts on the manifest, well before the art, so every fader flashed a
  // pink slab over the dark page for half a second and then vanished. An absent
  // fader reads as "not ready yet"; a magenta one reads as a glitch.
  if (!knob) return;
  const y = faderValueToY(f.origin, f.travel, f.range, valueOf(f.param), f.curve);
  // Natural pixel size — never scale knobs
  ctx.drawImage(knob, Math.round(f.origin.x), Math.round(y));
}

/** Magenta line + 5 fading echoes in SCOPE rect (cheap polyline trail). */
function drawEchoScope(ctx: CanvasRenderingContext2D) {
  const { x, y, w, h } = SCOPE;
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.lineWidth = 2;
  ctx.lineJoin = "round";
  const mid = y + h / 2;
  const amp = h * 0.42;
  // oldest → newest (newest on top)
  for (let e = SCOPE_ECHO; e >= 0; e--) {
    // waveIdx = next write = oldest; newest is waveIdx-1
    const idx = (((waveIdx - 1 - e) % (SCOPE_ECHO + 1)) + (SCOPE_ECHO + 1)) % (SCOPE_ECHO + 1);
    const row = waveHistory[idx];
    const alpha = e === 0 ? 1 : 0.85 * Math.pow(0.72, e);
    ctx.strokeStyle = `rgba(255,79,216,${alpha})`;
    ctx.beginPath();
    for (let i = 0; i < SCOPE_N; i++) {
      const px = x + i;
      const py = mid - row[i] * amp;
      if (i === 0) ctx.moveTo(px, py);
      else ctx.lineTo(px, py);
    }
    ctx.stroke();
  }
  ctx.restore();
}

/** Offscreen buffer sized to at least w x h, reused across frames. */
function maskBuffer(w: number, h: number): CanvasRenderingContext2D {
  if (!animMask) animMask = document.createElement("canvas");
  if (animMask.width < w || animMask.height < h) {
    animMask.width = Math.max(animMask.width, w);
    animMask.height = Math.max(animMask.height, h);
  }
  const mctx = animMask.getContext("2d")!;
  mctx.globalCompositeOperation = "source-over";
  mctx.clearRect(0, 0, w, h);
  return mctx;
}

/**
 * Sprite-sheet frames: slice current cell, screen-blend drops solid black.
 *
 * The sheets carry an opaque black backdrop, which `screen` erases — but only
 * where the player is already painted. Over the plate's transparent gaps the
 * blend has nothing to lift and the black cell survives as a hard square,
 * glaring against a light desktop. So each cell is first cut to the plate's
 * own alpha with `destination-in`: identical wherever the player is solid, and
 * gone everywhere it is not.
 */
function drawAnimations() {
  if (anims.length === 0 || !plate) return;
  const now = performance.now();
  for (const a of anims) {
    if (a.def.playback === "on-playing" && !playing) continue;
    // The ring reads as the "FX is live" lamp, so it follows the fx_enable
    // master — same shape as the on-playing gate, read off the manifest rather
    // than hardcoding this sheet's id in the compositor.
    if (a.def.playback === "on-fx" && !fxOn) continue;
    const fps = a.def.fps ?? 8;
    const frames = Math.max(1, a.def.frames);
    const idx = Math.floor(((now - a.start) / 1000) * fps) % frames;
    const col = idx % a.def.grid.cols;
    const row = Math.floor(idx / a.def.grid.cols);
    const sw = a.cw;
    const sh = a.ch;
    const x = a.def.origin.x;
    const y = a.def.origin.y;
    const dw = a.def.size?.w ?? sw;
    const dh = a.def.size?.h ?? sh;
    if (dw <= 0 || dh <= 0) continue;

    const mctx = maskBuffer(dw, dh);
    mctx.drawImage(a.img, col * sw, row * sh, sw, sh, 0, 0, dw, dh);
    mctx.globalCompositeOperation = "destination-in";
    mctx.drawImage(plate, -x, -y);

    ctx.save();
    if ((a.def.blend ?? "screen") === "screen") ctx.globalCompositeOperation = "screen";
    ctx.drawImage(animMask!, 0, 0, dw, dh, x, y, dw, dh);
    ctx.restore();
  }
}

function render(time: number) {
  const { width, height } = skin.canvas;
  ctx.clearRect(0, 0, width, height);

  if (plate) ctx.drawImage(plate, 0, 0);

  // Beads on the wire rails. Below the sprite-sheet animations so those keep
  // their contrast, and gated on the plate so nothing floats over a canvas
  // that has not drawn yet.
  if (plate && flow && spot) {
    // Clamped: a backgrounded tab stalls rAF for seconds, and an unclamped step
    // would jump every bead along its rail at once.
    const dt = lastFrame ? Math.min(0.05, (time - lastFrame) / 1000) : 0;
    lastFrame = time;
    flow.update(dt);
    flow.draw(ctx, spot);
  }

  drawAnimations();

  if (playing) {
    drawSpectrumSegments(ctx, images, skin.visuals.spectrum.bands, bins);
    drawEchoScope(ctx);
  }

  for (const f of skin.faders) drawFader(f);

  // Buttons: idle art in bg; ACTIVE overlay when pressed OR when state is on
  for (const b of skin.buttons) {
    const isActive =
      pressedButton === b.id ||
      (b.id === "play" && playing) ||
      (b.id === "fx_enable" && fxOn);
    if (!isActive) continue;
    const img = images.get(b.frames.pressed);
    if (img) {
      ctx.drawImage(img, b.origin.x, b.origin.y);
    }
  }

  const pl = skin.text.playlist;
  const colX = [pl.origin.x];
  for (let i = 0; i < pl.columns.length - 1; i++) colX.push(colX[i] + pl.columns[i].width);
  const box = pl.size ?? {
    w: pl.columns.reduce((s, c) => s + c.width, 0),
    h: pl.rows * pl.rowHeight,
  };
  ctx.save();
  ctx.beginPath();
  ctx.rect(pl.origin.x, pl.origin.y, box.w, box.h);
  ctx.clip();
  const visible = Math.min(pl.rows, playlist.length);
  for (let r = 0; r < visible; r++) {
    const trackIndex = r + playlistScroll;
    if (trackIndex >= playlist.length) break;
    const row = playlist[trackIndex];
    const y = pl.origin.y + r * pl.rowHeight;
    const active = row.id === activeId;
    if (active) {
      // The bar art sits slightly high against the glyph row; nudge it down so
      // it reads as centred on the text rather than riding above it.
      const hl = images.get("ui/active_track.png");
      // natural strip 373×24 — do not stretch across full box
      if (hl) {
        ctx.drawImage(hl, pl.origin.x, y + HIGHLIGHT_DY, hl.width, hl.height);
      } else {
        ctx.fillStyle = "#7dff4a";
        ctx.fillRect(pl.origin.x, y + HIGHLIGHT_DY, 373, 24);
      }
    }
    // № (2 digits) + 6 letters + duration
    // Files usually carry their own index ("01 - Song.mp3"); the row already
    // shows it, so strip that prefix or it eats the 6-letter budget.
    const base = row.title.replace(/\.[^.]+$/, "");
    const stripped = base.replace(/^\s*\d{1,3}\s*[-._)\]]?\s*/, "");
    const name6 = (stripped || base).slice(0, 6).toUpperCase();
    const num = String(r + 1).padStart(2, "0");
    // Duration column shows minutes only ("03" of "03:45") — seconds don't fit.
    const dur = (row.duration ?? "--:--").split(":")[0];
    // The selected row draws from the pre-darkened atlas, so its glyphs read as
    // inverted against the highlight bar. This used to be
    // `ctx.filter = "brightness(0.12)"`, which WebKitGTK silently ignores.
    font?.draw(ctx, num, pl.origin.x + 4, y, 48, active);
    font?.draw(ctx, name6, pl.origin.x + 56, y, 220, active);
    font?.draw(ctx, dur, pl.origin.x + 280, y, 48, active);
  }
  ctx.restore();
  // Status — art: (1153, 1825), width 220, trim overflow
  const st = skin.text.status;
  const stW = (st as { width?: number }).width ?? 220;
  ctx.save();
  ctx.beginPath();
  ctx.rect(st.origin.x, st.origin.y - 4, stW, 32);
  ctx.clip();
  font?.draw(ctx, status.toUpperCase().slice(0, 22), st.origin.x, st.origin.y, stW);
  ctx.restore();
  requestAnimationFrame(render);
}

function canvasPoint(ev: PointerEvent | MouseEvent) {
  return canvasPointFrom(ev, canvas);
}

function findFaderAt(x: number, y: number) {
  return skin.faders.find((f) => faderHit(x, y, f)) ?? null;
}

canvas.addEventListener("pointerdown", (ev) => {
  // Primary button only. `pointerdown` fires for right and middle too, so
  // without this a right-click was grabbing faders and — on empty plate —
  // calling `startDragging()`, which yanks the window around.
  if (ev.button !== 0) return;
  const p = canvasPoint(ev);
  for (const b of skin.buttons) {
    if (hitRect(p.x, p.y, b.origin, b.size)) {
      pressedButton = b.id;
      canvas.setPointerCapture(ev.pointerId);
      return;
    }
  }
  const fader = findFaderAt(p.x, p.y);
  if (fader) {
    dragFader = fader.id;
    canvas.setPointerCapture(ev.pointerId);
    setParam(fader.param, faderYToValue(fader.origin, fader.travel, fader.range, p.y, fader.curve));
    return;
  }
  // playlist rows: do not startDragging (click must play the track)
  const pl = skin.text.playlist;
  const boxW = pl.size?.w ?? pl.columns.reduce((s, c) => s + c.width, 0);
  const boxH = pl.size?.h ?? pl.rows * pl.rowHeight;
  if (
    p.x >= pl.origin.x &&
    p.x < pl.origin.x + boxW &&
    p.y >= pl.origin.y &&
    p.y < pl.origin.y + boxH
  ) {
    return;
  }
  void transport.startDragging();
});

canvas.addEventListener("pointermove", (ev) => {
  const p = canvasPoint(ev);
  if (dragFader) {
    const fader = skin.faders.find((f) => f.id === dragFader);
    if (fader) setParam(fader.param, faderYToValue(fader.origin, fader.travel, fader.range, p.y, fader.curve));
    return;
  }
  const over = findFaderAt(p.x, p.y);
  canvas.style.cursor = over ? "ns-resize" : "default";
});

/** Mouse wheel: fader under cursor, or playlist scroll if over playlist. */
canvas.addEventListener(
  "wheel",
  (ev) => {
    ev.preventDefault();
    // Ctrl = Windows/Linux pinch-zoom gesture; Meta = macOS Cmd + scroll.
    // Both must zoom instead of falling through to the fader/playlist branch.
    if (ev.ctrlKey || ev.metaKey) {
      cycleScale(ev.deltaY < 0 ? 1 : -1);
      return;
    }
    const p = canvasPoint(ev);
    const pl = skin.text.playlist;
    const boxW = pl.size?.w ?? pl.columns.reduce((s, c) => s + c.width, 0);
    const boxH = pl.size?.h ?? pl.rows * pl.rowHeight;
    const overList =
      p.x >= pl.origin.x &&
      p.x < pl.origin.x + boxW &&
      p.y >= pl.origin.y &&
      p.y < pl.origin.y + boxH;
    if (overList && playlist.length > pl.rows) {
      const maxScroll = Math.max(0, playlist.length - pl.rows);
      playlistScroll = Math.min(maxScroll, Math.max(0, playlistScroll + (ev.deltaY > 0 ? 1 : -1)));
      return;
    }
    const fader = findFaderAt(p.x, p.y);
    if (!fader) return;
    const frac = (ev.shiftKey ? 0.01 : 0.04) * (fader.wheelStep ?? 1);
    const next = faderStepValue(fader.range, valueOf(fader.param), frac, ev.deltaY < 0, fader.curve);
    setParam(fader.param, next);
  },
  { passive: false },
);

canvas.addEventListener("pointerup", async (ev) => {
  const p = canvasPoint(ev);
  if (pressedButton) {
    const b = skin.buttons.find((x) => x.id === pressedButton);
    pressedButton = null;
    if (b && hitRect(p.x, p.y, b.origin, b.size)) await action(b.action);
  }
  dragFader = null;
});

/**
 * The browser context menu is suppressed. The canvas is a picture of a player,
 * so "Save image as / Copy image / Inspect" is never what anyone wants here,
 * and `user-select: none` already rules out the text half of the menu.
 *
 * Scoped rather than blanket: real content keeps its menu, so the credit links
 * can still be copied or opened in a new tab. Handling the event also covers
 * the keyboard route (Menu key, Shift+F10), which an attribute-level
 * `oncontextmenu="return false"` would miss.
 *
 * Devtools are deliberately left alone — gating them is user-hostile, easily
 * defeated, and breaks the people who need them most.
 */
document.addEventListener("contextmenu", (ev) => {
  if ((ev.target as Element | null)?.closest("a")) return;
  ev.preventDefault();
});

/**
 * Right-click a fader to snap it back to its default.
 *
 * The default is the `value` the skin ships for that fader, which is the neutral
 * position for every control here: cutoff back to 20 kHz (the lowpass's open
 * bypass), pitch 0, reverb 0, all ten EQ bands 0, tempo 1.0. Taking it from the
 * manifest rather than hardcoding per-param constants means a skin that ships a
 * different resting point gets that one.
 *
 * Nothing else has claimed the right button by the time this runs — `pointerdown`
 * returns early for any non-primary button, so there is no drag to cancel and no
 * window drag to interrupt.
 */
canvas.addEventListener("contextmenu", (ev) => {
  const p = canvasPoint(ev);
  const fader = findFaderAt(p.x, p.y);
  if (!fader) return;
  ev.preventDefault();
  dragFader = null;
  setParam(fader.param, fader.value);
  status = `${fader.id} reset`;
});

canvas.addEventListener("dblclick", (ev: MouseEvent) => {
  void (async () => {
    const p = canvasPoint(ev);
    await playRowAt(p);
  })();
});

canvas.addEventListener("click", (ev: MouseEvent) => {
  void (async () => {
    const p = canvasPoint(ev);
    // single click on playlist row selects + plays
    const pl = skin.text.playlist;
    const box = pl.size ?? {
      w: pl.columns.reduce((s, c) => s + c.width, 0),
      h: pl.rows * pl.rowHeight,
    };
    for (let r = 0; r < pl.rows && r < playlist.length; r++) {
      const y0 = pl.origin.y + r * pl.rowHeight;
      if (p.y >= y0 && p.y < y0 + pl.rowHeight && p.x >= pl.origin.x && p.x < pl.origin.x + box.w) {
        await playRowAt(p);
        return;
      }
    }
  })();
});

async function playRowAt(p: { x: number; y: number }) {
  const pl = skin.text.playlist;
  const box = pl.size ?? {
    w: pl.columns.reduce((s, c) => s + c.width, 0),
    h: pl.rows * pl.rowHeight,
  };
  for (let r = 0; r < pl.rows; r++) {
    const trackIndex = r + playlistScroll;
    if (trackIndex >= playlist.length) break;
    const y0 = pl.origin.y + r * pl.rowHeight;
    if (p.y >= y0 && p.y < y0 + pl.rowHeight && p.x >= pl.origin.x && p.x < pl.origin.x + box.w) {
      const row = playlist[trackIndex];
      const idx = playlist.findIndex((x) => x.id === row.id);
      if (idx < 0) break;
      activeId = row.id;
      playing = true;
      await transport.playIndex(idx);
      await pushPlaylist();
      status = `PLAY ${row.title}`;
      break;
    }
  }
}

function loadImageSafe(url: string): Promise<HTMLImageElement | null> {
  return loadImage(url).catch((err) => {
    console.warn("missing sprite, skipped:", url, err);
    return null;
  });
}

async function init() {
  // Native side already fitted the window; sync canvas CSS to that scale.
  // On the web there is no window to fit, so zoom is CSS from the start.
  if (!transport.isWeb) {
    await transport.on<"ui_scale">("ui_scale", (e) => {
      currentScale = e.scale;
      paintCanvasSize(e);
    });
    try {
      const info = await transport.getUiScale();
      currentScale = info.scale;
      paintCanvasSize(info);
    } catch {
      paintCanvasSize({ w: Math.round(ART_W * 0.5), h: Math.round(ART_H * 0.5) });
    }
  } else {
    // No native window to fit, so size the canvas to the viewport instead.
    refit();
    window.addEventListener("resize", () => {
      if (autoFit) refit();
    });
  }
  skin = await loadJson(BASE + "skin.json");
  const resolve = (p: string) => BASE + p.replace(/^\/?/, "");

  // Prefer explicit bands; else auto-layout from chip pool + bandLeftX / bottomY
  const vis = skin.visuals.spectrum as unknown as {
    mode: string;
    bands: SkinManifestV2["visuals"]["spectrum"]["bands"];
    auto?: SpectrumAutoLayout & { overlap?: number };
  };
  if ((!vis.bands || vis.bands.length === 0) && vis.auto) {
    vis.bands = layoutSpectrumFromPool(vis.auto, vis.auto.overlap ?? 0.4);
    (skin.visuals.spectrum as unknown as { bands: unknown }).bands = vis.bands;
  }

  // Start painting NOW, before any sprite arrives.
  //
  // The render loop used to begin after every image had loaded, which meant the
  // canvas stayed blank for the whole of that — and since the track load runs
  // after it, it looked like the drawing was waiting on the audio. Starting
  // here means the page is alive as soon as the skin manifest parses and fills
  // in progressively: background first, then knobs and chips as they land.
  //
  // Safe on partial assets: render() skips the background when `plate` is null,
  // `drawAnimations` returns early with no anims, and every glyph draw is
  // `font?.draw`, a no-op until the atlas arrives.
  canvas.width = skin.canvas.width;
  canvas.height = skin.canvas.height;

  // Built here rather than after the sprite batch: parsing 21 short paths and
  // painting one 64px gradient is a fraction of a millisecond, and it has to
  // exist before the first frame asks for it (AGENTS 3.2.6).
  if (skin.visuals.rails !== false) {
    spot = makeSpotCanvas(64);
    flow = new WireFlow({ artW: skin.canvas.width });
  }
  requestAnimationFrame(render);

  // Every sprite is requested in ONE batch rather than awaited in turn.
  //
  // This was ~135 sequential requests — one per chip, fader and button, in
  // nested loops. On localhost that is invisible; over the network it is the
  // difference between a player in half a second and one that takes half a
  // minute to appear, because every image waited on the one before it.
  // Collect the whole set first, then let them all race.
  const animDefs = skin.animations ?? [];
  const overlayDefs = skin.background.overlays ?? [];
  const segments = skin.visuals.spectrum.bands.flatMap((b) => b.segments);
  const buttons = skin.buttons;

  // Progress hairline. Counted per file rather than by byte weight — the
  // background plate is 2.2 MB of the 3.6 MB total, so the bar sits low for a
  // while and then jumps; honest about "something is happening" without
  // pretending to a precision it does not have.
  let done = 0;
  const total =
    1 + animDefs.length + overlayDefs.length + segments.length + skin.faders.length +
    buttons.length + buttons.filter((b) => b.frames.normal).length + 2;
  const loadBar = document.getElementById("load-bar");
  const loadFill = loadBar?.firstElementChild as HTMLElement | null;
  const settle = <T,>(p: Promise<T>): Promise<T> =>
    p.finally(() => {
      done++;
      if (loadFill) loadFill.style.width = `${Math.min(100, (done / total) * 100)}%`;
    });
  const img = (path: string) => settle(loadImageSafe(resolve(path)));

  const [
    bgImgs,
    animImgs,
    overlayImgs,
    segmentImgs,
    faderImgs,
    pressedImgs,
    normalImgs,
    hlImg,
    loadedFont,
  ] = await Promise.all([
    img(skin.background.image),
    Promise.all(animDefs.map((d) => img(d.image))),
    Promise.all(overlayDefs.map((o) => img(o.image))),
    Promise.all(segments.map((s) => img(s.image))),
    Promise.all(skin.faders.map((f) => img(f.knob))),
    Promise.all(buttons.map((b) => img(b.frames.pressed))),
    Promise.all(
      buttons.map((b) =>
        b.frames.normal ? img(b.frames.normal) : Promise.resolve(null),
      ),
    ),
    img("ui/active_track.png"),
    settle(
      loadFont({ ...skin.text.font, atlas: resolve(skin.text.font.atlas) }, "").catch(
        () => null,
      ),
    ),
  ]);

  // Done: fill the bar, then let it fade rather than snapping out of existence.
  if (loadFill) loadFill.style.width = "100%";
  loadBar?.classList.add("done");

  // Sprite-sheet animations (uniform grid, solid black BG → screen blend).
  anims = animDefs.flatMap((def, i) => {
    const img = animImgs[i];
    if (!img) return [];
    return [{
      def,
      img,
      cw: Math.floor(img.width / def.grid.cols),
      ch: Math.floor(img.height / def.grid.rows),
      start: performance.now(),
    }];
  });

  // Still highlight overlays are drawn as-is; the artist erases animated
  // regions from the layer in the PSD, so no engine-side masking.
  bgOverlays = overlayDefs.flatMap((o, i) => {
    const img = overlayImgs[i];
    return img ? [{ img, x: o.origin.x, y: o.origin.y }] : [];
  });

  // Flatten background + overlays into one plate. render() blits it, and
  // drawAnimations() uses its alpha as the mask that keeps a cell from
  // spilling past the silhouette — so this has to be baked, not re-blitted
  // per frame, and it must land at the same coordinates the mask samples.
  if (bgImgs) {
    plate = document.createElement("canvas");
    plate.width = skin.canvas.width;
    plate.height = skin.canvas.height;
    const pctx = plate.getContext("2d")!;
    pctx.drawImage(bgImgs, skin.background.origin.x, skin.background.origin.y);
    for (const o of bgOverlays) pctx.drawImage(o.img, o.x, o.y);
  }

  segments.forEach((seg, i) => {
    const img = segmentImgs[i];
    if (!img) return;
    images.set(seg.image, img);
    seg.size = { w: img.width, h: img.height };
  });

  skin.faders.forEach((f, i) => {
    const img = faderImgs[i];
    if (!img) return;
    images.set(f.knob, img);
    if (!f.knobSize) f.knobSize = { w: img.width, h: img.height };
  });

  buttons.forEach((b, i) => {
    const p = pressedImgs[i];
    if (p) {
      images.set(b.frames.pressed, p);
      b.size = { w: p.width, h: p.height };
    }
    const n = normalImgs[i];
    if (n && b.frames.normal) images.set(b.frames.normal, n);
  });

  if (hlImg) images.set("ui/active_track.png", hlImg);
  font = loadedFont;

  for (const f of skin.faders) {
    const v = typeof f.value === "number" ? f.value : (f.range[0] + f.range[1]) / 2;
    setParam(f.param, v);
  }

  await transport.on<"spectrum">("spectrum", (payload) => {
    const raw = Float32Array.from(payload);
    const out = new Float32Array(10);
    const n = raw.length;
    const edges = [0, 0.04, 0.1, 0.18, 0.3, 0.45, 0.6, 0.75, 0.85, 0.93, 1.0];
    // Display tilt: low bins carry far more raw energy (band 0 alone spans
    // ~43-600 Hz of bass) — without compensation it pins at max while high
    // bands never light. ≈ +1.5 dB per band (+3 dB/octave for this spacing).
    const BAND_GAIN = [0.5, 0.58, 0.67, 0.78, 0.9, 1.0, 1.3, 1.7, 2.1, 2.6];
    for (let b = 0; b < 10; b++) {
      const i0 = Math.floor(edges[b] * n);
      const i1 = Math.max(i0 + 1, Math.floor(edges[b + 1] * n));
      let s = 0;
      for (let i = i0; i < i1 && i < n; i++) s += raw[i] ?? 0;
      const v = (s / Math.max(1, i1 - i0)) * (BAND_GAIN[b] ?? 1);
      // Gate display noise so idle tails don't light chips (AGENTS rule).
      out[b] = v <= 0.02 ? 0 : Math.min(1, v);
    }
    bins = out;
  });
  await transport.on<"waveform">("waveform", (payload) => {
    const raw = Float32Array.from(payload);
    const row = waveHistory[waveIdx % (SCOPE_ECHO + 1)];
    for (let i = 0; i < SCOPE_N && i < raw.length; i++) {
      row[i] = raw[i] * scopeWindow[i];
    }
    waveIdx = (waveIdx + 1) % (SCOPE_ECHO + 1);
  });
  await transport.on<"play_started">("play_started", () => {
    playing = true;
    status = "Playing";
  });
  await transport.on<"error">("error", (msg) => {
    playing = false;
    status = msg;
  });
  await transport.on<"track_changed">("track_changed", (id) => {
    activeId = id;
    playing = true;
    void pushPlaylist();
  });
  await transport.on<"track_ended">("track_ended", async () => {
    try {
      await transport.next();
      await pushPlaylist();
      playing = true;
    } catch {
      status = "ENDED";
      playing = false;
    }
  });

  // Whole-window drag-and-drop is a web-only affordance; on desktop files
  // arrive through the native dialog, which gives real paths.
  if (transport instanceof WebTransport) {
    const player = transport.webPlayer;
    installDropTarget(async (files) => {
      status = `Loading ${files.length}…`;
      // Same replace semantics as the picker: dropped files are the new set.
      await player.openFiles(files, true);
      await pushPlaylist();
      status = `${files.length} loaded`;
    });
    // Hosted-MP3 loading (spec Phase 5). Exposed on `window.misima` so a track
    // can be added by URL from the console or a future UI affordance. It goes
    // through the same playlist refresh as the other entry points, otherwise
    // the rows exist in the engine but never reach the screen.
    (window as unknown as { misima: unknown }).misima = {
      loadUrl: async (url: string, title?: string) => {
        await player.addUrl(url, title);
        await pushPlaylist();
      },
      player,
    };

    // Record the first real gesture before any handler can reach `play()`.
    // Capture phase, so it runs ahead of the canvas listeners below. This is
    // what lets `ensureContext` call `resume()` only when the browser will
    // actually honour it.
    const markGesture = () => player.noteGesture();
    window.addEventListener("pointerdown", markGesture, { capture: true });
    window.addEventListener("keydown", markGesture, { capture: true });
  }

  await pushPlaylist();

  // The render loop is already running (started as soon as the skin manifest
  // parsed, above). Nothing below may sit between the canvas becoming visible
  // and it being drawn.
  if (transport instanceof WebTransport) {
    const player = transport.webPlayer;

    // Startup tracks: register the whole bundled set so the playlist is
    // populated on arrival, but only fetch the first — it is the one that
    // autoplays. The rest stream in when the listener reaches them.
    for (const t of STARTUP_TRACKS) player.registerRemote(t.url, t.title);
    await pushPlaylist();
    await player.addUrl(STARTUP_TRACKS[0].url, STARTUP_TRACKS[0].title);
    await pushPlaylist();

    // Autoplay is gated on a user gesture, so this succeeds only on a repeat
    // visit or where the browser is lenient. When it does not, the track is
    // already decoded and sitting in the playlist: arm a one-shot handler so
    // the first click or keypress anywhere starts it, rather than leaving the
    // visitor to work out that the play button is the thing to press.
    if (await player.tryAutoplay()) {
      playing = true;
      status = "Playing";
    } else {
      status = "Press play";
      const kick = () => {
        window.removeEventListener("pointerdown", kick);
        window.removeEventListener("keydown", kick);
        void (async () => {
          try {
            await transport.play();
            playing = true;
            status = "Playing";
          } catch (err) {
            status = String(err);
          }
        })();
      };
      window.addEventListener("pointerdown", kick);
      window.addEventListener("keydown", kick);
    }
  }
}

void init();
