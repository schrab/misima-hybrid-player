import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import type { AudioParams, FaderDef, PlaylistRow, SkinManifestV2 } from "./sprite/types";
import {
  canvasPointFrom,
  faderHit,
  faderValueToY,
  faderYToValue,
  hitBox,
  hitRect,
  loadJson,
  loadImage,
} from "./sprite/layout";
import { loadFont, type BitmapFont } from "./sprite/font";
import { drawSpectrum, drawWaterfall } from "./sprite/visuals";

const BASE = "/sprite/";

const canvas = document.getElementById("ui") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

let skin: SkinManifestV2;
let font: BitmapFont;
const images = new Map<string, HTMLImageElement>();
let spectrumSheet: HTMLImageElement;

const params: AudioParams = {
  volume: 0.8,
  pitch: 0,
  reverb: 0.15,
  eq: new Array(10).fill(0),
  speed: 1,
};

let playlist: PlaylistRow[] = [];
let activeId: number | null = null;
let status = "Ready";
let bins = new Float32Array(10);
let pressedButton: string | null = null;
let dragFader: string | null = null;
let hoverFader: string | null = null;

function setParam(key: string, value: number) {
  if (key.startsWith("eq")) {
    const i = Number(key.slice(2));
    params.eq[i] = value;
  } else if (key === "volume") params.volume = value;
  else if (key === "pitch") params.pitch = value;
  else if (key === "reverb") params.reverb = value;
  else if (key === "speed") params.speed = value;
  void invoke("set_params", {
    volume: params.volume,
    pitch: params.pitch,
    reverb: params.reverb,
    eq: params.eq,
    speed: params.speed,
  }).catch(() => {});
}

async function pushPlaylist() {
  const rows = await invoke<{ id: number; path: string; title: string; duration?: string }[]>(
    "get_playlist",
  );
  playlist = rows.map((r) => ({
    id: r.id,
    title: r.title,
    duration: r.duration ?? "--:--",
  }));
}

async function action(name: string) {
  switch (name) {
    case "open": {
      const selected = await open({
        multiple: true,
        filters: [{ name: "Audio", extensions: ["mp3", "flac", "wav", "ogg"] }],
      });
      if (!selected) return;
      const paths = Array.isArray(selected) ? selected : [selected];
      await invoke("open_files", { paths });
      await pushPlaylist();
      status = `${paths.length} added`;
      break;
    }
    case "play":
    case "pause":
    case "stop":
    case "prev":
    case "next":
      await invoke(name);
      await pushPlaylist();
      status = name.toUpperCase();
      break;
    case "clear":
      await invoke("clear_playlist");
      activeId = null;
      await pushPlaylist();
      status = "Cleared";
      break;
    case "reset_eq":
      for (let i = 0; i < 10; i++) {
        params.eq[i] = 0;
      }
      setParam("eq0", 0);
      status = "EQ reset";
      break;
    default:
      break;
  }
}

function drawFader(f: FaderDef) {
  const knob = images.get(f.knob);
  const y = faderValueToY(f.origin, f.travel, f.range, valueOf(f.param));
  const kx = f.origin.x - (knob?.width ?? 22) / 2;
  if (knob) {
    ctx.drawImage(knob, kx, y - (knob.height ?? 28) / 2);
  } else {
    ctx.fillStyle = "#ff4fd8";
    ctx.fillRect(f.origin.x - 8, y - 6, 16, 12);
  }
}

function valueOf(param: string): number {
  if (param.startsWith("eq")) return params.eq[Number(param.slice(2))] ?? 0;
  if (param === "volume") return params.volume;
  if (param === "pitch") return params.pitch;
  if (param === "reverb") return params.reverb;
  if (param === "speed") return params.speed;
  return 0;
}

function drawButton(id: string, origin: { x: number; y: number }, size: { w: number; h: number }, frames: { normal: string; pressed?: string }) {
  const pressed = pressedButton === id;
  const key = pressed && frames.pressed ? frames.pressed : frames.normal;
  const img = images.get(key);
  if (img) ctx.drawImage(img, origin.x, origin.y, size.w, size.h);
  else {
    ctx.fillStyle = pressed ? "#2a8f6f" : "#1a3034";
    ctx.fillRect(origin.x, origin.y, size.w, size.h);
  }
  font.draw(ctx, id.toUpperCase().slice(0, 4), origin.x + 6, origin.y + 10);
}

function render(time: number) {
  const { width, height } = skin.canvas;
  ctx.clearRect(0, 0, width, height);

  // blocks
  for (const block of Object.values(skin.blocks)) {
    const img = images.get(block.image);
    if (img) ctx.drawImage(img, block.origin.x, block.origin.y, block.size.w, block.size.h);
  }

  // generative
  drawWaterfall(ctx, skin.visuals.waterfall, [...bins], time);
  if (spectrumSheet) {
    drawSpectrum(ctx, spectrumSheet, skin.visuals.spectrum, bins);
  }

  // faders
  for (const f of skin.faders) drawFader(f);

  // buttons
  for (const b of skin.buttons) {
    drawButton(b.id, b.origin, b.size, b.frames);
  }

  // playlist text
  const pl = skin.text.playlist;
  const colX = [pl.origin.x];
  for (let i = 0; i < pl.columns.length - 1; i++) {
    colX.push(colX[i] + pl.columns[i].width);
  }
  for (let r = 0; r < pl.rows && r < playlist.length; r++) {
    const row = playlist[r];
    const y = pl.origin.y + r * pl.rowHeight;
    const mark = row.id === activeId ? "*" : " ";
    font.draw(ctx, mark, pl.origin.x - 12, y, 12);
    font.draw(ctx, String(r + 1).padStart(2, "0"), colX[0], y, pl.columns[0].width);
    font.draw(ctx, row.title.toUpperCase().slice(0, 36), colX[1], y, pl.columns[1].width);
    const dur = row.duration ?? "";
    font.draw(
      ctx,
      dur,
      colX[2] + pl.columns[2].width - dur.length * font.cell.w,
      y,
      pl.columns[2].width,
    );
  }

  font.draw(ctx, status.toUpperCase(), skin.text.status.origin.x, skin.text.status.origin.y);

  requestAnimationFrame(render);
}

