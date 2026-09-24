import type { FontClass, FontSpec, GlyphDef, SkinManifestV2 } from "./types";
import { loadImage } from "./layout";

export type BitmapFont = {
  spec: FontSpec;
  atlas: HTMLImageElement;
  draw(ctx: CanvasRenderingContext2D, text: string, x: number, y: number, maxWidth?: number): void;
  measure(text: string): number;
  lineHeight: number;
};

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
    draw(ctx, text, x, y, maxWidth) {
      let cx = x;
      for (const ch of text) {
        const b = boxFor(ch);
        if (!b) {
          cx += spec.cell.w;
          continue;
        }
        if (maxWidth != null && cx + b.sw > x + maxWidth) break;
        const dy = y + (lineH - b.sh);
        ctx.drawImage(font.atlas, b.sx, b.sy, b.sw, b.sh, Math.round(cx), Math.round(dy), b.sw, b.sh);
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
