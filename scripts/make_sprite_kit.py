"""
Regenerate ONLY derived artifacts (public copy, .mskin pack, spectrum auto-bands
if missing). NEVER rewrite faders/buttons/artist anchors in skin.json.

SOURCE OF TRUTH = skins/misima-hybrid/skin.json (artist-measured).
"""
from __future__ import annotations

import json
import shutil
import zipfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SKIN = ROOT / "skins" / "misima-hybrid"
SRC = SKIN / "sprites"
PUBLIC = ROOT / "app" / "public" / "sprite"

CANVAS_W, CANVAS_H = 1500, 2060
SCALE = 2

# Known artist measurements (do not invent others)
BAND_LEFT_X = [370, 411, 451, 496, 546, 602, 655, 703, 750, 805]
BOTTOM_Y = 585
PLAYLIST = {
    "origin": {"x": 1024, "y": 1490},
    "size": {"w": 378, "h": 310},
    "rows": 10,
    "rowHeight": 31,
    "columns": [
        {"id": "index", "width": 48},
        {"id": "title", "width": 234},
        {"id": "duration", "width": 96, "align": "right"},
    ],
}


def load_or_init() -> dict:
    path = SKIN / "skin.json"
    if path.exists():
        return json.loads(path.read_text(encoding="utf-8"))

    # First-ever bootstrap only — empty shell so recover_anchors can fill.
    return {
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
        "faders": [],
        "buttons": [],
        "visuals": {
            "spectrum": {"mode": "segments", "bands": [], "auto": {
                "bandLeftX": BAND_LEFT_X,
                "bottomY": BOTTOM_Y,
                "segmentsPerBand": 10,
                "overlap": 0.5,
                "chips": [f"spectrum/chip_{i}.png" for i in range(10)],
            }},
            "waterfall": {"origin": {"x": 0, "y": 0}, "size": {"w": 0, "h": 0}, "mode": "off", "color": "#3dffb5"},
        },
        "text": {
            "font": {
                "atlas": "font/glyphs.png",
                "cell": {"w": 24, "h": 24},
                "classes": {
                    "digit": {"cell": {"w": 24, "h": 24}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 0}},
                    "letter": {"cell": {"w": 36, "h": 18}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 120}},
                    "symbol": {"cell": {"w": 24, "h": 18}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 72}},
                },
                "map": {"?": {"col": 0, "row": 0, "class": "symbol"}},
                "fallback": "?",
            },
            "playlist": PLAYLIST,
            "status": {"origin": {"x": 120, "y": 1980}},
        },
    }


def auto_layout_bands(skin: dict) -> None:
    """Fill spectrum.bands from auto if empty. Does not touch faders/buttons."""
    spect = skin.setdefault("visuals", {}).setdefault("spectrum", {})
    if spect.get("bands"):
        return  # artist or recover already set — leave alone
    auto = spect.get("auto") or {}
    lefts = auto.get("bandLeftX") or BAND_LEFT_X
    bottom_y = auto.get("bottomY", BOTTOM_Y)
    per_band = auto.get("segmentsPerBand", 10)
    overlap = auto.get("overlap", 0.5)
    chips = auto.get("chips") or [f"spectrum/chip_{i}.png" for i in range(10)]
    sizes = []
    for p in chips:
        fp = SRC / p
        sizes.append(Image.open(fp).size if fp.exists() else (42, 32))
    jitter = [0, 2, -2, 3, -1, 1, -3, 2, -1, 0]
    bands = []
    for b, left_x in enumerate(lefts):
        segments = []
        cursor_y = float(bottom_y)
        for s in range(per_band):
            ci = s % len(chips)
            w, h = sizes[ci]
            top_y = cursor_y - h
            jx = jitter[s % len(jitter)]
            segments.append({
                "image": chips[ci],
                "origin": {"x": left_x + jx, "y": int(round(top_y))},
                "reveal": round(min(1.0, s / max(1, per_band - 1)), 3),
            })
            cursor_y = top_y + h * overlap
        bands.append({"id": b, "segments": segments})
    spect["bands"] = bands


def ensure_known_measures(skin: dict) -> None:
    """Apply only values the artist explicitly gave (playlist, spectrum, font sizes)."""
    skin.setdefault("text", {})["playlist"] = PLAYLIST
    spect = skin.setdefault("visuals", {}).setdefault("spectrum", {})
    auto = spect.setdefault("auto", {})
    auto.setdefault("bandLeftX", BAND_LEFT_X)
    auto.setdefault("bottomY", BOTTOM_Y)
    # NEVER invent faders/buttons here.
    if "faders" not in skin:
        skin["faders"] = []
    if "buttons" not in skin:
        skin["buttons"] = []
    if "waterfall" not in skin.setdefault("visuals", {}):
        skin["visuals"]["waterfall"] = {
            "origin": {"x": 0, "y": 0}, "size": {"w": 0, "h": 0}, "mode": "off", "color": "#3dffb5"
        }


def natural_sizes(skin: dict) -> None:
    for f in skin.get("faders") or []:
        p = SRC / f.get("knob", "")
        if p.exists():
            w, h = Image.open(p).size
            f["knobSize"] = {"w": w, "h": h}
    for b in skin.get("buttons") or []:
        rel = (b.get("frames") or {}).get("pressed")
        if rel and (SRC / rel).exists():
            w, h = Image.open(SRC / rel).size
            b["size"] = {"w": w, "h": h}


def copy_public() -> None:
    if PUBLIC.exists():
        shutil.rmtree(PUBLIC)
    shutil.copytree(SRC, PUBLIC)
    shutil.copy2(SKIN / "skin.json", PUBLIC / "skin.json")


def pack() -> None:
    mskin = SKIN / "misima-hybrid.mskin"
    with zipfile.ZipFile(mskin, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in SKIN.rglob("*"):
            if p.is_file() and p.suffix.lower() in {".png", ".json"}:
                zf.write(p, p.relative_to(SKIN).as_posix())


def main() -> None:
    skin = load_or_init()
    ensure_known_measures(skin)
    natural_sizes(skin)
    auto_layout_bands(skin)
    # Write ONLY after merge — faders/buttons come from skin.json or recover_anchors.py
    (SKIN / "skin.json").write_text(json.dumps(skin, indent=2), encoding="utf-8")
    copy_public()
    pack()
    print("preserved faders", len(skin.get("faders") or []), "buttons", len(skin.get("buttons") or []))
    print("spectrum auto", skin["visuals"]["spectrum"].get("auto", {}).get("bandLeftX"))
    print("playlist", skin["text"]["playlist"]["origin"])


if __name__ == "__main__":
    main()
