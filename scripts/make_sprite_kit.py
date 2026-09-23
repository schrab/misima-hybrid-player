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
                "origin": {"x": 400, "y": 200},
                "size": {"w": 900, "h": 400},
                "bands": 10,
                "cell": {"w": 64, "h": 12},
                "frames": 12,
                "sheet": "spectrum/sheet.png",
                "align": "bottom",
                "gapPx": 0,
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
                "cell": {"w": 10, "h": 18},
                "map": json.loads((SRC / "font" / "glyphs.json").read_text(encoding="utf-8")),
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


def main() -> None:
    write_skin_json()
    copy_public()
    pack()
    print("skin.json v2 (2x artboard) + public/sprite synced from real assets")


if __name__ == "__main__":
    main()
