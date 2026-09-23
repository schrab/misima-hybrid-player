import json
from pathlib import Path
import numpy as np
from PIL import Image

REPO = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp")
MAIN = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp")
UI = REPO / "skins/misima-hybrid/sprites/ui"
LAYER = np.asarray(Image.open(MAIN / "gfx/UI_elements.png").convert("RGBA"), dtype=np.float32)
BG = np.asarray(Image.open(REPO / "skins/misima-hybrid/sprites/bg/bg.png").convert("RGBA"), dtype=np.float32)

# Semantic L→R order on the plate
ORDER = [
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


def match(scene, templ):
    sh, sw = scene.shape[:2]
    th, tw = templ.shape[:2]
    sa = scene[:, :, 3] / 255.0
    srgb = scene[:, :, :3]
    ta = templ[:, :, 3] / 255.0
    trgb = templ[:, :, :3]

    def score_at(x, y):
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
    return best[0], best[1], best[2]


# Collect ALL knob matches, then sort by X and assign names L→R
found = []
for fid, param, png, rng, val in ORDER:
    templ = np.asarray(Image.open(UI / png).convert("RGBA"), dtype=np.float32)
    x, y, sc = match(LAYER, templ)
    found.append({"png": png, "x": x, "y": y, "w": templ.shape[1], "h": templ.shape[0], "sc": sc})
    print(f"match {png:20} ({x:4},{y:4}) sc={sc:.1f}")

found.sort(key=lambda f: f["x"])
print("L→R order:", [f["png"] for f in found])
# enforce unique columns (min 36px) so two knobs never share a line
for i in range(1, len(found)):
    if found[i]["x"] - found[i - 1]["x"] < 36:
        found[i]["x"] = found[i - 1]["x"] + 36

# bg tracks
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
    best, best_y = 0, (y0, y0)
    yy, n = 0, y1 - y0
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
slots.sort(key=lambda s: s["x"])

faders = []
for i, (fid, param, png, rng, val) in enumerate(ORDER):
    f = found[i]  # i-th from the left
    slot = min(slots, key=lambda s: abs(s["x"] - (f["x"] + f["w"] // 2)))
    top = slot["y0"]
    travel = max(60, slot["y1"] - slot["y0"] - f["h"])
    faders.append(
        {
            "id": fid,
            "param": param,
            "orientation": "vertical",
            "origin": {"x": f["x"], "y": top},
            "travel": travel,
            "knob": f"ui/{png}",
            "knobSize": {"w": f["w"], "h": f["h"]},
            "knobHotspot": "top-left",
            "range": rng,
            "value": val,
        }
    )
    print(f"{fid:8} x={f['x']:4} top={top} travel={travel}")

skin_path = REPO / "skins/misima-hybrid/skin.json"
pub = REPO / "app/public/sprite/skin.json"
skin = json.loads(skin_path.read_text())
skin["faders"] = faders
# status must stay on the plate — put it inside playlist header area
skin["text"]["status"] = {"origin": {"x": 1040, "y": 1455}}
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub.write_text(text)
print("saved")
