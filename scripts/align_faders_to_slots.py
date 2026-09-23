import json
from pathlib import Path
import numpy as np
from PIL import Image

REPO = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp")
UI = REPO / "skins/misima-hybrid/sprites/ui"
BG = np.asarray(Image.open(REPO / "skins/misima-hybrid/sprites/bg/bg.png").convert("RGBA"), dtype=np.float32)

# Fixed L→R columns from art (UI_elements X), min 36px apart
# volume pitch reverb | eq1..eq10 | tempo
COLUMNS = [
    516, 572, 628,
    664, 718, 754, 800, 856, 904, 956, 1002, 1042, 1104, 1164,
]
# eq4 is 4th EQ = index 3 of eq group = COLUMNS[6] = 800... user wants eq4 NOT after eq10.
# COLUMNS[3..12] are eq1..eq10:
# eq1=664 eq2=718 eq3=754 eq4=800 eq5=856 eq6=904 eq7=956 eq8=1002 eq9=1042 eq10=1104
# Wait, we need 10 eq + 3 + 1 = 14. Fix list:
COLUMNS = [
    516,  # volume
    572,  # pitch
    628,  # reverb
    664,  # eq1
    718,  # eq2
    754,  # eq3
    800,  # eq4  ← 4th EQ, left of eq5..eq10
    856,  # eq5
    904,  # eq6
    956,  # eq7
    1002, # eq8
    1042, # eq9
    1104, # eq10
    1164, # tempo
]

META = [
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

# Track tops vary per slot — measure from bg columns
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
        cands.append({"x": x, "y0": best_y[0], "y1": best_y[1]})
cands.sort(key=lambda t: t["x"])
slots = []
for t in cands:
    if slots and abs(t["x"] - slots[-1]["x"]) < 22:
        p = slots[-1]
        p["x"] = (p["x"] + t["x"]) // 2
        p["y0"] = min(p["y0"], t["y0"])
        p["y1"] = max(p["y1"], t["y1"])
    else:
        slots.append(dict(t))
slots.sort(key=lambda s: s["x"])

faders = []
for i, (fid, param, png, rng, val) in enumerate(META):
    im = Image.open(UI / png)
    w, h = im.size
    x = COLUMNS[i]
    # nearest track for top/bottom (Y only)
    slot = min(slots, key=lambda s: abs(s["x"] - x))
    top = slot["y0"]
    travel = max(80, slot["y1"] - slot["y0"] - h)
    faders.append({
        "id": fid,
        "param": param,
        "orientation": "vertical",
        "origin": {"x": x, "y": top},
        "travel": travel,
        "knob": f"ui/{png}",
        "knobSize": {"w": w, "h": h},
        "knobHotspot": "top-left",
        "range": rng,
        "value": val,
    })
    print(f"{fid:8} origin=({x},{top}) travel={travel} size={w}x{h}")

assert faders[6]["id"] == "eq4" and faders[6]["origin"]["x"] < faders[12]["origin"]["x"]

skin_path = REPO / "skins/misima-hybrid/skin.json"
pub = REPO / "app/public/sprite/skin.json"
skin = json.loads(skin_path.read_text())
skin["faders"] = faders
skin["text"]["status"] = {"origin": {"x": 1040, "y": 1455}}
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub.write_text(text)
print("eq4 x", faders[6]["origin"]["x"], "< eq10 x", faders[12]["origin"]["x"])
print("saved")
