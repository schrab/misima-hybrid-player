import json
from pathlib import Path
from PIL import Image

REPO = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp")
UI = REPO / "skins/misima-hybrid/sprites/ui"

# Rest poses from UI_elements.png — knob sits here at DEFAULT value.
# maxY (origin.y) = restY - (1 - default_norm) * travel  → all differ.
ART = [
    ("volume", "volume", "knob_volume.png", [0, 1], 0.8, 515, 834, 200),
    ("pitch", "pitch", "knob_pitch.png", [-12, 12], 0, 571, 833, 220),
    ("reverb", "reverb", "knob_reverb.png", [0, 1], 0.15, 628, 833, 220),
    ("eq1", "eq0", "knob_eq_1.png", [-12, 12], 0, 664, 832, 240),
    ("eq2", "eq1", "knob_eq_2.png", [-12, 12], 0, 719, 833, 240),
    ("eq3", "eq2", "knob_eq_3.png", [-12, 12], 0, 755, 834, 240),
    ("eq4", "eq3", "knob_eq_4.png", [-12, 12], 0, 856, 835, 240),
    ("eq5", "eq4", "knob_eq_5.png", [-12, 12], 0, 905, 839, 250),
    ("eq6", "eq5", "knob_eq_6.png", [-12, 12], 0, 956, 848, 250),
    ("eq7", "eq6", "knob_eq_7.png", [-12, 12], 0, 1002, 842, 250),
    ("eq8", "eq7", "knob_eq_8.png", [-12, 12], 0, 1042, 840, 250),
    ("eq9", "eq8", "knob_eq_9.png", [-12, 12], 0, 1103, 910, 220),
    ("eq10", "eq9", "knob_eq_10.png", [-12, 12], 0, 1111, 912, 220),
    ("tempo", "speed", "knob_tempo.png", [0.5, 2], 1.0, 1164, 952, 180),
]

faders = []
max_ys = []
used = set()
for i, (fid, param, png, rng, default, rx, ry, travel0) in enumerate(ART):
    w, h = Image.open(UI / png).size
    lo, hi = rng
    n = (default - lo) / (hi - lo) if hi != lo else 1.0
    # Unique maxY per fader (user requirement). Keep rest pose exact.
    max_y = ry - (1.0 - n) * travel0
    while round(max_y) in used:
        max_y -= 3.0
    my = round(max_y)
    used.add(my)
    # travel so default value lands exactly on restY
    travel = round((ry - max_y) / (1.0 - n)) if n < 0.999 else travel0
    travel = max(80, travel)
    faders.append({
        "id": fid,
        "param": param,
        "orientation": "vertical",
        "origin": {"x": rx, "y": my},
        "travel": travel,
        "knob": f"ui/{png}",
        "knobSize": {"w": w, "h": h},
        "knobHotspot": "top-left",
        "range": rng,
        "value": default,
    })
    max_ys.append(my)
    print(f"{fid:8} rest=({rx},{ry}) maxY={my} travel={travel}")

assert len(set(max_ys)) == 14, max_ys

skin_path = REPO / "skins/misima-hybrid/skin.json"
pub = REPO / "app/public/sprite/skin.json"
skin = json.loads(skin_path.read_text())
skin["faders"] = faders
for b in skin.get("buttons") or []:
    if b.get("id") == "fx_enable":
        b["action"] = "fx_enable"
    if b.get("id") == "fx_reset":
        b["action"] = "fx_reset"
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub.write_text(text)
print("saved")
