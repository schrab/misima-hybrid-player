/**
 * Platform abstraction over the player's control surface.
 *
 * `main.ts` used to talk to Tauri directly through 17 `invoke()` commands and
 * 7 `listen()` event channels. Rather than fork the UI, both platforms
 * implement the same `Transport` interface and `main.ts` keeps its single
 * code path. The Tauri implementation is the original behaviour, unchanged.
 *
 * On the web, `open_files` and the desktop-only window/zoom commands are the
 * only places the two implementations genuinely differ — see the notes on
 * each method.
 */

export type AudioParamsInput = {
  volume: number;
  speed: number;
  pitch: number;
  reverb: number;
  eq: number[];
};

export type UiScaleInfo = { scale: number; w: number; h: number; max_scale: number };

/**
 * A playlist row.
 *
 * `sourceUrl` marks a row whose audio has not been fetched yet — the web build
 * registers the bundled startup tracks by URL and only downloads one when the
 * listener plays it. Desktop rows have no `sourceUrl` because they are decoded
 * from disk before they ever appear.
 */
export type PlaylistRow = {
  id: number;
  title: string;
  duration?: string;
  sourceUrl?: string;
};

/** Events pushed from the engine up to the UI. */
export type TransportEvents = {
  spectrum: Float32Array;
  waveform: Float32Array;
  play_started: void;
  error: string;
  track_changed: number;
  track_ended: void;
  ui_scale: UiScaleInfo;
};

export type EventName = keyof TransportEvents;

/**
 * The full control surface `main.ts` uses. Every method is safe to call on
 * both platforms; desktop-only capabilities become documented no-ops on web
 * rather than throwing, so the UI never needs to branch on platform.
 */
export interface Transport {
  readonly isWeb: boolean;

  // --- invoke commands ---
  play(): Promise<void>;
  pause(): Promise<void>;
  stop(): Promise<void>;
  togglePlay(): Promise<boolean>;
  next(): Promise<void>;
  prev(): Promise<void>;
  playIndex(index: number): Promise<void>;
  setParams(params: AudioParamsInput): Promise<void>;
  /** `paths` are filesystem paths on desktop, ignored on web (see `openFiles`). */
  openFiles(paths: string[]): Promise<void>;
  /** Open the platform's file picker. Returns the chosen files/paths. */
  openFilePicker(): Promise<string[]>;
  getPosition(): Promise<number>;
  getPlaylist(): Promise<PlaylistRow[]>;
  clearPlaylist(): Promise<void>;
  seek(seconds: number): Promise<void>;
  cuePercent(fraction: number): Promise<number>;
  setUiScale(scale: number): Promise<UiScaleInfo>;
  getUiScale(): Promise<UiScaleInfo>;
  cycleUiScale(direction: 1 | -1): Promise<UiScaleInfo>;

  // --- window control (no-ops on web) ---
  startDragging(): Promise<void>;
  minimize(): Promise<void>;
  close(): Promise<void>;

  // --- events ---
  on<E extends EventName>(event: E, handler: (payload: TransportEvents[E]) => void): Promise<void>;
}
