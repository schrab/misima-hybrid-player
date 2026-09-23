"""Regenerate skin.json v2 + public/sprite from real artist assets (2x artboard)."""
from __future__ import annotations

import json
import shutil
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SKIN = ROOT / "skins" / "misima-hybrid"
SRC = SKIN / "sprites"
PUBLIC = ROOT / "app" / "public" / "sprite"

# Artboard is 2x UI. JSON coords = Photoshop pixels on this canvas (do not divide by 2).
CANVAS_W, CANVAS_H = 1500, 2060
SCALE = 2


def knob_def(fader_id: str, param: str, png: str, origin: tuple[int, int], travel: int, lo: float, hi: float, value: float) -> dict:
    return {
        "id": fader_id,
        "param": param,
        "orientation": "vertical",
        # origin = TOP-LEFT of knob sprite at MAXIMUM value (top of travel)
        "origin": {"x": origin[0], "y": origin[1]},
        # travel = Y distance (px, 2x art) from top-of-travel to top-of-travel-at-min
        "travel": travel,
        "knob": f"ui/{png}",
        "range": [lo, hi],
        "value": value,
    }


def button_def(btn_id: str, action: str, png: str, origin: tuple[int, int], w: int, h: int) -> dict:
    return {
        "id": btn_id,
        "action": action,
        # origin = TOP-LEFT of the button PNG (matches inactive art baked into bg.png)
        "origin": {"x": origin[0], "y": origin[1]},
        "size": {"w": w, "h": h},
        "frames": {
            # only ACTIVE overlay; idle state is painted in bg.png
            "pressed": f"ui/{png}"
        },
    }


def write_skin_json() -> dict:
    # Coordinates are PLACEHOLDERS in 2x artboard space — replace with Photoshop values.
    # Measure origin as TOP-LEFT of the layer bounds (Photoshop Info / W,H).
    skin = {
        "formatVersion": 2,
        "id": "misima-hybrid",
        "name": "Misima Hybrid",
        "author": "Misima",
        "units": "artboard-2x-px",
        "canvas": {"width": CANVAS_W, "height": CANVAS_H, "scale": SCALE},
        "background": {
            "image": "bg/bg.png",
            "origin": {"x": 0, "y": 0},
            "size": {"w": CANVAS_W, "h": CANVAS_H},
        },
        "faders": [
            knob_def("volume", "volume", "knob_volume.png", (120, 1500), 180, 0, 1, 0.8),
            knob_def("pitch", "pitch", "knob_pitch.png", (260, 1500), 180, -12, 12, 0),
            knob_def("reverb", "reverb", "knob_reverb.png", (400, 1500), 180, 0, 1, 0.15),
            knob_def("eq1", "eq0", "knob_eq_1.png", (560, 1500), 160, -12, 12, 0),
            knob_def("eq2", "eq1", "knob_eq_2.png", (640, 1500), 160, -12, 12, 0),
            knob_def("eq3", "eq2", "knob_eq_3.png", (720, 1500), 160, -12, 12, 0),
            knob_def("eq4", "eq3", "knob_eq_4.png", (800, 1500), 160, -12, 12, 0),
            knob_def("eq5", "eq4", "knob_eq_5.png", (880, 1500), 160, -12, 12, 0),
            knob_def("eq6", "eq5", "knob_eq_6.png", (960, 1500), 160, -12, 12, 0),
            knob_def("eq7", "eq6", "knob_eq_7.png", (1040, 1500), 160, -12, 12, 0),
            knob_def("eq8", "eq7", "knob_eq_8.png", (1120, 1500), 160, -12, 12, 0),
            knob_def("eq9", "eq8", "knob_eq_9.png", (1200, 1500), 160, -12, 12, 0),
            knob_def("eq10", "eq9", "knob_eq_10.png", (1280, 1500), 160, -12, 12, 0),
            knob_def("tempo", "speed", "knob_tempo.png", (1360, 1500), 180, 0.5, 2, 1),
        ],
        "buttons": [
            button_def("prev", "prev", "button_prev.png", (80, 300), 33, 71),
            button_def("play", "play", "button_play.png", (130, 300), 57, 63),
            button_def("stop", "stop", "button_stop.png", (200, 300), 42, 129),
            button_def("next", "next", "button_next.png", (260, 300), 45, 51),
            button_def("power", "stop", "button_power.png", (1400, 80), 45, 106),
            button_def("fx_enable", "reset_eq", "button_fx_enable.png", (80, 700), 136, 103),
            button_def("fx_reset", "reset_eq", "button_fx_reset.png", (230, 700), 91, 53),
            button_def("playlist_open", "open", "button_playlist_open.png", (900, 1700), 22, 64),
            button_def("playlist_shuffle", "next", "button_playlist_shuffle.png", (940, 1700), 29, 74),
        ],
        "visuals": {
            "spectrum": {
                "mode": "segments",
                # Irregular hand-drawn segments: bands[i].segments[0] = bottom.
                # Each segment has absolute top-left on the 2x artboard.
                "bands": [
                    {
                        "id": i,
                        "segments": [
                            {
                                "image": f"spectrum/band{i}_{j}.png",
                                "origin": {"x": 420 + i * 90 + (j % 3), "y": 520 - j * 18},
                            }
                            for j in range(8)
                        ],
                    }
                    for i in range(10)
                ],
            },
            "waterfall": {
                "origin": {"x": 120, "y": 200},
                "size": {"w": 240, "h": 360},
                "mode": "phase3d",
                "color": "#3dffb5",
            },
        },
        "text": {
            "font": {
                "atlas": "font/glyphs.png",
                "cell": {"w": 18, "h": 18},
                "classes": {
                    # digits ≈ square; letters 2:1 and slightly shorter — bottom-aligned
                    "digit": {"cell": {"w": 18, "h": 18}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 0}},
                    "letter": {"cell": {"w": 36, "h": 14}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 80}},
                    "symbol": {"cell": {"w": 18, "h": 14}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 36}},
                },
                "map": {
                    "0": {"col": 0, "row": 0, "class": "digit"},
                    "1": {"col": 1, "row": 0, "class": "digit"},
                    "2": {"col": 2, "row": 0, "class": "digit"},
                    "3": {"col": 3, "row": 0, "class": "digit"},
                    "4": {"col": 4, "row": 0, "class": "digit"},
                    "5": {"col": 5, "row": 0, "class": "digit"},
                    "6": {"col": 6, "row": 0, "class": "digit"},
                    "7": {"col": 7, "row": 0, "class": "digit"},
                    "8": {"col": 8, "row": 0, "class": "digit"},
                    "9": {"col": 9, "row": 0, "class": "digit"},
                    " ": {"col": 0, "row": 3, "class": "symbol"},
                    "-": {"col": 1, "row": 3, "class": "symbol"},
                    ".": {"col": 2, "row": 3, "class": "symbol"},
                    ":": {"col": 3, "row": 3, "class": "symbol"},
                    "?": {"col": 4, "row": 3, "class": "symbol"},
                }
                | {
                    # A–Z as wide letter cells (col 0..7 on letter grid rows 0..3)
                    **{
                        chr(ord("A") + i): {
                            "col": i % 8,
                            "row": 4 + (i // 8),
                            "class": "letter",
                        }
                        for i in range(26)
                    }
                },
                "fallback": "?",
            },
            "playlist": {
                "origin": {"x": 980, "y": 1780},
                "rows": 10,
                "rowHeight": 36,
                "columns": [
                    {"id": "index", "width": 50},
                    {"id": "title", "width": 700},
                    {"id": "duration", "width": 100, "align": "right"},
                ],
            },
            "status": {"origin": {"x": 120, "y": 1980}},
        },
    }

    # natural sizes for knobs/buttons from files (authoritative)
    def natural(rel: str) -> tuple[int, int]:
        from PIL import Image

        im = Image.open(SRC / rel)
        return im.size

    for f in skin["faders"]:
        w, h = natural(f["knob"])
        f["knobSize"] = {"w": w, "h": h}
        f["knobHotspot"] = "top-left"
    for b in skin["buttons"]:
        rel = b["frames"]["pressed"]
        w, h = natural(rel)
        b["size"] = {"w": w, "h": h}

    (SKIN / "skin.json").write_text(json.dumps(skin, indent=2), encoding="utf-8")
    return skin


