/**
 * Desktop `Transport`: the original Tauri IPC, unchanged.
 *
 * Every method maps 1:1 onto a Rust command, so this file is a thin adapter —
 * the point of it is that `main.ts` can stop importing `@tauri-apps/api`
 * directly and go through the interface both platforms share.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AudioParamsInput,
  EventName,
  PlaylistRow,
  Transport,
  TransportEvents,
  UiScaleInfo,
} from "./transport";

export class TauriTransport implements Transport {
  readonly isWeb = false;

  play(): Promise<void> {
    return invoke("play");
  }
  pause(): Promise<void> {
    return invoke("pause");
  }
  stop(): Promise<void> {
    return invoke("stop");
  }
  togglePlay(): Promise<boolean> {
    return invoke("toggle_play");
  }
  next(): Promise<void> {
    return invoke("next");
  }
  prev(): Promise<void> {
    return invoke("prev");
  }
  playIndex(index: number): Promise<void> {
    return invoke("play_index", { index });
  }
  setParams(params: AudioParamsInput): Promise<void> {
    return invoke("set_params", {
      cutoff: params.cutoff,
      pitch: params.pitch,
      reverb: params.reverb,
      eq: params.eq,
      speed: params.speed,
    });
  }
  openFiles(paths: string[]): Promise<void> {
    return invoke("open_files", { paths });
  }

  /** Native dialog. Returns filesystem paths, which `openFiles` then takes. */
  async openFilePicker(): Promise<string[]> {
    const selected = await open({
      multiple: true,
      filters: [{ name: "Audio", extensions: ["mp3", "flac", "wav", "ogg"] }],
    });
    if (!selected) return [];
    return Array.isArray(selected) ? selected : [selected];
  }

  getPosition(): Promise<number> {
    return invoke("get_position");
  }
  getPlaylist(): Promise<PlaylistRow[]> {
    return invoke("get_playlist");
  }
  clearPlaylist(): Promise<void> {
    return invoke("clear_playlist");
  }
  seek(seconds: number): Promise<void> {
    return invoke("seek", { seconds });
  }
  cuePercent(fraction: number): Promise<number> {
    return invoke("cue_percent", { fraction });
  }
  setUiScale(scale: number): Promise<UiScaleInfo> {
    return invoke("set_ui_scale", { scale });
  }
  getUiScale(): Promise<UiScaleInfo> {
    return invoke("get_ui_scale");
  }
  cycleUiScale(direction: 1 | -1): Promise<UiScaleInfo> {
    return invoke("cycle_ui_scale", { direction });
  }

  startDragging(): Promise<void> {
    return getCurrentWindow().startDragging();
  }
  minimize(): Promise<void> {
    return getCurrentWindow().minimize();
  }
  close(): Promise<void> {
    return getCurrentWindow().close();
  }

  async on<E extends EventName>(
    event: E,
    handler: (payload: TransportEvents[E]) => void,
  ): Promise<void> {
    // Keep the unlisten function reachable so teardown is possible; the UI
    // registers these once at startup and never detaches, matching the
    // original main.ts behaviour.
    const unlisten: UnlistenFn = await listen<TransportEvents[E]>(event, (e) =>
      handler(e.payload),
    );
    unlisteners.push(unlisten);
  }
}

const unlisteners: UnlistenFn[] = [];

/** Detach every listener registered through this transport. */
export function unlistenAll(): void {
  while (unlisteners.length > 0) unlisteners.pop()?.();
}
