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
    """
    Build/update skin.json.
    IMPORTANT: if an existing skin.json has faders/buttons/playlist/text origins,
    those artist-measured anchors are PRESERVED (merge, do not clobber).
    """
    existing_path = SKIN / "skin.json"
    existing = {}
    if existing_path.exists():
        try:
            existing = json.loads(existing_path.read_text(encoding="utf-8"))
        except Exception:
            existing = {}

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
                # Prefilled from artist measurements (2x artboard):
                # left-border X of each band; shared bottom Y.
                "auto": {
                    "bandLeftX": [370, 411, 451, 496, 546, 602, 655, 703, 750, 805],
                    "bottomY": 585,
                    "maxHeight": 240,
                    "segmentsPerBand": 10,
                    # chip_0 = bottom base … chip_9 = top tip (artist export order)
                    "chips": [f"spectrum/chip_{i}.png" for i in range(10)],
                    "overlap": 0.5,
                },
                # Filled by auto-layout below (also written for manual tweaks)
                "bands": [],
            },
            "waterfall": {
                # off by default — green-line square until artist places it
                "origin": {"x": 0, "y": 0},
                "size": {"w": 0, "h": 0},
                "mode": "off",
                "color": "#3dffb5",
            },
        },
        "text": {
            "font": {
                "atlas": "font/glyphs.png",
                "cell": {"w": 18, "h": 18},
                "classes": {
                    # artist: digits 24×24, letters 36×18 (artboard px)
                    "digit": {"cell": {"w": 24, "h": 24}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 0}},
                    "letter": {"cell": {"w": 36, "h": 18}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 120}},
                    "symbol": {"cell": {"w": 24, "h": 18}, "baseline": "bottom", "atlasOrigin": {"x": 0, "y": 72}},
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
                # Artist rect (2x artboard): TL 1024,1490  BR 1402,1800  →  378×310, 10 lines
                "origin": {"x": 1024, "y": 1490},
                "size": {"w": 378, "h": 310},
                "rows": 10,
                "rowHeight": 31,
                "columns": [
                    {"id": "index", "width": 48},
                    {"id": "title", "width": 234},
                    {"id": "duration", "width": 96, "align": "right"},
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

    # Merge artist-measured anchors from existing skin.json
    def merge_list_by_id(new_list, old_list, keys):
        if not old_list:
            return new_list
        old_by = {item.get("id"): item for item in old_list if isinstance(item, dict)}
        out = []
        for item in new_list:
            old = old_by.get(item.get("id"))
            if not old:
                out.append(item)
                continue
            merged = dict(item)
            for k in keys:
                if k in old and old[k] is not None:
                    merged[k] = old[k]
            out.append(merged)
        return out

    if existing:
        skin["faders"] = merge_list_by_id(
            skin["faders"],
            existing.get("faders") or [],
            ["origin", "travel", "range", "value", "knobSize", "knob"],
        )
        skin["buttons"] = merge_list_by_id(
            skin["buttons"],
            existing.get("buttons") or [],
            ["origin", "size", "action", "frames"],
        )
        old_pl = (existing.get("text") or {}).get("playlist")
        if old_pl:
            skin["text"]["playlist"].update(
                {k: old_pl[k] for k in ("origin", "size", "rows", "rowHeight", "columns") if k in old_pl}
            )
        old_status = (existing.get("text") or {}).get("status")
        if old_status and "origin" in old_status:
            skin["text"]["status"] = old_status
        old_wf = (existing.get("visuals") or {}).get("waterfall")
        if old_wf:
            skin["visuals"]["waterfall"] = old_wf
        old_auto = (existing.get("visuals") or {}).get("spectrum", {}).get("auto")
        if old_auto:
            skin["visuals"]["spectrum"]["auto"].update(old_auto)

    (SKIN / "skin.json").write_text(json.dumps(skin, indent=2), encoding="utf-8")
    return skin


def copy_public() -> None:
    if PUBLIC.exists():
        shutil.rmtree(PUBLIC)
    shutil.copytree(SRC, PUBLIC)
    # Runtime fetches /sprite/skin.json — copy manifest next to assets
    src_manifest = SKIN / "skin.json"
    if src_manifest.exists():
        shutil.copy2(src_manifest, PUBLIC / "skin.json")


def pack() -> None:
    mskin = SKIN / "misima-hybrid.mskin"
    with zipfile.ZipFile(mskin, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in SKIN.rglob("*"):
            if p.is_file() and p.suffix.lower() in {".png", ".json"}:
                zf.write(p, p.relative_to(SKIN).as_posix())


def write_placeholder_segments() -> None:
    """
    Small unique chip pool (8 pieces) — replace with hand-drawn organic chips.
    Band columns are auto-placed from artist bandLeftX / bottomY (see write_skin_json).
    """
    from PIL import Image, ImageDraw

    out = SRC / "spectrum"
    out.mkdir(parents=True, exist_ok=True)
    for i in range(10):
        dest = out / f"chip_{i}.png"
        if dest.exists():
            continue
        w = 56 + (i % 4) * 10
        h = 18 + (i % 3) * 6
        img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        pts = [(1, h - 1), (w - 2, h - 3), (w - 2, 1), (w // 2, 0), (1, 3)]
        e = i / 7.0
        col = (int(60 + 80 * e), int(180 + 60 * e), int(200 - 80 * e), 230)
        d.polygon(pts, fill=col, outline=(255, 255, 255, 40))
        img.save(out / f"chip_{i}.png")


def auto_layout_bands(skin: dict) -> None:
    """chip_0 = bottom base at bottomY … chip_9 = top tip. Reveal bottom → top."""
    auto = skin["visuals"]["spectrum"]["auto"]
    lefts = auto["bandLeftX"]
    bottom_y = auto["bottomY"]
    per_band = auto.get("segmentsPerBand", 10)
    overlap = auto.get("overlap", 0.5)
    chips = auto["chips"]
    sizes = []
    for p in chips:
        fp = SRC / p
        if fp.exists():
            from PIL import Image

            sizes.append(Image.open(fp).size)
        else:
            sizes.append((42, 32))
    jitter = [0, 2, -2, 3, -1, 1, -3, 2, -1, 0]
    bands = []
    for b, left_x in enumerate(lefts):
        segments = []
        cursor_y = float(bottom_y)
        for s in range(per_band):
            ci = s % len(chips)  # 0=bottom … n-1=top
            w, h = sizes[ci]
            top_y = cursor_y - h
            jx = jitter[s % len(jitter)]
            segments.append(
                {
                    "image": chips[ci],
                    "origin": {"x": left_x + jx, "y": int(round(top_y))},
                    "reveal": round(min(1.0, s / max(1, per_band - 1)), 3),
                }
            )
            cursor_y = top_y + h * overlap
        bands.append({"id": b, "segments": segments})
    skin["visuals"]["spectrum"]["bands"] = bands


def write_font_atlas() -> None:
    """Placeholder atlas ONLY if missing — never overwrite artist glyphs.png."""
    if (SRC / "font" / "glyphs.png").exists():
        return
    from PIL import Image, ImageDraw

    atlas = Image.new("RGBA", (480, 220), (0, 0, 0, 0))
    d = ImageDraw.Draw(atlas)
    # digits 0-9 : 24×24 at y=0
    for i in range(10):
        x, y = i * 24, 0
        d.rectangle([x + 1, y + 1, x + 22, y + 22], outline=(61, 255, 181, 200))
        d.text((x + 7, y + 5), str(i), fill=(255, 79, 216, 255))
    # symbols at y=60 (24×18)
    for i, s in enumerate([" ", "-", ".", ":", "?"]):
        x, y = i * 24, 60
        d.rectangle([x + 1, y + 1, x + 22, y + 16], outline=(94, 200, 255, 120))
    # letters 36×18 at y=120
    for i in range(26):
        col, row = i % 8, i // 8
        x, y = col * 36, 120 + row * 18
        d.rectangle([x, y, x + 35, y + 17], outline=(61, 255, 181, 180))
        d.text((x + 3, y + 2), chr(ord("A") + i), fill=(61, 255, 181, 220))
    atlas.save(SRC / "font" / "glyphs.png")


def main() -> None:
    write_placeholder_segments()  # chips only; does not touch font/glyphs.png
    write_font_atlas()  # no-op if artist glyphs.png exists
    write_skin_json()
    skin = json.loads((SKIN / "skin.json").read_text(encoding="utf-8"))
    auto_layout_bands(skin)
    (SKIN / "skin.json").write_text(json.dumps(skin, indent=2), encoding="utf-8")
    copy_public()
    pack()
    print(
        "spectrum auto bands X=",
        skin["visuals"]["spectrum"]["auto"]["bandLeftX"],
        "bottomY=",
        skin["visuals"]["spectrum"]["auto"]["bottomY"],
    )


if __name__ == "__main__":
    main()
