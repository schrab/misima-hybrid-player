import type { SkinManifestV2 } from "./types";
import { loadImage } from "./layout";

export type BitmapFont = {
  atlas: HTMLImageElement;
  cell: { w: number; h: number };
  map: Record<string, [number, number]>;
  fallback: string;
  draw(
    ctx: CanvasRenderingContext2D,
    text: string,
    x: number,
    y: number,
    maxWidth?: number,
  ): void;
};

export function createFont(spec: SkinManifestV2["text"]["font"], atlas: HTMLImageElement): BitmapFont {
  const font: BitmapFont = {
    atlas,
    cell: spec.cell,
    map: spec.map,
    fallback: spec.fallback ?? "?",
    draw(ctx, text, x, y, maxWidth) {
      let cx = x;
      for (const ch of text) {
        const cell = font.map[ch] ?? font.map[font.fallback];
        if (!cell) continue;
        if (maxWidth != null && cx + font.cell.w > x + maxWidth) break;
        const [col, row] = cell;
        ctx.drawImage(
          font.atlas,
          col * font.cell.w,
          row * font.cell.h,
          font.cell.w,
          font.cell.h,
          Math.round(cx),
          Math.round(y),
          font.cell.w,
          font.cell.h,
        );
        cx += font.cell.w;
      }
    },
  };
  return font;
}

export async function loadFont(spec: SkinManifestV2["text"]["font"], baseUrl: string): Promise<BitmapFont> {
  const atlas = await loadImage(baseUrl + spec.atlas);
  return createFont(spec, atlas);
}
