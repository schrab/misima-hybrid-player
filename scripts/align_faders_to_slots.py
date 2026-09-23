import json
from pathlib import Path
import numpy as np
from PIL import Image

REPO = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp")
MAIN = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp")
UI = REPO / "skins/misima-hybrid/sprites/ui"
LAYER = np.asarray(Image.open(MAIN / "gfx/UI_elements.png").convert("RGBA"), dtype=np.float32)
BG = np.asarray(Image.open(REPO / "skins/misima-hybrid/sprites/bg/bg.png").convert("RGBA"), dtype=np.float32)

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


def match(scene: np.ndarray, templ: np.ndarray) -> tuple[int, int]:
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
    for y in range(0, sh - th + 1, 2):
        for x in range(0, sw - tw + 1, 2):
            s = score_at(x, y)
            if s > best[2]:
                best = (x, y, s)
    bx, by = best[0], best[1]
    for y in range(max(0, by - 2), min(sh - th, by + 3)):
        for x in range(max(0, bx - 2), min(sw - tw, bx + 3)):
            s = score_at(x, y)
            if s > best[2]:
                best = (x, y, s)
    return best[0], best[1]


# bg track spans by x
rgb = BG[:, :, :3].mean(axis=2)
alpha = BG[:, :, 3] / 255.0
y0, y1 = 700, 1300
cands = []
for x in range(40, 1460):
    L = rgb[y0:y1, x - 10 : x - 4].mean(axis=1)
    R = rgb[y0:y1, x + 4 : x + 10].mean(axis=1)
    M = rgb[y0:y1, x]
    gap = np.minimum(np.abs(M - L), np.abs(M - R))
    inside = alpha[y0:y1, x] > 0.15
    is_ch = inside & (gap > 4) & (np.abs(L - R) < 50)
    best = 0
    best_y = (y0, y0)
    yy = 0
    n = y1 - y0
    while yy < n:
        if is_ch[yy]:
            s = yy
            while yy < n and is_ch[yy]:
                yy += 1
            if yy - s > best:
                best = yy - s
                best_y = (y0 + s, y0 + yy)
        else:
            yy += 1
    if best >= 140:
        cands.append({"x": x, "y0": best_y[0], "y1": best_y[1], "h": best})
cands.sort(key=lambda t: t["x"])
slots = []
for t in cands:
    if slots and abs(t["x"] - slots[-1]["x"]) < 22:
        p = slots[-1]
        p["x"] = (p["x"] + t["x"]) // 2
        p["y0"] = min(p["y0"], t["y0"])
        p["y1"] = max(p["y1"], t["y1"])
        p["h"] = p["y1"] - p["y0"]
    else:
        slots.append(dict(t))

hits = []
for fid, param, png, rng, val in KNOBS:
    templ = np.asarray(Image.open(UI / png).convert("RGBA"), dtype=np.float32)
    x, y = match(LAYER, templ)
    tw, th = templ.shape[1], templ.shape[0]
    # nearest slot to this knob (for track top/bottom only — X stays art X)
    slot = min(slots, key=lambda s: abs(s["x"] - (x + tw // 2)))
    hits.append(
        {
            "id": fid,
            "param": param,
            "knob": f"ui/{png}",
            "range": rng,
            "value": val,
            "x": x,
            "y_art": y,
            "w": tw,
            "h": th,
            "track_y0": slot["y0"],
            "track_y1": slot["y1"],
        }
    )
    print(f"{fid:8} art=({x:4},{y:4}) track=({slot['y0']}-{slot['y1']}) w={tw} h={th}")

# De-dupe X only if two knobs share nearly the same column (eq4/eq10 bug)
hits.sort(key=lambda h: h["x"])
for i in range(1, len(hits)):
    if hits[i]["x"] - hits[i - 1]["x"] < 18:
        hits[i]["x"] = hits[i - 1]["x"] + 24
        print("nudge", hits[i]["id"], "to x", hits[i]["x"])

# origin = TOP of travel (MAX). Art rest Y is often mid/bottom — do not use it as origin.
faders = []
by = {h["id"]: h for h in hits}
for fid, param, png, rng, val in KNOBS:
    h = by[fid]
    top = h["track_y0"]
    travel = max(60, h["track_y1"] - h["track_y0"] - h["h"])
    faders.append(
        {
            "id": fid,
            "param": param,
            "orientation": "vertical",
            "origin": {"x": h["x"], "y": top},
            "travel": travel,
            "knob": h["knob"],
            "knobSize": {"w": h["w"], "h": h["h"]},
            "knobHotspot": "top-left",
            "range": rng,
            "value": val,
        }
    )
    print(f"FINAL {fid:8} origin=({h['x']},{top}) travel={travel}")

skin_path = REPO / "skins/misima-hybrid/skin.json"
pub = REPO / "app/public/sprite/skin.json"
skin = json.loads(skin_path.read_text())
skin["faders"] = faders
skin["visuals"]["waterfall"] = {
    "origin": {"x": 0, "y": 0},
    "size": {"w": 0, "h": 0},
    "mode": "off",
    "color": "#3dffb5",
}
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub.write_text(text)
print("saved")
