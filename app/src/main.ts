/**
 * Misima Hybrid — sprite UI entry.
 * Coordinates are Photoshop 2x artboard pixels (see README "Coordinate system").
 */
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
  hitRect,
  loadJson,
  loadImage,
} from "./sprite/layout";
import { loadFont, type BitmapFont } from "./sprite/font";
import { drawSpectrumSegments } from "./sprite/visuals";
import { layoutSpectrumFromPool, type SpectrumAutoLayout } from "./sprite/spectrumLayout";

const BASE = "/sprite/";

const canvas = document.getElementById("ui") as HTMLCanvasElement;
const ctx = canvas.getContext("2d")!;

let skin: SkinManifestV2;
let font: BitmapFont;
const images = new Map<string, HTMLImageElement>();
let bg: HTMLImageElement | null = null;

const params: AudioParams = {
  volume: 1.0,
  pitch: 0,
  reverb: 0,
  eq: new Array(10).fill(0),
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

function setParam(key: string, value: number) {
  if (key.startsWith("eq")) {
    const i = Number(key.slice(2));
    params.eq[i] = value;
  } else if (key === "volume") params.volume = value;
  else if (key === "pitch") params.pitch = value;
  else if (key === "reverb") params.reverb = value;
  else if (key === "speed") params.speed = value;
  pushParams();
}

function pushParams() {
  const eq = fxOn ? params.eq : new Array(10).fill(0);
  const reverb = fxOn ? params.reverb : 0;
  void invoke("set_params", {
    volume: params.volume,
    pitch: fxOn ? params.pitch : 0,
    reverb,
    eq,
    speed: params.speed,
  }).catch(() => {});
}

function valueOf(param: string): number {
  if (param.startsWith("eq")) return params.eq[Number(param.slice(2))] ?? 0;
  if (param === "volume") return params.volume;
  if (param === "pitch") return params.pitch;
  if (param === "reverb") return params.reverb;
  if (param === "speed") return params.speed;
  return 0;
}

async function pushPlaylist() {
  const rows = await invoke<{ id: number; title: string; duration?: string }[]>("get_playlist");
  playlist = rows.map((r) => ({ id: r.id, title: r.title, duration: r.duration ?? "--:--" }));
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
      status = "Adding…";
      await invoke("open_files", { paths });
      await pushPlaylist();
      status = `${paths.length} added`;
      break;
    }
    case "play": {
      if (playing) {
        await invoke("pause");
        playing = false;
        status = "Paused";
      } else {
        await invoke("play");
        playing = true;
        status = "Playing";
      }
      await pushPlaylist();
      break;
    }
    case "pause":
      await invoke("pause");
      playing = false;
      status = "Paused";
      break;
    case "stop":
      await invoke("stop");
      playing = false;
      status = "Stopped";
      break;
    case "prev":
    case "next": {
      await invoke(name === "next" ? "next" : "prev");
      playing = true;
      await pushPlaylist();
      const rows = await invoke<{ id: number; title: string }[]>("get_playlist");
      const cur = rows.find((r) => r.id === activeId);
      status = name === "next" ? "Next" : "Prev";
      if (cur) status += ` ${cur.title}`;
      break;
    }
    case "clear":
      await invoke("clear_playlist");
      activeId = null;
      playing = false;
      await pushPlaylist();
      status = "Cleared";
      break;
    case "reset_eq":
      for (let i = 0; i < 10; i++) params.eq[i] = 0;
      setParam("eq0", 0);
      status = "EQ reset";
      break;
    case "fx_enable":
    case "fx_reset": {
      if (name === "fx_enable") {
        fxOn = !fxOn;
        status = fxOn ? "FX on" : "FX off";
      } else {
        for (let i = 0; i < 10; i++) params.eq[i] = 0;
        params.reverb = 0;
        params.pitch = 0;
        status = "FX reset";
      }
      pushParams();
      break;
    }
    case "power":
      await getCurrentWindow().close();
      break;
    default:
      break;
  }
}

function drawFader(f: FaderDef) {
  const knob = images.get(f.knob);
  const y = faderValueToY(f.origin, f.travel, f.range, valueOf(f.param));
  // Natural pixel size — never scale knobs
  const kw = knob?.width ?? f.knobSize?.w ?? 24;
  const kh = knob?.height ?? f.knobSize?.h ?? 24;
  if (knob) {
    ctx.drawImage(knob, Math.round(f.origin.x), Math.round(y));
  } else {
    ctx.fillStyle = "#ff4fd8";
    ctx.fillRect(f.origin.x, y, kw, kh);
  }
}

