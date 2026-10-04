import type { FontClass, FontSpec, GlyphDef, SkinManifestV2 } from "./types";
import { loadImage } from "./layout";

export type BitmapFont = {
  spec: FontSpec;
  atlas: HTMLImageElement;
  draw(
    ctx: CanvasRenderingContext2D,
    text: string,
    x: number,
    y: number,
    maxWidth?: number,
    /** Draw as dark inverted glyphs, for the selected playlist row. */
    dim?: boolean,
  ): void;
  measure(text: string): number;
  lineHeight: number;
};

/**
 * Brightness of the selected playlist row's glyphs. Matches the
 * `ctx.filter = "brightness(0.12)"` this replaced, so the look is unchanged on
 * the platforms where that filter actually worked.
 */
const DIM_BRIGHTNESS = 0.12;

/**
 * A copy of the atlas with every glyph knocked down to `brightness`, used for
 * the selected playlist row so it reads as inverted against the green bar.
 *
 * Built with `source-atop` rather than a plain `fillRect`, which is the whole
 * point: `source-atop` confines the fill to pixels that already exist and
 * preserves their alpha, so this reproduces `brightness()` — colour scaled,
 * shape untouched — instead of also making the glyphs translucent. Doing it
 * once here costs nothing per frame, unlike filtering at draw time.
 *
 * The original `ctx.filter` approach was Chromium-only: WebKit gates canvas
 * filters behind a preference that WebKitGTK does not enable, so on Linux the
 * assignment was silently ignored and the selected row rendered undimmed.
 */
function darkenedAtlas(atlas: HTMLImageElement, brightness: number): HTMLCanvasElement {
  const c = document.createElement("canvas");
  c.width = atlas.naturalWidth || atlas.width;
  c.height = atlas.naturalHeight || atlas.height;
  const g = c.getContext("2d")!;
  g.drawImage(atlas, 0, 0);
  g.globalCompositeOperation = "source-atop";
  const v = Math.round(brightness * 255);
  g.fillStyle = `rgb(${v},${v},${v})`;
  g.fillRect(0, 0, c.width, c.height);
  return c;
}

function resolveGlyph(def: GlyphDef) {
  if (Array.isArray(def)) {
    return { col: def[0], row: def[1], className: undefined as string | undefined, wCells: 1, hCells: 1 };
  }
  return {
    col: def.col,
    row: def.row,
    className: def.class,
    wCells: def.w ?? 1,
    hCells: def.h ?? 1,
  };
}

/**
 * Atlas: col/row are indices in that class's grid, then + atlasOrigin.
 * Letters: row 0..3 at origin (0,120) — do NOT offset row by 4.
 */
export function createFont(spec: FontSpec, atlas: HTMLImageElement): BitmapFont {
  const classes: Record<string, FontClass> = {
    digit: { cell: { w: 24, h: 24 }, baseline: "bottom", atlasOrigin: { x: 0, y: 0 } },
    letter: { cell: { w: 36, h: 18 }, baseline: "bottom", atlasOrigin: { x: 0, y: 120 } },
    symbol: { cell: { w: 24, h: 18 }, baseline: "bottom", atlasOrigin: { x: 0, y: 72 } },
    ...(spec.classes ?? {}),
  };
  const lineH = Math.max(...Object.values(classes).map((c) => c.cell.h), spec.cell.h);

  function boxFor(ch: string): { sx: number; sy: number; sw: number; sh: number } | null {
    const def = spec.map[ch] ?? spec.map[spec.fallback] ?? spec.map[" "] ?? spec.map["?"];
    if (!def) return null;
    const g = resolveGlyph(def);
    const clsName = g.className ?? "digit";
    const cls = classes[clsName] ?? classes.digit!;
    const ox = cls.atlasOrigin?.x ?? 0;
    const oy = cls.atlasOrigin?.y ?? 0;
    return {
      sx: ox + g.col * cls.cell.w,
      sy: oy + g.row * cls.cell.h,
      sw: cls.cell.w * g.wCells,
      sh: cls.cell.h * g.hCells,
    };
  }

  const dimAtlas = darkenedAtlas(atlas, DIM_BRIGHTNESS);

  const font: BitmapFont = {
    spec,
    atlas,
    lineHeight: lineH,
    measure(text: string) {
      let w = 0;
      for (const ch of text) {
        const b = boxFor(ch);
        w += b?.sw ?? spec.cell.w;
      }
      return w;
    },
    draw(ctx, text, x, y, maxWidth, dim) {
      const src = dim ? dimAtlas : font.atlas;
      let cx = x;
      for (const ch of text) {
        const b = boxFor(ch);
        if (!b) {
          cx += spec.cell.w;
          continue;
        }
        if (maxWidth != null && cx + b.sw > x + maxWidth) break;
        const dy = y + (lineH - b.sh);
        ctx.drawImage(src, b.sx, b.sy, b.sw, b.sh, Math.round(cx), Math.round(dy), b.sw, b.sh);
        cx += b.sw;
      }
    },
  };
  return font;
}

export async function loadFont(spec: FontSpec, baseUrl: string): Promise<BitmapFont> {
  const atlas = await loadImage(baseUrl + spec.atlas);
  return createFont(spec, atlas);
}

export function lineHeight(spec: FontSpec): number {
  return Math.max(spec.cell.h, ...Object.values(spec.classes ?? {}).map((c) => c.cell.h), spec.cell.h);
}

export type { SkinManifestV2 };
