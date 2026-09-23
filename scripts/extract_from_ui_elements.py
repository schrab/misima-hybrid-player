"""
Extract fader/button origins from gfx/UI_elements.png (knobs + faders layer).

This layer shows controls AT REST on the artboard — match ui/knob_*.png and
ui/button_*.png against it. Never uses bg.png for knobs (bg has no knobs).

Writes faders/buttons into skins/misima-hybrid/skin.json only.
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from PIL import Image

REPO = Path(__file__).resolve().parents[1]  # worktree root (.../skinnable-player-mvp)
MAIN = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp")
SRC = REPO / "skins" / "misima-hybrid" / "sprites"
UI = SRC / "ui"
LAYER = MAIN / "gfx" / "UI_elements.png"  # artist knobs+faders layer
SKIN = REPO / "skins" / "misima-hybrid" / "skin.json"
PUBLIC_SKIN = REPO / "app" / "public" / "sprite" / "skin.json"

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

BUTTONS = [
    ("prev", "prev", "button_prev.png"),
    ("play", "play", "button_play.png"),
    ("stop", "stop", "button_stop.png"),
    ("next", "next", "button_next.png"),
    ("power", "stop", "button_power.png"),
    ("fx_enable", "reset_eq", "button_fx_enable.png"),
    ("fx_reset", "reset_eq", "button_fx_reset.png"),
    ("playlist_open", "open", "button_playlist_open.png"),
    ("playlist_shuffle", "next", "button_playlist_shuffle.png"),
]


def match_on_layer(scene: np.ndarray, templ: np.ndarray) -> tuple[int, int, float]:
    """SAD match where both have alpha. stride 1 on coarse then refine."""
    sh, sw = scene.shape[:2]
    th, tw = templ.shape[:2]
    if th > sh or tw > sw:
        return 0, 0, 0.0
    sa = scene[:, :, 3] / 255.0
    srgb = scene[:, :, :3]
    ta = templ[:, :, 3] / 255.0
    trgb = templ[:, :, :3]
    if ta.sum() < 4:
        return 0, 0, 0.0

    def score_at(x: int, y: int) -> float:
        m = sa[y : y + th, x : x + tw] * ta
        if m.sum() < ta.sum() * 0.5:
            return -1e9
        diff = np.abs(srgb[y : y + th, x : x + tw] - trgb) * m[:, :, None]
        return -float(diff.sum() / (m.sum() * 3 + 1e-6))

    best = (0, 0, -1e18)
    # coarse
    for y in range(0, sh - th + 1, 4):
        for x in range(0, sw - tw + 1, 4):
            s = score_at(x, y)
            if s > best[2]:
                best = (x, y, s)
    # refine
    bx, by = best[0], best[1]
    for y in range(max(0, by - 4), min(sh - th, by + 5)):
        for x in range(max(0, bx - 4), min(sw - tw, bx + 5)):
            s = score_at(x, y)
            if s > best[2]:
                best = (x, y, s)
    return best


def travel_below(layer: np.ndarray, x: int, y: int, w: int, h: int) -> int:
    """
    Estimate fader travel: extend down the track from knob bottom while
    the layer has non-empty pixels in a thin strip under the knob (track/cap art).
    """
    alpha = layer[:, :, 3]
    strip_x0 = x + max(0, w // 2 - 4)
    strip_x1 = min(layer.shape[1], x + w // 2 + 5)
    y_from = y + h
    depth = 0
    for yy in range(y_from, min(layer.shape[0], y_from + 400)):
        row = alpha[yy, strip_x0:strip_x1]
        if row.max() < 20:
            break
        depth += 1
    return max(48, depth)


def main() -> None:
    layer = np.asarray(Image.open(LAYER).convert("RGBA"), dtype=np.float32)
    print("layer", layer.shape[1], layer.shape[0])

    faders = []
    for fid, param, png, rng, val in KNOBS:
        kp = UI / png
        if not kp.exists():
            print("missing", png)
            continue
        templ = np.asarray(Image.open(kp).convert("RGBA"), dtype=np.float32)
        x, y, sc = match_on_layer(layer, templ)
        th, tw = templ.shape[:2]
        travel = travel_below(layer, x, y, tw, th)
        print(f"{fid:8} ({x:4},{y:4}) score={sc:.1f} travel={travel} size={tw}x{th}")
        faders.append(
            {
                "id": fid,
                "param": param,
                "orientation": "vertical",
                "origin": {"x": x, "y": y},
                "travel": travel,
                "knob": f"ui/{png}",
                "knobSize": {"w": tw, "h": th},
                "knobHotspot": "top-left",
                "range": rng,
                "value": val,
            }
        )

    buttons = []
    for bid, action, png in BUTTONS:
        bp = UI / png
        if not bp.exists():
            continue
        templ = np.asarray(Image.open(bp).convert("RGBA"), dtype=np.float32)
        x, y, sc = match_on_layer(layer, templ)
        th, tw = templ.shape[:2]
        print(f"{bid:16} ({x:4},{y:4}) score={sc:.1f}")
        buttons.append(
            {
                "id": bid,
                "action": action,
                "origin": {"x": x, "y": y},
                "size": {"w": tw, "h": th},
                "frames": {"pressed": f"ui/{png}"},
            }
        )

    skin = json.loads(SKIN.read_text(encoding="utf-8"))
    skin["faders"] = faders
    skin["buttons"] = buttons
    SKIN.write_text(json.dumps(skin, indent=2), encoding="utf-8")
    if PUBLIC_SKIN.parent.exists():
        PUBLIC_SKIN.write_text(json.dumps(skin, indent=2), encoding="utf-8")
    print("wrote", SKIN)


if __name__ == "__main__":
    main()
