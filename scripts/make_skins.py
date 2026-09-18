from pathlib import Path
from PIL import Image, ImageDraw, ImageFilter
import zipfile
import math

root = Path(__file__).resolve().parents[1]


def neon_plate(w: int, h: int, accent: tuple[int, int, int], title_hint: str = "") -> Image.Image:
    """Organic dark plate with neon rim, glyphs, and wireframe motif from the sketch."""
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    # Soft glow layer then sharp plate
    glow = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    gd = ImageDraw.Draw(glow)
    gd.rounded_rectangle([8, 8, w - 9, h - 9], radius=36, fill=(*accent, 40))
    glow = glow.filter(ImageFilter.GaussianBlur(12))
    img.alpha_composite(glow)

    d = ImageDraw.Draw(img)
    # Plate body
    d.rounded_rectangle([6, 6, w - 7, h - 7], radius=32, fill=(14, 28, 32, 245), outline=(*accent, 200), width=2)
    # Inner rim
    d.rounded_rectangle([14, 14, w - 15, h - 15], radius=26, outline=(94, 200, 255, 90), width=1)
    # Corner accents
    for x, y in [(24, 24), (w - 40, 24), (24, h - 40), (w - 40, h - 40)]:
        d.ellipse([x, y, x + 10, y + 10], outline=(*accent, 160), width=1)

    # Wireframe orb (sketch motif)
    cx, cy = int(w * 0.12), int(h * 0.5)
    r = min(w, h) // 5
    d.ellipse([cx - r, cy - r, cx + r, cy + r], outline=(61, 255, 181, 140), width=1)
    for i in range(1, 5):
        dy = int(r * math.sin(i * 0.7))
        d.arc([cx - r, cy - r + i * 6, cx + r, cy + r - i * 6], 0, 360, fill=(61, 255, 181, 70), width=1)
    d.line([cx - r, cy, cx + r, cy], fill=(94, 200, 255, 90), width=1)
    d.line([cx, cy - r, cx, cy + r], fill=(94, 200, 255, 90), width=1)

    # Glyph row
    gx = int(w * 0.28)
    for i in range(8):
        col = (255, 79, 216, 180) if i % 3 == 0 else (61, 255, 181, 140)
        d.rectangle([gx + i * 18, h - 36, gx + i * 18 + 10, h - 26], outline=col, width=1)

    # Waveform trace
    pts = []
    for x in range(int(w * 0.3), int(w * 0.9), 4):
        y = int(h * 0.35 + 18 * math.sin(x / 18.0) * math.sin(x / 47.0))
        pts.append((x, y))
    if len(pts) > 1:
        d.line(pts, fill=(255, 79, 216, 200), width=2)

    if title_hint:
        # faux alien bars (no real font dependency)
        bx = int(w * 0.72)
        for i in range(6):
            d.rectangle([bx + i * 12, 22, bx + i * 12 + 6, 34], fill=(179, 136, 255, 160))

    return img


def button(w: int, h: int, accent: tuple[int, int, int], mark: str) -> Image.Image:
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([1, 1, w - 2, h - 2], radius=6, fill=(18, 36, 40, 230), outline=(*accent, 200), width=1)
    # simple glyph
    cx, cy = w // 2, h // 2
    if mark == "prev":
        d.polygon([(cx + 6, cy - 7), (cx - 2, cy), (cx + 6, cy + 7)], fill=(*accent, 230))
        d.rectangle([cx - 7, cy - 7, cx - 4, cy + 7], fill=(*accent, 230))
    elif mark == "play":
        d.polygon([(cx - 5, cy - 8), (cx + 8, cy), (cx - 5, cy + 8)], fill=(*accent, 230))
    elif mark == "pause":
        d.rectangle([cx - 6, cy - 7, cx - 2, cy + 7], fill=(*accent, 230))
        d.rectangle([cx + 2, cy - 7, cx + 6, cy + 7], fill=(*accent, 230))
    elif mark == "stop":
        d.rectangle([cx - 6, cy - 6, cx + 6, cy + 6], fill=(*accent, 230))
    elif mark == "next":
        d.polygon([(cx - 6, cy - 7), (cx + 2, cy), (cx - 6, cy + 7)], fill=(*accent, 230))
        d.rectangle([cx + 4, cy - 7, cx + 7, cy + 7], fill=(*accent, 230))
    return img


def main() -> None:
    native = root / "skins" / "misima-hybrid"
    (native / "assets").mkdir(parents=True, exist_ok=True)
    public_skin = root / "app" / "public" / "skin"
    public_skin.mkdir(parents=True, exist_ok=True)

    panels = {
        "panel_main.png": (920, 280, (61, 255, 181), True),
        "panel_eq.png": (920, 160, (255, 79, 216), False),
        "panel_playlist.png": (520, 320, (179, 136, 255), True),
    }
    for name, (w, h, accent, hint) in panels.items():
        img = neon_plate(w, h, accent, "t" if hint else "")
        img.save(native / "assets" / name)
        img.save(public_skin / name)

    png = root / "skins" / "winamp5-misima" / "png"
    png.mkdir(parents=True, exist_ok=True)
    neon_plate(600, 420, (61, 255, 181), "t").save(png / "player_bg.png")
    neon_plate(600, 240, (255, 79, 216), "").save(png / "eq_bg.png")
    neon_plate(480, 520, (179, 136, 255), "t").save(png / "playlist_bg.png")

    accents = (61, 255, 181)
    for name in ["prev", "play", "pause", "stop", "next"]:
        button(28, 28, accents, name).save(png / f"cbutton_{name}.png")
    thumb = Image.new("RGBA", (12, 14), (255, 79, 216, 240))
    ImageDraw.Draw(thumb).rounded_rectangle([0, 0, 11, 13], radius=3, fill=(255, 79, 216, 240))
    thumb.save(png / "volume_thumb.png")
    bar = Image.new("RGBA", (120, 12), (20, 40, 44, 220))
    ImageDraw.Draw(bar).rounded_rectangle([0, 0, 119, 11], radius=4, outline=(61, 255, 181, 160), width=1)
    bar.save(png / "volume_bar.png")

    mskin = root / "skins" / "misima-hybrid.mskin"
    with zipfile.ZipFile(mskin, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in native.rglob("*"):
            if p.is_file():
                zf.write(p, p.relative_to(native).as_posix())

    wal_src = root / "skins" / "winamp5-misima"
    wal = root / "skins" / "winamp5-misima.wal"
    with zipfile.ZipFile(wal, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in wal_src.rglob("*"):
            if p.is_file() and p.name != "README.md":
                zf.write(p, p.relative_to(wal_src).as_posix())

    print("wrote skins + public/skin assets")


if __name__ == "__main__":
    main()
