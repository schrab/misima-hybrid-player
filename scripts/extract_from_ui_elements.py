"""
Place faders from UI_elements.png correctly:
- origin = TOP of travel (knob at MAX value)
- travel  = stroke length downward to MIN
- each knob gets a unique slot (no two knobs on one line)
Also ensures waterfall is off (that was the green-line square).
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from PIL import Image

MAIN = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp")
REPO = MAIN / ".worktrees" / "skinnable-player-mvp"
UI = REPO / "skins" / "misima-hybrid" / "sprites" / "ui"
LAYER = MAIN / "gfx" / "UI_elements.png"
SKIN = REPO / "skins" / "misima-hybrid" / "skin.json"
PUBLIC = REPO / "app" / "public" / "sprite" / "skin.json"

KNOBS = [
    ("volume", "volume", "knob_volume.png", [0, 1], 0.8),
    ("pitch", "pitch", "knob_pitch.png", [-12, 12], 0),
    ("reverb", "reverb", "knob_reverb.png", [0, 1], 0.15),
    ("eq1", "eq0", "knob_eq_1.png", [-12, 12], 0),
    ("eq2", "eq1", "knob_eq_2.png", [-12, 12], 0),
    ("eq3", "eq2", "knob_eq_3.png", [-12, 12], 0),
    ("eq4", "eq3", "knob_eq_4.png", [-12, 12], 0),
    ("eq5", "eq4", "knob_eq_5.png", [-12, 12], 0),
    ("eq6", "eq5", "knob_eq_6.png", [-12, 12], 0),
    ("eq7", "eq6", "knob_eq_7.png", [-12, 12], 0),
    ("eq8", "eq7", "knob_eq_8.png", [-12, 12], 0),
    ("eq9", "eq8", "knob_eq_9.png", [-12, 12], 0),
    ("eq10", "eq9", "knob_eq_10.png", [-12, 12], 0),
    ("tempo", "speed", "knob_tempo.png", [0.5, 2], 1.0),
]


def match(scene: np.ndarray, templ: np.ndarray) -> tuple[int, int, float]:
    sh, sw = scene.shape[:2]
    th, tw = templ.shape[:2]
    sa = scene[:, :, 3] / 255.0
    srgb = scene[:, :, :3]
    ta = templ[:, :, 3] / 255.0
    trgb = templ[:, :, :3]

    def score_at(x: int, y: int) -> float:
        m = sa[y : y + th, x : x + tw] * ta
        if m.sum() < ta.sum() * 0.5:
            return -1e9
        d = np.abs(srgb[y : y + th, x : x + tw] - trgb) * m[:, :, None]
        return -float(d.sum() / (m.sum() * 3 + 1e-6))

    best = (0, 0, -1e18)
    for y in range(0, sh - th + 1, 3):
        for x in range(0, sw - tw + 1, 3):
            s = score_at(x, y)
            if s > best[2]:
                best = (x, y, s)
    bx, by = best[0], best[1]
    for y in range(max(0, by - 3), min(sh - th, by + 4)):
        for x in range(max(0, bx - 3), min(sw - tw, bx + 4)):
            s = score_at(x, y)
            if s > best[2]:
                best = (x, y, s)
    return best


def track_span(al: np.ndarray, x: int, y: int, w: int, h: int) -> tuple[int, int, int]:
    """
    From knob bbox, scan up/down along the column for continuous alpha
    (track/cap art). Return (top_y, bottom_y, travel).
    origin for MAX = top_y (or knob y if no art above).
    travel = bottom_y - top_y - knob_h.
    """
    cx = x + w // 2
    col = al[:, max(0, cx - 3) : cx + 4].max(axis=1)
    # expand up from knob top
    top = y
    while top > 0 and col[top - 1] > 25:
        top -= 1
    bottom = y + h
    while bottom < al.shape[0] - 1 and col[bottom] > 25:
        bottom += 1
    travel = max(80, bottom - top - h)
    return top, bottom, travel


def main() -> None:
    layer = np.asarray(Image.open(LAYER).convert("RGBA"), dtype=np.float32)
    al = layer[:, :, 3]
    hits = []
    for fid, param, png, rng, val in KNOBS:
        templ = np.asarray(Image.open(UI / png).convert("RGBA"), dtype=np.float32)
        x, y, sc = match(layer, templ)
        th, tw = templ.shape[:2]
        top, bottom, travel = track_span(al, x, y, tw, th)
        hits.append(
            {
                "id": fid,
                "param": param,
                "knob": f"ui/{png}",
                "range": rng,
                "value": val,
                "knobSize": {"w": tw, "h": th},
                "match": (x, y, sc),
                "top": top,
                "bottom": bottom,
                "travel": travel,
            }
        )
        print(f"{fid:8} match=({x:4},{y:4}) top={top} bot={bottom} travel={travel} sc={sc:.1f}")

    # Unique X slots: sort by match x, nudge if collision
    hits.sort(key=lambda h: h["match"][0])
    used_x = []
    for h in hits:
        x = h["match"][0]
        if used_x and abs(x - used_x[-1]) < 20:
            x = used_x[-1] + 28  # force unique column
        used_x.append(x)
        h["slot_x"] = x
    # restore artist L→R param order for ids that must stay semantic
    by_id = {h["id"]: h for h in hits}
    faders = []
    for fid, param, png, rng, val in KNOBS:
        h = by_id[fid]
        faders.append(
            {
                "id": fid,
                "param": param,
                "orientation": "vertical",
                "origin": {"x": h["slot_x"], "y": h["top"]},
                "travel": h["travel"],
                "knob": f"ui/{png}",
                "knobSize": h["knobSize"],
                "knobHotspot": "top-left",
                "range": rng,
                "value": val,
            }
        )

    skin = json.loads(SKIN.read_text())
    skin["faders"] = faders
    # green-line square = waterfall
    skin.setdefault("visuals", {})["waterfall"] = {
        "origin": {"x": 0, "y": 0},
        "size": {"w": 0, "h": 0},
        "mode": "off",
        "color": "#3dffb5",
    }
    text = json.dumps(skin, indent=2)
    SKIN.write_text(text)
    PUBLIC.write_text(text)
    print("\nfinal:")
    for f in faders:
        print(f"  {f['id']:8} origin={f['origin']} travel={f['travel']} val={f['value']}")


if __name__ == "__main__":
    main()