function render(_time: number) {
  const { width, height } = skin.canvas;
  ctx.clearRect(0, 0, width, height);

  if (bg) ctx.drawImage(bg, skin.background.origin.x, skin.background.origin.y);

  if (playing) {
    drawSpectrumSegments(ctx, images, skin.visuals.spectrum.bands, bins);
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
  for (let r = 0; r < pl.rows && r < playlist.length; r++) {
    const row = playlist[r];
    const y = pl.origin.y + r * pl.rowHeight;
    font.draw(ctx, row.id === activeId ? "*" : " ", pl.origin.x, y, 20);
    font.draw(ctx, String(r + 1).padStart(2, "0"), colX[0], y, pl.columns[0].width);
    font.draw(ctx, row.title.toUpperCase().slice(0, 28), colX[1], y, pl.columns[1].width);
    const dur = row.duration ?? "";
    const durW = font.measure(dur);
    const col = pl.columns[2];
    const dx = col.align === "right" ? colX[2] + col.width - durW : colX[2];
    font.draw(ctx, dur, dx, y, col.width);
  }
  ctx.restore();
  // status only inside playlist header strip (never outside bg)
  const st = skin.text.status.origin;
  ctx.save();
  ctx.beginPath();
  ctx.rect(pl.origin.x, pl.origin.y - 28, box.w, 26);
  ctx.clip();
  font.draw(ctx, status.toUpperCase().slice(0, 30), st.x, st.y, box.w - 8);
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
  void getCurrentWindow().startDragging();
});

canvas.addEventListener("pointermove", (ev) => {
  const p = canvasPoint(ev);
  if (dragFader) {
    const fader = skin.faders.find((f) => f.id === dragFader);
    if (fader) setParam(fader.param, faderYToValue(fader.origin, fader.travel, fader.range, p.y));
    return;
  }
  const over = findFaderAt(p.x, p.y);
  canvas.style.cursor = over ? "ns-resize" : "default";
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
  for (let r = 0; r < pl.rows && r < playlist.length; r++) {
    const y0 = pl.origin.y + r * pl.rowHeight;
    if (p.y >= y0 && p.y < y0 + pl.rowHeight && p.x >= pl.origin.x && p.x < pl.origin.x + box.w) {
      const row = playlist[r];
      activeId = row.id;
      await invoke("play_index", { index: playlist.findIndex((x) => x.id === row.id) });
      playing = true;
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

  bg = await loadImageSafe(resolve(skin.background.image));
  // spectrum segments
  for (const band of skin.visuals.spectrum.bands) {
    for (const seg of band.segments) {
      const img = await loadImageSafe(resolve(seg.image));
      if (img) {
        images.set(seg.image, img);
        seg.size = { w: img.width, h: img.height };
      }
    }
  }

  for (const f of skin.faders) {
    const img = await loadImageSafe(resolve(f.knob));
    if (img) {
      images.set(f.knob, img);
      if (!f.knobSize) f.knobSize = { w: img.width, h: img.height };
    }
  }
  for (const b of skin.buttons) {
    const p = await loadImageSafe(resolve(b.frames.pressed));
    if (p) {
      images.set(b.frames.pressed, p);
      b.size = { w: p.width, h: p.height };
    }
    if (b.frames.normal) {
      const n = await loadImageSafe(resolve(b.frames.normal));
      if (n) images.set(b.frames.normal, n);
    }
  }
  font = await loadFont({ ...skin.text.font, atlas: resolve(skin.text.font.atlas) }, "");

  canvas.width = skin.canvas.width;
  canvas.height = skin.canvas.height;

  for (const f of skin.faders) {
    const v = typeof f.value === "number" ? f.value : (f.range[0] + f.range[1]) / 2;
    setParam(f.param, v);
  }

  await listen<Float32Array>("spectrum", (e) => {
    const raw = Float32Array.from(e.payload);
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
    playing = true;
    void pushPlaylist();
  });
  await listen("track_ended", async () => {
    try {
      await invoke("next");
      await pushPlaylist();
      playing = true;
    } catch {
      status = "ENDED";
      playing = false;
    }
  });

  await pushPlaylist();
  requestAnimationFrame(render);
}

void init();
