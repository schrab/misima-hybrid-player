export type XY = { x: number; y: number };
export type Size = { w: number; h: number };

export type FaderDef = {
  id: string;
  param: string;
  orientation: "vertical" | "horizontal";
  origin: XY;
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
  origin: XY;
  size: Size;
  frames: { normal?: string; pressed: string };
};

export type BackgroundDef = {
  image: string;
  origin: XY;
  size: Size;
};

/** One organic bar piece. Absolute artboard position; may overlap other segments. */
export type SpectrumSegment = {
  image: string;
  origin: XY;
  size?: Size;
  /**
   * Energy threshold 0..1 when this piece lights (after the previous one).
   * Pieces can overlap freely — order of reveal is `reveal` (then array order).
   */
  reveal: number;
};

/** A spectrum column: freeform overlapping pieces, not a rectangular stack. */
export type SpectrumBand = {
  id: string | number;
  /** Pieces may share vertical space and vary in size. */
  segments: SpectrumSegment[];
};

export type FontClass = {
  /** Glyph box in artboard px. Letters are typically 2:1 and shorter than digits. */
  cell: Size;
  /** Vertical align within the line box: baseline is bottom of tallest class. */
  baseline?: "bottom" | "center";
  /** Optional atlas grid origin for this class (when classes share one PNG). */
  atlasOrigin?: XY;
};

export type GlyphDef =
  | [number, number]
  | {
      col: number;
      row: number;
      class?: string;
      w?: number;
      h?: number;
    };

export type FontSpec = {
  atlas: string;
  /** Default class cell (fallback). */
  cell: Size;
  classes?: Record<string, FontClass>;
  map: Record<string, GlyphDef>;
  fallback: string;
};

export type SkinManifestV2 = {
  formatVersion: 2;
  id: string;
  name: string;
  units?: string;
  canvas: { width: number; height: number; scale?: number };
  background: BackgroundDef;
  blocks?: Record<string, BackgroundDef>;
  faders: FaderDef[];
  buttons: ButtonDef[];
  visuals: {
    spectrum: {
      mode: "segments";
      /** Explicit pieces (may be empty if `auto` is used). */
      bands: SpectrumBand[];
      /**
       * Prefill helper: band left-border X list + shared bottomY + chip pool.
       * Engine/layout expands this to `bands` so artists need only ~8 unique chips.
       */
      auto?: {
        bandLeftX: number[];
        bottomY: number;
        maxHeight?: number;
        segmentsPerBand?: number;
        chips: string[];
        chipSizes?: Array<{ w: number; h: number }>;
        overlap?: number;
      };
    };
    waterfall: {
      origin: XY;
      size: Size;
      mode: string;
      color: string;
    };
  };
  text: {
    font: FontSpec;
    playlist: {
      origin: XY;
      /** Optional bounding box (top-left origin + size) for clipping/hit. */
      size?: Size;
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
