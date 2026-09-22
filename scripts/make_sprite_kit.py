"""Generate placeholder sprite kit for the organic hand-drawn UI (skin.json v2)."""
from __future__ import annotations

import json
import zipfile
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parents[1]
SKIN = ROOT / "skins" / "misima-hybrid"
SPRITES = SKIN / "sprites"
BG = SPRITES / "bg"
UI = SPRITES / "ui"
SPECT = SPRITES / "spectrum"
FONT = SPRITES / "font"
PUBLIC = ROOT / "app" / "public" / "sprite"

CANVAS_W, CANVAS_H = 1280, 980


def ensure_dirs() -> None:
    for p in (BG, UI, SPECT, FONT, PUBLIC, PUBLIC / "bg", PUBLIC / "ui", PUBLIC / "spectrum", PUBLIC / "font"):
        p.mkdir(parents=True, exist_ok=True)


def plate(w: int, h: int, accent: tuple[int, int, int]) -> Image.Image:
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([4, 4, w - 5, h - 5], radius=36, fill=(14, 28, 32, 250), outline=(*accent, 210), width=3)
    d.rounded_rectangle([16, 16, w - 17, h - 17], radius=28, outline=(94, 200, 255, 70), width=1)
    # wireframe orb motif
    cx, cy, r = 90, h // 2, min(70, h // 3)
    d.ellipse([cx - r, cy - r, cx + r, cy + r], outline=(61, 255, 181, 150), width=1)
    d.line([cx - r, cy, cx + r, cy], fill=(61, 255, 181, 90), width=1)
    d.line([cx, cy - r, cx, cy + r], fill=(61, 255, 181, 90), width=1)
    return img


