import json
from pathlib import Path
import numpy as np
from PIL import Image

REPO = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp")
UI = REPO / "skins/misima-hybrid/sprites/ui"
BG = np.asarray(Image.open(REPO / "skins/misima-hybrid/sprites/bg/bg.png").convert("RGBA"), dtype=np.float32)

# 14 unique tracks L→R from bg slots (each fader gets one)
SLOTS = [
    {"x": 588, "y0": 930, "y1": 1074},
    {"x": 636, "y0": 890, "y1": 1092},
    {"x": 685, "y0": 904, "y1": 1116},
    {"x": 733, "y0": 868, "y1": 1078},
    {"x": 789, "y0": 1018, "y1": 1158},
    {"x": 835, "y0": 990, "y1": 1138},
    {"x": 874, "y0": 850, "y1": 1073},
    {"x": 923, "y0": 855, "y1": 1199},
    {"x": 972, "y0": 856, "y1": 1192},
    {"x": 1020, "y0": 942, "y1": 1190},
    {"x": 1070, "y0": 854, "y1": 1201},
    {"x": 1128, "y0": 922, "y1": 1192},
    {"x": 1221, "y0": 965, "y1": 1186},
    {"x": 1276, "y0": 936, "y1": 1114},
]

# id, param, png, range, DEFAULT position intent
# value = max | mid | min | number
META = [
    ("volume", "volume", "knob_volume.png", [0, 1], "max"),
    ("pitch", "pitch", "knob_pitch.png", [-12, 12], "mid"),
    ("reverb", "reverb", "knob_reverb.png", [0, 1], "min"),
    ("eq1", "eq0", "knob_eq_1.png", [-12, 12], "mid"),
    ("eq2", "eq1", "knob_eq_2.png", [-12, 12], "mid"),
    ("eq3", "eq2", "knob_eq_3.png", [-12, 12], "mid"),
    ("eq4", "eq3", "knob_eq_4.png", [-12, 12], "mid"),
    ("eq5", "eq4", "knob_eq_5.png", [-12, 12], "mid"),
    ("eq6", "eq5", "knob_eq_6.png", [-12, 12], "mid"),
    ("eq7", "eq6", "knob_eq_7.png", [-12, 12], "mid"),
    ("eq8", "eq7", "knob_eq_8.png", [-12, 12], "mid"),
    ("eq9", "eq8", "knob_eq_9.png", [-12, 12], "mid"),
    ("eq10", "eq9", "knob_eq_10.png", [-12, 12], "mid"),
    ("tempo", "speed", "knob_tempo.png", [0.5, 2], "mid"),
]


def default_value(rng, intent):
    lo, hi = rng
    if intent == "max":
        return hi
    if intent == "min":
        return lo
    if rng == [0.5, 2]:
        return 1.0  # tempo: musical centre (1×)
    return (lo + hi) / 2.0


faders = []
max_ys = []
for i, (fid, param, png, rng, intent) in enumerate(META):
    w, h = Image.open(UI / png).size
    s = SLOTS[i]  # unique L→R — eq4 is 7th track (x=874), eq10 is 13th (x=1221)
    top = s["y0"]          # MAX value y
    travel = max(80, s["y1"] - s["y0"] - h)  # down to MIN
    val = default_value(rng, intent)
    max_ys.append(top)
    faders.append({
        "id": fid,
        "param": param,
        "orientation": "vertical",
        "origin": {"x": s["x"] - w // 2, "y": top},
        "travel": travel,
        "knob": f"ui/{png}",
        "knobSize": {"w": w, "h": h},
        "knobHotspot": "top-left",
        "range": rng,
        "value": val,
    })
    print(f"{fid:8} x={s['x']:4} maxY={top:4} travel={travel:3} default={intent} ({val})")

assert len(set(max_ys)) == 14, max_ys
assert faders[6]["id"] == "eq4" and faders[6]["origin"]["x"] < faders[12]["origin"]["x"]

skin_path = REPO / "skins/misima-hybrid/skin.json"
pub = REPO / "app/public/sprite/skin.json"
skin = json.loads(skin_path.read_text())
skin["faders"] = faders
for b in skin.get("buttons") or []:
    if b.get("id") == "fx_enable":
        b["action"] = "fx_enable"
    if b.get("id") == "fx_reset":
        b["action"] = "fx_reset"
    if b.get("id") == "power":
        b["action"] = "power"
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub.write_text(text)
print("eq4 x", faders[6]["origin"]["x"], "eq10 x", faders[12]["origin"]["x"])
print("saved")
