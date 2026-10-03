/**
 * Web `Transport`: drives `WebPlayer` and presents the same surface as the
 * Tauri transport.
 *
 * Three categories of difference from desktop, all handled here so `main.ts`
 * stays platform-blind:
 *
 * - **Window control** (`startDragging`, `minimize`, `close`) is a no-op: a
 *   browser tab has no window to drive. Dragging is the browser's own job.
 * - **Zoom** is CSS, not a native window size. `main.ts` calls `applyCssZoom`
 *   directly; the scale commands are inert so the shared code path still runs.
 * - **Events** are pushed by `WebPlayer` rather than emitted by Rust. The
 *   desktop-only `ui_scale` event is never fired.
 */

import type {
  AudioParamsInput,
  EventName,
  PlaylistRow,
  Transport,
  TransportEvents,
  UiScaleInfo,
} from "./transport";
import { WebPlayer } from "./web/player";
import { pickFiles } from "./web/files";

export class WebTransport implements Transport {
  readonly isWeb = true;
  private player = new WebPlayer();
  private handlers = new Map<EventName, ((payload: never) => void)[]>();

  constructor() {
    this.player.setCallbacks({
      tap: (spectrum, waveform) => {
        this.emit("spectrum", spectrum);
        this.emit("waveform", waveform);
      },
      ended: () => {
        this.emit("track_ended", undefined);
      },
      position: (pos) => {
        // The UI polls `getPosition`; this keeps the internal value fresh for
        // anyone who reads it between frames.
        void pos;
      },
      error: (msg) => this.emit("error", msg),
      trackChanged: (id) => this.emit("track_changed", id),
    });
  }

  private emit<E extends EventName>(event: E, payload: TransportEvents[E]) {
    for (const h of this.handlers.get(event) ?? []) {
      (h as (p: TransportEvents[E]) => void)(payload);
    }
  }

  on<E extends EventName>(
    event: E,
    handler: (payload: TransportEvents[E]) => void,
  ): Promise<void> {
    const list = this.handlers.get(event) ?? [];
    list.push(handler as (payload: never) => void);
    this.handlers.set(event, list);
    return Promise.resolve();
  }

  // ------------------------------------------------------------------ player

  async play(): Promise<void> {
    await this.player.play();
    this.emit("play_started", undefined);
  }
  pause(): Promise<void> {
    this.player.pause();
    return Promise.resolve();
  }
  stop(): Promise<void> {
    this.player.stop();
    return Promise.resolve();
  }
  async togglePlay(): Promise<boolean> {
    const playing = await this.player.togglePlay();
    if (playing) this.emit("play_started", undefined);
    return playing;
  }
  next(): Promise<void> {
    this.player.next();
    return Promise.resolve();
  }
  prev(): Promise<void> {
    this.player.prev();
    return Promise.resolve();
  }
  async playIndex(index: number): Promise<void> {
    await this.player.playIndex(index);
  }
  setParams(params: AudioParamsInput): Promise<void> {
    this.player.setParams(params);
    return Promise.resolve();
  }

  /**
   * `paths` is meaningless on the web — the audio never touches a filesystem.
   * The picker path goes through `openFilePicker` instead.
   */
  openFiles(_paths: string[]): Promise<void> {
    return Promise.resolve();
  }

  /** Opens the browser picker and decodes whatever comes back. */
  async openFilePicker(): Promise<string[]> {
    const files = await pickFiles();
    if (files.length === 0) return [];
    await this.player.openFiles(files);
    return files.map((f) => f.name);
  }

  getPosition(): Promise<number> {
    return Promise.resolve(this.player.getPosition());
  }
  getPlaylist(): Promise<PlaylistRow[]> {
    return Promise.resolve(this.player.playlist());
  }
  clearPlaylist(): Promise<void> {
    this.player.clear();
    return Promise.resolve();
  }
  seek(seconds: number): Promise<void> {
    this.player.seek(seconds);
    return Promise.resolve();
  }
  cuePercent(fraction: number): Promise<number> {
    return Promise.resolve(this.player.cuePercent(fraction));
  }

  // ------------------------------------------------- desktop-only, inert here

  setUiScale(scale: number): Promise<UiScaleInfo> {
    return Promise.resolve(inertScale(scale));
  }
  getUiScale(): Promise<UiScaleInfo> {
    return Promise.resolve(inertScale(0.5));
  }
  cycleUiScale(_direction: 1 | -1): Promise<UiScaleInfo> {
    return Promise.resolve(inertScale(0.5));
  }

  startDragging(): Promise<void> {
    return Promise.resolve();
  }
  minimize(): Promise<void> {
    return Promise.resolve();
  }
  close(): Promise<void> {
    return Promise.resolve();
  }

  /** Exposed for `main.ts`'s drag-and-drop wiring and hosted-MP3 loading. */
  get webPlayer(): WebPlayer {
    return this.player;
  }
}

/**
 * A plausible `UiScaleInfo` for the no-op zoom commands. `main.ts` only reads
 * `.w`/`.h` to size the canvas, and the CSS zoom path sizes it itself, so the
 * values here are only ever a placeholder.
 */
function inertScale(scale: number): UiScaleInfo {
  return { scale, w: 0, h: 0, max_scale: 1 };
}