def knob() -> Image.Image:
    img = Image.new("RGBA", (22, 28), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([2, 2, 19, 17], radius=8, fill=(255, 79, 216, 240), outline=(255, 180, 230, 255), width=1)
    d.rounded_rectangle([8, 16, 13, 26], radius=2, fill=(255, 79, 216, 230))
    return img


def button(w: int = 52, h: int = 40, pressed: bool = False) -> Image.Image:
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    fill = (61, 255, 181, 220) if not pressed else (40, 160, 120, 240)
    d.rounded_rectangle([1, 1, w - 2, h - 2], radius=8, fill=(18, 36, 40, 230), outline=fill, width=2)
    return img


def spectrum_sheet(cols: int = 10, rows: int = 12, cw: int = 64, ch: int = 12) -> Image.Image:
    img = Image.new("RGBA", (cols * cw, rows * ch), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    for col in range(cols):
        for row in range(rows):
            # row 0 = empty (bottom index later); higher row = more energy painted
            e = row / max(1, rows - 1)
            x0, y0 = col * cw, row * ch
            if row == 0:
                d.rectangle([x0 + 2, y0 + 2, x0 + cw - 3, y0 + ch - 3], outline=(40, 80, 80, 120), width=1)
                continue
            g = int(80 + 175 * e)
            r = int(61 + 100 * e)
            b = int(181 - 40 * e)
            d.rounded_rectangle(
                [x0 + 2, y0 + 1, x0 + cw - 3, y0 + ch - 2],
                radius=3,
                fill=(r, g, b, 230),
            )
    return img


def glyph_atlas(cols: int = 32, rows: int = 4, cw: int = 10, ch: int = 18) -> Image.Image:
    """Simple readable mono-ish placeholders; artist replaces with alien glyphs."""
    img = Image.new("RGBA", (cols * cw, rows * ch), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # Draw a stick pattern per cell so letters are distinguishable; production atlas is custom art.
    for row in range(rows):
        for col in range(cols):
            x0, y0 = col * cw, row * ch
            d.rectangle([x0 + 1, y0 + 1, x0 + cw - 2, y0 + ch - 2], outline=(61, 255, 181, 90), width=1)
            d.point((x0 + cw // 2, y0 + ch // 2), fill=(255, 79, 216, 255))
    return img


def write_glyph_map() -> dict:
    chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .-:+*#_[]()"
    mapping = {}
    for i, ch in enumerate(chars):
        mapping[ch] = [i % 32, i // 32]
        if ch.isalpha():
            mapping[ch.lower()] = mapping[ch]
    return mapping


def copy_public() -> None:
    for folder in (BG, UI, SPECT, FONT):
        dest = PUBLIC / folder.name
        dest.mkdir(parents=True, exist_ok=True)
        for p in folder.iterdir():
            if p.is_file():
                (dest / p.name).write_bytes(p.read_bytes())
    # skin.json for the app (paths rewritten to /sprite/...)
    src = json.loads((SKIN / "skin.json").read_text(encoding="utf-8"))
    def fix(path: str) -> str:
        if path.startswith("sprites/"):
            return path[len("sprites/") :]
        return path

    for block in src["blocks"].values():
        block["image"] = fix(block["image"])
    for f in src["faders"]:
        f["knob"] = fix(f["knob"])
    for b in src["buttons"]:
        b["frames"]["normal"] = fix(b["frames"]["normal"])
        if "pressed" in b["frames"]:
            b["frames"]["pressed"] = fix(b["frames"]["pressed"])
    src["visuals"]["spectrum"]["sheet"] = fix(src["visuals"]["spectrum"]["sheet"])
    src["text"]["font"]["atlas"] = fix(src["text"]["font"]["atlas"])
    (PUBLIC / "skin.json").write_text(json.dumps(src, indent=2), encoding="utf-8")


def write_skin_json(glyph_map: dict) -> None:
    # Layout matches the sketch blocks: visualizer top, EQ mid, playlist bottom-right.
    skin = {
        "formatVersion": 2,
        "id": "misima-hybrid",
        "name": "Misima Hybrid",
        "author": "Misima",
        "canvas": {"width": CANVAS_W, "height": CANVAS_H, "scale": 1},
        "blocks": {
            "visualizer": {
                "image": "sprites/bg/visualizer.png",
                "origin": {"x": 0, "y": 0},
                "size": {"w": 1280, "h": 400},
                "hit": "auto-alpha",
                "drag": [{"x": 24, "y": 12, "w": 420, "h": 36}],
            },
            "eq": {
                "image": "sprites/bg/eq.png",
                "origin": {"x": 0, "y": 410},
                "size": {"w": 1280, "h": 240},
                "hit": "auto-alpha",
                "drag": [{"x": 24, "y": 12, "w": 300, "h": 32}],
            },
            "playlist": {
                "image": "sprites/bg/playlist.png",
                "origin": {"x": 620, "y": 660},
                "size": {"w": 560, "h": 300},
                "hit": "auto-alpha",
                "drag": [{"x": 24, "y": 12, "w": 240, "h": 32}],
            },
        },
        "faders": [
            {
                "id": "volume",
                "param": "volume",
                "orientation": "vertical",
                "origin": {"x": 220, "y": 520},
                "travel": 120,
                "knob": "sprites/ui/knob_magenta.png",
                "knobHotspot": "center",
                "range": [0, 1],
                "value": 0.8,
            },
            {
                "id": "pitch",
                "param": "pitch",
                "orientation": "vertical",
                "origin": {"x": 300, "y": 520},
                "travel": 120,
                "knob": "sprites/ui/knob_magenta.png",
                "range": [-12, 12],
                "value": 0,
                "unit": "st",
            },
            {
                "id": "reverb",
                "param": "reverb",
                "orientation": "vertical",
                "origin": {"x": 380, "y": 520},
                "travel": 120,
                "knob": "sprites/ui/knob_magenta.png",
                "range": [0, 1],
                "value": 0.15,
            },
        ]
        + [
            {
                "id": f"eq{i}",
                "param": f"eq{i}",
                "orientation": "vertical",
                "origin": {"x": 480 + i * 70, "y": 520},
                "travel": 120,
                "knob": "sprites/ui/knob_magenta.png",
                "range": [-12, 12],
                "value": 0,
                "unit": "db",
            }
            for i in range(10)
        ]
        + [
            {
                "id": "speed",
                "param": "speed",
                "orientation": "vertical",
                "origin": {"x": 1220, "y": 520},
                "travel": 120,
                "knob": "sprites/ui/knob_magenta.png",
                "range": [0.5, 2],
                "value": 1.0,
                "unit": "x",
            }
        ],
        "buttons": [
            {
                "id": "prev",
                "action": "prev",
                "origin": {"x": 40, "y": 330},
                "size": {"w": 52, "h": 40},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
            {
                "id": "play",
                "action": "play",
                "origin": {"x": 104, "y": 330},
                "size": {"w": 52, "h": 40},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
            {
                "id": "pause",
                "action": "pause",
                "origin": {"x": 168, "y": 330},
                "size": {"w": 52, "h": 40},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
            {
                "id": "stop",
                "action": "stop",
                "origin": {"x": 232, "y": 330},
                "size": {"w": 52, "h": 40},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
            {
                "id": "next",
                "action": "next",
                "origin": {"x": 296, "y": 330},
                "size": {"w": 52, "h": 40},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
            {
                "id": "open",
                "action": "open",
                "origin": {"x": 364, "y": 330},
                "size": {"w": 72, "h": 40},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
            {
                "id": "reset_eq",
                "action": "reset_eq",
                "origin": {"x": 1180, "y": 430},
                "size": {"w": 72, "h": 32},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
            {
                "id": "clear",
                "action": "clear",
                "origin": {"x": 1120, "y": 680},
                "size": {"w": 64, "h": 32},
                "frames": {
                    "normal": "sprites/ui/btn_normal.png",
                    "pressed": "sprites/ui/btn_pressed.png",
                },
            },
        ],
        "visuals": {
            "spectrum": {
                "origin": {"x": 360, "y": 80},
                "size": {"w": 820, "h": 200},
                "bands": 10,
                "cell": {"w": 64, "h": 12},
                "frames": 12,
                "sheet": "sprites/spectrum/sheet.png",
                "align": "bottom",
                "gapPx": 8,
            },
            "waterfall": {
                "origin": {"x": 120, "y": 90},
                "size": {"w": 200, "h": 160},
                "mode": "phase3d",
                "color": "#3dffb5",
            },
        },
        "text": {
            "font": {
                "atlas": "sprites/font/glyphs.png",
                "cell": {"w": 10, "h": 18},
                "map": glyph_map,
                "fallback": "?",
            },
            "playlist": {
                "origin": {"x": 680, "y": 740},
                "rows": 10,
                "rowHeight": 20,
                "columns": [
                    {"id": "index", "width": 28},
                    {"id": "title", "width": 380},
                    {"id": "duration", "width": 56, "align": "right"},
                ],
            },
            "status": {"origin": {"x": 40, "y": 375}},
        },
    }
    (SKIN / "skin.json").write_text(json.dumps(skin, indent=2), encoding="utf-8")


def pack() -> None:
    mskin = ROOT / "skins" / "misima-hybrid.mskin"
    with zipfile.ZipFile(mskin, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in SKIN.rglob("*"):
            if p.is_file() and p.suffix.lower() in {".png", ".json"}:
                zf.write(p, p.relative_to(SKIN).as_posix())


def main() -> None:
    ensure_dirs()
    plate(1280, 400, (61, 255, 181)).save(BG / "visualizer.png")
    plate(1280, 240, (255, 79, 216)).save(BG / "eq.png")
    plate(560, 300, (179, 136, 255)).save(BG / "playlist.png")
    knob().save(UI / "knob_magenta.png")
    button().save(UI / "btn_normal.png")
    button(pressed=True).save(UI / "btn_pressed.png")
    spectrum_sheet().save(SPECT / "sheet.png")
    glyph_atlas().save(FONT / "glyphs.png")
    gmap = write_glyph_map()
    (FONT / "glyphs.json").write_text(json.dumps(gmap, indent=2), encoding="utf-8")
    write_skin_json(gmap)
    copy_public()
    pack()
    print("sprite kit + skin.json v2 + public/sprite ready")


if __name__ == "__main__":
    main()
