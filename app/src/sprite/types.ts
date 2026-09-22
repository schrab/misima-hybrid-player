export type XY = { x: number; y: number };
export type Size = { w: number; h: number };

export type FaderDef = {
  id: string;
  param: string;
  orientation: "vertical" | "horizontal";
  origin: XY;
  travel: number;
  knob: string;
  range: [number, number];
  value: number;
  unit?: string;
};

export type ButtonDef = {
  id: string;
  action: string;
  origin: XY;
  size: Size;
  frames: { normal: string; pressed?: string };
};

export type BlockDef = {
  image: string;
  origin: XY;
  size: Size;
  hit?: string;
  drag?: Array<{ x: number; y: number; w: number; h: number }>;
};

export type SkinManifestV2 = {
  formatVersion: 2;
  id: string;
  name: string;
  canvas: { width: number; height: number; scale?: number };
  blocks: Record<string, BlockDef>;
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
