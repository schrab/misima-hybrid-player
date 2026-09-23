export type XY = { x: number; y: number };
export type Size = { w: number; h: number };

export type FaderDef = {
  id: string;
  param: string;
  orientation: "vertical" | "horizontal";
  /** TOP-LEFT of knob at MAX value (Photoshop 2x artboard px). */
  origin: XY;
  /** Y distance to top-left at MIN value (vertical). */
  travel: number;
  knob: string;
  knobSize?: Size;
  knobHotspot?: string;
  range: [number, number];
  value: number;
  unit?: string;
};

export type ButtonDef = {
  id: string;
  action: string;
  /** TOP-LEFT of button PNG (idle art lives in bg.png). */
  origin: XY;
  size: Size;
  frames: { normal?: string; pressed: string };
};

export type BackgroundDef = {
  image: string;
  origin: XY;
  size: Size;
};

export type SkinManifestV2 = {
  formatVersion: 2;
  id: string;
  name: string;
  units?: string;
  canvas: { width: number; height: number; scale?: number };
  background: BackgroundDef;
  /** Optional extra plates; primary art is `background`. */
  blocks?: Record<string, BackgroundDef>;
  faders: FaderDef[];
  buttons: ButtonDef[];
  visuals: {
    spectrum: {
      origin: XY;
      size: Size;
      bands: number;
      cell: Size;
      frames: number;
      sheet: string;
      align: "bottom";
      gapPx: number;
    };
    waterfall: {
      origin: XY;
      size: Size;
      mode: string;
      color: string;
    };
  };
  text: {
    font: {
      atlas: string;
      cell: Size;
      map: Record<string, [number, number]>;
      fallback: string;
    };
    playlist: {
      origin: XY;
      rows: number;
      rowHeight: number;
      columns: Array<{ id: string; width: number; align?: "left" | "right" }>;
    };
    status: { origin: XY };
  };
};

export type PlaylistRow = {
  id: number;
  title: string;
  duration?: string;
};

export type AudioParams = {
  volume: number;
  pitch: number;
  reverb: number;
  eq: number[];
  speed: number;
};
