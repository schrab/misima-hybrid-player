import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";

export interface PlaylistEntry {
  id: number;
  path: string;
  title: string;
}

const statusEl = document.getElementById("status")!;
const playlistEl = document.getElementById("playlist") as HTMLOListElement;
const spectrumCanvas = document.getElementById("spectrum") as HTMLCanvasElement;
const ctx = spectrumCanvas.getContext("2d")!;
const volumeInput = document.getElementById("volume") as HTMLInputElement;
const eqBandsEl = document.getElementById("eq-bands")!;

const EQ_FREQS = [60, 170, 310, 600, 1000, 3000, 6000, 12000, 14000, 16000];
let bins: Float32Array = new Float32Array(48);
let activeId: number | null = null;
let draggingId: number | null = null;
let peakHold = new Float32Array(48);

function setStatus(msg: string) {
  statusEl.textContent = msg;
}

/** Default skin plates shipped in /public/skin (Vite serves them). */
function applyDefaultSkin() {
  const map: Record<string, string> = {
    "panel-main": "/skin/panel_main.png",
    "panel-eq": "/skin/panel_eq.png",
    "panel-playlist": "/skin/panel_playlist.png",
  };
  for (const [id, url] of Object.entries(map)) {
    document.getElementById(id)?.style.setProperty("--panel-bg", `url(${url})`);
  }
}

function buildEq() {
  eqBandsEl.innerHTML = "";
  EQ_FREQS.forEach((freq, i) => {
    const wrap = document.createElement("div");
    wrap.className = "eq-band";
    const label = document.createElement("label");
    label.textContent = freq >= 1000 ? `${freq / 1000}k` : String(freq);
    const input = document.createElement("input");
    input.type = "range";
    input.min = "-12";
    input.max = "12";
    input.step = "0.5";
    input.value = "0";
    input.dataset.index = String(i);
    input.addEventListener("change", () => sendEq());
    wrap.append(label, input);
    eqBandsEl.append(wrap);
  });
}

async function sendEq() {
  const inputs = [...eqBandsEl.querySelectorAll<HTMLInputElement>('input[type="range"]')];
  const gains = inputs.map((el) => parseFloat(el.value));
  await invoke("set_eq", { gains });
}

function renderPlaylist(entries: PlaylistEntry[]) {
  playlistEl.innerHTML = "";
  for (const entry of entries) {
    const li = document.createElement("li");
    li.textContent = entry.title;
    li.dataset.id = String(entry.id);
    li.draggable = true;
    if (entry.id === activeId) li.classList.add("active");
    li.addEventListener("dblclick", async () => {
      activeId = entry.id;
      await invoke("play_index", { index: entries.findIndex((e) => e.id === entry.id) });
      await refreshPlaylist();
      setStatus(`Playing: ${entry.title}`);
    });
    li.addEventListener("dragstart", () => {
      draggingId = entry.id;
      li.classList.add("dragging");
    });
    li.addEventListener("dragend", () => {
      draggingId = null;
      li.classList.remove("dragging");
    });
    li.addEventListener("dragover", (e) => e.preventDefault());
    li.addEventListener("drop", async (e) => {
      e.preventDefault();
      if (draggingId == null || draggingId === entry.id) return;
      const from = entries.findIndex((x) => x.id === draggingId);
      const to = entries.findIndex((x) => x.id === entry.id);
      await invoke("reorder_playlist", { from, to });
      await refreshPlaylist();
    });
    playlistEl.append(li);
  }
  if (entries.length > 0 && !statusEl.textContent?.startsWith("Playing")) {
    setStatus(`${entries.length} track${entries.length === 1 ? "" : "s"} loaded`);
  }
}

async function refreshPlaylist() {
  const entries = await invoke<PlaylistEntry[]>("get_playlist");
  renderPlaylist(entries);
}

