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

function setStatus(msg: string) {
  statusEl.textContent = msg;
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
      renderPlaylist(entries);
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
}

async function refreshPlaylist() {
  const entries = await invoke<PlaylistEntry[]>("get_playlist");
  renderPlaylist(entries);
}

function drawSpectrum() {
  const w = spectrumCanvas.width;
  const h = spectrumCanvas.height;
  ctx.clearRect(0, 0, w, h);
  const n = bins.length;
  if (n === 0) return;
  const barW = w / n;
  for (let i = 0; i < n; i++) {
    const v = Math.min(1, bins[i]);
    const barH = Math.max(2, v * (h - 8));
    const t = i / n;
    const r = Math.round(61 + t * (255 - 61));
    const g = Math.round(255 - t * (255 - 79));
    const b = Math.round(181 + t * (216 - 181));
    ctx.fillStyle = `rgb(${r},${g},${b})`;
    ctx.shadowColor = "rgba(61,255,181,0.45)";
    ctx.shadowBlur = 8;
    ctx.fillRect(i * barW + 1, h - barH - 2, Math.max(2, barW - 2), barH);
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
}

document.getElementById("btn-open")!.addEventListener("click", () => {
  void openFiles();
});

document.getElementById("pl-clear")!.addEventListener("click", async () => {
  await invoke("clear_playlist");
  activeId = null;
  await refreshPlaylist();
});

document.getElementById("eq-reset")!.addEventListener("click", async () => {
  eqBandsEl.querySelectorAll<HTMLInputElement>('input[type="range"]').forEach((el) => {
    el.value = "0";
  });
  await sendEq();
});

volumeInput.addEventListener("change", async () => {
  await invoke("set_volume", { volume: Number(volumeInput.value) / 100 });
});

document.querySelectorAll<HTMLButtonElement>("[data-cmd]").forEach((btn) => {
  btn.addEventListener("click", async () => {
    const cmd = btn.dataset.cmd!;
    await invoke(cmd);
    if (cmd === "play" || cmd === "next" || cmd === "prev") {
      await refreshPlaylist();
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
  setStatus("Ready — OPEN to add files");
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
    const el = document.getElementById(id);
    if (el) {
      el.style.backgroundImage = `url(${data})`;
      el.style.backgroundSize = "100% 100%";
    }
  }
  setStatus(`Skin: ${bundle.manifest.name}`);
}

void init();