function canvasPoint(ev: PointerEvent | MouseEvent) {
  return canvasPointFrom(ev, canvas);
}

function findFaderAt(x: number, y: number) {
  for (const f of skin.faders) {
    if (faderHit(x, y, f.origin, f.travel)) return f;
  }
  return null;
}

canvas.addEventListener("pointerdown", (ev) => {
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
    setParam(fader.param, faderYToValue(fader.origin, fader.travel, fader.range, p.y));
    return;
  }
  // drag window from block drag zones
  for (const block of Object.values(skin.blocks)) {
    for (const d of block.drag ?? []) {
      const box = { x: block.origin.x + d.x, y: block.origin.y + d.y, w: d.w, h: d.h };
      if (hitBox(p.x, p.y, box)) {
        void getCurrentWindow().startDragging();
        return;
      }
    }
  }
});

canvas.addEventListener("pointermove", (ev) => {
  const p = canvasPoint(ev);
  if (dragFader) {
    const fader = skin.faders.find((f) => f.id === dragFader);
    if (fader) setParam(fader.param, faderYToValue(fader.origin, fader.travel, fader.range, p.y));
    return;
  }
  hoverFader = findFaderAt(p.x, p.y)?.id ?? null;
  canvas.style.cursor = hoverFader ? "ns-resize" : "default";
});

canvas.addEventListener("pointerup", async (ev) => {
  const p = canvasPoint(ev);
  if (pressedButton) {
    const b = skin.buttons.find((x) => x.id === pressedButton);
    pressedButton = null;
    if (b && hitRect(p.x, p.y, b.origin, b.size)) await action(b.action);
  }
  dragFader = null;
});

canvas.addEventListener("dblclick", (ev: MouseEvent) => {
  void (async () => {
    const p = canvasPoint(ev);
    const pl = skin.text.playlist;
    for (let r = 0; r < pl.rows && r < playlist.length; r++) {
      const y0 = pl.origin.y + r * pl.rowHeight;
      if (p.y >= y0 && p.y < y0 + pl.rowHeight && p.x >= pl.origin.x - 16) {
        const row = playlist[r];
        activeId = row.id;
        const idx = playlist.findIndex((x) => x.id === row.id);
        await invoke("play_index", { index: idx });
        await pushPlaylist();
        status = `PLAY ${row.title}`;
        break;
      }
    }
  })();
});

function loadImageSafe(url: string): Promise<HTMLImageElement | null> {
  return loadImage(url).catch((err) => {
    console.warn("missing sprite, skipped:", url, err);
    return null;
  });
}

async function init() {
  skin = await loadJson(BASE + "skin.json");
  const resolve = (p: string) => BASE + p.replace(/^\/?/, "");

  for (const block of Object.values(skin.blocks)) {
    images.set(block.image, (await loadImageSafe(resolve(block.image))) as HTMLImageElement);
  }
  for (const f of skin.faders) {
    const img = await loadImageSafe(resolve(f.knob));
    if (img) images.set(f.knob, img);
  }
  for (const b of skin.buttons) {
    const n = await loadImageSafe(resolve(b.frames.normal));
    if (n) images.set(b.frames.normal, n);
    if (b.frames.pressed) {
      const p = await loadImageSafe(resolve(b.frames.pressed));
      if (p) images.set(b.frames.pressed, p);
    }
  }
  spectrumSheet = (await loadImageSafe(resolve(skin.visuals.spectrum.sheet))) as HTMLImageElement;
  font = await loadFont({ ...skin.text.font, atlas: resolve(skin.text.font.atlas) }, "");

  canvas.width = skin.canvas.width;
  canvas.height = skin.canvas.height;

  // seed fader values
  for (const f of skin.faders) {
    const v = typeof f.value === "number" ? f.value : (f.range[0] + f.range[1]) / 2;
    setParam(f.param, v);
  }

  await listen<Float32Array>("spectrum", (e) => {
    const raw = Float32Array.from(e.payload);
    // 10 log bands aligned to EQ centers (approx 60…16k) over 48 log-ish bins
    const out = new Float32Array(10);
    const n = raw.length;
    const edges = [0, 0.04, 0.1, 0.18, 0.3, 0.45, 0.6, 0.75, 0.85, 0.93, 1.0];
    for (let b = 0; b < 10; b++) {
      const i0 = Math.floor(edges[b] * n);
      const i1 = Math.max(i0 + 1, Math.floor(edges[b + 1] * n));
      let s = 0;
      for (let i = i0; i < i1 && i < n; i++) s += raw[i] ?? 0;
      out[b] = s / Math.max(1, i1 - i0);
    }
    bins = out;
  });
  await listen<number>("track_changed", (e) => {
    activeId = e.payload;
    void pushPlaylist();
  });
  await listen("track_ended", async () => {
    try {
      await invoke("next");
      await pushPlaylist();
    } catch {
      status = "ENDED";
    }
  });

  await pushPlaylist();
  requestAnimationFrame(render);
}

void init();