function drawSpectrum() {
  const w = spectrumCanvas.width;
  const h = spectrumCanvas.height;
  ctx.clearRect(0, 0, w, h);

  // grid
  ctx.strokeStyle = "rgba(94,200,255,0.08)";
  ctx.lineWidth = 1;
  for (let y = 20; y < h; y += 20) {
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();
  }

  const n = bins.length;
  if (n === 0) {
    requestAnimationFrame(drawSpectrum);
    return;
  }
  const barW = w / n;
  let max = 0.001;
  for (let i = 0; i < n; i++) max = Math.max(max, bins[i]);

  for (let i = 0; i < n; i++) {
    // Adaptive normalize + mild gamma so quiet mixes still fill the panel
    const norm = bins[i] / max;
    const v = Math.min(1, Math.pow(norm, 0.65) * 0.95 + bins[i] * 0.4);
    peakHold[i] = Math.max(v, peakHold[i] * 0.92);
    const barH = Math.max(3, v * (h - 10));
    const peakY = h - Math.max(3, peakHold[i] * (h - 10)) - 2;
    const t = i / Math.max(1, n - 1);
    const r = Math.round(61 + t * (255 - 61));
    const g = Math.round(255 - t * (255 - 79));
    const b = Math.round(181 + t * (216 - 181));
    ctx.fillStyle = `rgb(${r},${g},${b})`;
    ctx.shadowColor = `rgba(${r},${g},${b},0.55)`;
    ctx.shadowBlur = 10;
    const x = i * barW + 1;
    ctx.fillRect(x, h - barH - 2, Math.max(2, barW - 2), barH);
    // peak cap
    ctx.shadowBlur = 0;
    ctx.fillStyle = "rgba(255,255,255,0.55)";
    ctx.fillRect(x, peakY, Math.max(2, barW - 2), 2);
  }
  ctx.shadowBlur = 0;
  requestAnimationFrame(drawSpectrum);
}

async function openFiles() {
  const selected = await open({
    multiple: true,
    filters: [
      {
        name: "Audio",
        extensions: ["mp3", "flac", "wav", "ogg"],
      },
    ],
  });
  if (!selected) return;
  const paths = Array.isArray(selected) ? selected : [selected];
  await invoke("open_files", { paths });
  await refreshPlaylist();
  setStatus(`${paths.length} file${paths.length === 1 ? "" : "s"} added`);
}

document.getElementById("btn-open")!.addEventListener("click", () => {
  void openFiles();
});

document.getElementById("pl-clear")!.addEventListener("click", async () => {
  await invoke("clear_playlist");
  activeId = null;
  await refreshPlaylist();
  setStatus("Playlist cleared — OPEN to add files");
});

document.getElementById("eq-reset")!.addEventListener("click", async () => {
  eqBandsEl.querySelectorAll<HTMLInputElement>('input[type="range"]').forEach((el) => {
    el.value = "0";
  });
  await sendEq();
  setStatus("EQ reset");
});

volumeInput.addEventListener("input", async () => {
  await invoke("set_volume", { volume: Number(volumeInput.value) / 100 });
});

document.querySelectorAll<HTMLButtonElement>("[data-cmd]").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const cmd = btn.dataset.cmd!;
    await invoke(cmd);
    if (cmd === "play" || cmd === "next" || cmd === "prev") {
      await refreshPlaylist();
      const entries = await invoke<PlaylistEntry[]>("get_playlist");
      const cur = entries.find((e) => e.id === activeId);
      setStatus(cur ? `Playing: ${cur.title}` : `Transport: ${cmd}`);
    } else if (cmd === "pause") {
      setStatus("Paused");
    } else if (cmd === "stop") {
      setStatus("Stopped");
    }
  });
});

document.getElementById("btn-close")!.addEventListener("click", async () => {
  await getCurrentWindow().close();
});

document.getElementById("btn-min")!.addEventListener("click", async () => {
  await getCurrentWindow().minimize();
});

async function init() {
  applyDefaultSkin();
  buildEq();
  drawSpectrum();
  await listen<Float32Array>("spectrum", (e) => {
    bins = Float32Array.from(e.payload);
  });
  await listen<number>("track_changed", (e) => {
    activeId = e.payload;
    void refreshPlaylist();
  });
  await listen("track_ended", async () => {
    try {
      await invoke("next");
      await refreshPlaylist();
    } catch {
      setStatus("Playback ended");
    }
  });
  await refreshPlaylist();
  if (!playlistEl.children.length) {
    setStatus("Ready — OPEN to add files");
  }
}

/** Apply a skin bundle returned by the load_skin command. */
export async function applySkinFromPath(path: string) {
  const bundle = await invoke<{
    manifest: {
      id: string;
      name: string;
      panels?: Record<
        string,
        { image?: string; rect?: { x: number; y: number; w: number; h: number } }
      >;
    };
    assets: Record<string, string>;
  }>("load_skin", { path });
  const panels = bundle.manifest.panels ?? {};
  const map: Record<string, string> = {
    main: "panel-main",
    eq: "panel-eq",
    playlist: "panel-playlist",
  };
  for (const [key, id] of Object.entries(map)) {
    const panel = panels[key];
    if (!panel?.image) continue;
    const data = bundle.assets[panel.image];
    if (!data) continue;
    document.getElementById(id)?.style.setProperty("--panel-bg", `url(${data})`);
  }
  setStatus(`Skin: ${bundle.manifest.name}`);
}

void init();