def copy_public() -> None:
    if PUBLIC.exists():
        shutil.rmtree(PUBLIC)
    shutil.copytree(SRC, PUBLIC)


def pack() -> None:
    mskin = SKIN / "misima-hybrid.mskin"
    with zipfile.ZipFile(mskin, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in SKIN.rglob("*"):
            if p.is_file() and p.suffix.lower() in {".png", ".json"}:
                zf.write(p, p.relative_to(SKIN).as_posix())


def write_placeholder_segments() -> None:
    """Irregular organic segment placeholders (replace with hand-drawn pieces)."""
    from PIL import Image, ImageDraw

    out = SRC / "spectrum"
    out.mkdir(parents=True, exist_ok=True)
    for band in range(10):
        # Different heights per band (taller mid bands, like the sketch)
        n = 8 + (band % 3)
        w = 70 + (band % 4) * 8
        for j in range(n):
            h = 14 + (j * 2) + (band % 2) * 3
            img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
            d = ImageDraw.Draw(img)
            # irregular polygon — not a rectangle
            pts = [
                (2, h - 2),
                (w - 3, h - 4),
                (w - 2, 2),
                (w // 2, 1),
                (1, 4),
            ]
            e = j / max(1, n - 1)
            col = (
                int(60 + 80 * e),
                int(180 + 60 * e),
                int(200 - 80 * e),
                230,
            )
            d.polygon(pts, fill=col, outline=(255, 255, 255, 40))
            img.save(out / f"band{band}_{j}.png")


def write_font_atlas() -> None:
    """Placeholder atlas: digits (square), letters (2:1 shorter), symbols."""
    from PIL import Image, ImageDraw

    # Layout: rows 0-1 digits 18x18 at (0,0); symbols; letters 36x14 from y=80
    atlas = Image.new("RGBA", (400, 140), (0, 0, 0, 0))
    d = ImageDraw.Draw(atlas)
    # digits 0-9
    for i in range(10):
        x, y = i * 18, 0
        d.rectangle([x + 1, y + 1, x + 16, y + 16], outline=(61, 255, 181, 200), width=1)
        d.text((x + 5, y + 3), str(i), fill=(255, 79, 216, 255))
    # symbols
    for i, s in enumerate([" ", "-", ".", ":", "?"]):
        x, y = i * 18, 36
        d.rectangle([x + 1, y + 1, x + 16, y + 32], outline=(94, 200, 255, 120))
    # letters 36x14 (2:1, shorter) at y=80
    for i in range(26):
        col, row = i % 8, i // 8
        x, y = col * 36, 80 + row * 14
        d.rectangle([x, y, x + 35, y + 13], outline=(61, 255, 181, 180), width=1)
        d.text((x + 2, y + 1), chr(ord("A") + i), fill=(61, 255, 181, 220))
    atlas.save(SRC / "font" / "glyphs.png")


def main() -> None:
    write_placeholder_segments()
    write_font_atlas()
    write_skin_json()
    copy_public()
    pack()
    print("skin.json v2 (irregular spectrum + font classes) synced")


if __name__ == "__main__":
    main()
