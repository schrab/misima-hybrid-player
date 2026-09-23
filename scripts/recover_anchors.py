"""
Recover fader tracks + button hit boxes from bg.png.

bg.png has TRACKS and IDLE BUTTONS — not knobs.
Knobs (ui/knob_*.png) are overlays placed at the TOP of each track (max value).

SOURCE OF TRUTH for faders/buttons is skin.json after this file writes once.
make_sprite_kit.py must not overwrite non-placeholder anchors.
"""
from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "skins" / "misima-hybrid" / "sprites"
UI = SRC / "ui"
SKIN = ROOT / "skins" / "misima-hybrid" / "skin.json"

# Artist order L→R on the EQ plate
FADER_ORDER = [
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


def find_tracks(rgb: np.ndarray, alpha: np.ndarray) -> list[dict]:
    """
    Vertical capsule tracks in the EQ plate (mid artboard).
    Detected as columns with long runs of "channel" interior: slightly
    darker/lighter than both flanks, continuous height > 120.
    """
    h, w = rgb.shape
    y0, y1 = 700, 1300
    tracks = []
    for x in range(40, w - 40):
        L = rgb[y0:y1, x - 10 : x - 4].mean(axis=1)
        R = rgb[y0:y1, x + 4 : x + 10].mean(axis=1)
        M = rgb[y0:y1, x]
        # Channel: mid differs from both sides consistently (either direction)
        gap = np.minimum(np.abs(M - L), np.abs(M - R))
        inside = alpha[y0:y1, x] > 40
        is_ch = inside & (gap > 6) & (np.abs(L - R) < 40)
        run = 0
        best = 0
        best_y = (0, 0)
        y = 0
        n = y1 - y0
        while y < n:
            if is_ch[y]:
                s = y
                while y < n and is_ch[y]:
                    y += 1
                ln = y - s
                if ln > best:
                    best = ln
                    best_y = (y0 + s, y0 + y)
            else:
                y += 1
        if best >= 130:
            tracks.append(
                {"x": x, "y0": best_y[0], "y1": best_y[1], "h": best}
            )
    # cluster x
    tracks.sort(key=lambda t: t["x"])
    out: list[dict] = []
    for t in tracks:
        if out and abs(t["x"] - out[-1]["x"]) < 20:
            p = out[-1]
            p["x"] = (p["x"] + t["x"]) // 2
            p["y0"] = min(p["y0"], t["y0"])
            p["y1"] = max(p["y1"], t["y1"])
            p["h"] = p["y1"] - p["y0"]
        else:
            out.append(dict(t))
    return out


def find_button_boxes(rgb: np.ndarray, alpha: np.ndarray, buttons: list[tuple]) -> list[dict]:
    """
    Idle buttons are painted on green wings / plates.
    Find compact opaque blobs sized like each button PNG in those regions.
    Uses multi-scale sum-of-abs on alpha edges — cheap block search.
    """
    results = []
    for bid, action, png in buttons:
        path = UI / png
        if not path.exists():
            results.append(None)
            continue
        templ = np.asarray(Image.open(path).convert("RGBA"), dtype=np.float32)
        th, tw = templ.shape[:2]
        ta = templ[:, :, 3] / 255.0
        # search only in plate interiors (left wing, right wing, playlist)
        regions = [
            (20, 250, 200, 700),  # left green transport
            (1250, 250, 1500, 900),  # right green
            (850, 1350, 1500, 2000),  # playlist
            (40, 350, 500, 750),  # transport row under vis
            (80, 700, 400, 1200),  # fx left of EQ
        ]
        best = (0, 0, -1.0)
        for x0, y0, x1, y1 in regions:
            step = 4
            for y in range(y0, max(y0, y1 - th), step):
                for x in range(x0, max(x0, x1 - tw), step):
                    sa = alpha[y : y + th, x : x + tw]
                    if sa.shape[0] < th or sa.shape[1] < tw:
                        continue
                    m = sa * ta
                    if m.sum() < ta.sum() * 0.35:
                        continue
                    prgb = rgb[y : y + th, x : x + tw]
                    trgb = templ[:, :, :3]
                    mae = float(np.abs(prgb - trgb).mean())
                    score = -mae + 0.01 * float(m.sum())
                    if score > best[2]:
                        best = (x, y, score)
        results.append(
            {
                "id": bid,
                "action": action,
                "origin": {"x": best[0], "y": best[1]},
                "size": {"w": tw, "h": th},
                "frames": {"pressed": f"ui/{png}"},
                "score": best[2],
            }
        )
    return [r for r in results if r]


def main() -> None:
    img = Image.open(SRC / "bg" / "bg.png").convert("RGBA")
    a = np.asarray(img, dtype=np.float32)
    rgb, alpha = a[:, :, :3], a[:, :, 3] / 255.0
    print("bg", img.size)

    tracks = find_tracks(a[:, :, :3].mean(axis=2), a[:, :, 3])
    print(f"tracks found: {len(tracks)}")
    for t in tracks:
        print(f"  x={t['x']:4} y0={t['y0']} y1={t['y1']} h={t['h']}")

    # Keep the N tallest / most track-like, sorted by X
    tracks = sorted(tracks, key=lambda t: t["x"])
    # If we have more than 14, keep the 14 with best height uniformity
    if len(tracks) > 14:
        tracks = sorted(tracks, key=lambda t: -t["h"])[:14]
        tracks = sorted(tracks, key=lambda t: t["x"])
    if len(tracks) < 14:
        print(f"WARNING: only {len(tracks)} tracks for 14 faders")

    faders = []
    for i, (fid, param, png, rng, val) in enumerate(FADER_ORDER):
        kpath = UI / png
        kw, kh = (42, 32)
        if kpath.exists():
            kw, kh = Image.open(kpath).size
        if i < len(tracks):
            t = tracks[i]
            origin = {"x": t["x"] - kw // 2, "y": t["y0"]}
            travel = max(40, t["y1"] - t["y0"] - kh)
        else:
            # right-side leftovers: spread along known bandLeft? use last track + step
            last = tracks[-1] if tracks else {"x": 400 + i * 70, "y0": 900, "y1": 1150}
            origin = {"x": last["x"] + 70 * (i - len(tracks) + 1) - kw // 2, "y": last["y0"]}
            travel = max(40, last["y1"] - last["y0"] - kh)
        faders.append(
            {
                "id": fid,
                "param": param,
                "orientation": "vertical",
                "origin": origin,
                "travel": travel,
                "knob": f"ui/{png}",
                "knobSize": {"w": kw, "h": kh},
                "knobHotspot": "top-left",
                "range": rng,
                "value": val,
            }
        )

    print("matching buttons (idle art is ON the plate)…")
    buttons = find_button_boxes(rgb, alpha, BUTTONS)
    for b in buttons:
        print(f"  {b['id']:16} origin={b['origin']} score={b.get('score', 0):.1f}")

    skin = json.loads(SKIN.read_text(encoding="utf-8"))
    skin["faders"] = faders
    skin["buttons"] = buttons
    SKIN.write_text(json.dumps(skin, indent=2), encoding="utf-8")

    # refresh public copy
    pub = ROOT / "app" / "public" / "sprite" / "skin.json"
    pub.write_text(json.dumps(skin, indent=2), encoding="utf-8")

    print("\nRECOVERED faders:")
    for f in faders:
        print(f"  {f['id']:8} origin=({f['origin']['x']},{f['origin']['y']}) travel={f['travel']}")
    print("Wrote", SKIN)


if __name__ == "__main__":
    main()
