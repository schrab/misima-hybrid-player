import json
from pathlib import Path

REPO = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp")
SKIN = REPO / "skins/misima-hybrid/skin.json"
PUB = REPO / "app/public/sprite/skin.json"

# EXACT X from UI_elements.png (match score 0) — never use bg slot X (those shift right)
# maxY = track top (unique), travel = stroke, value = your default poses
FADERS = [
    # id, param, png, range, value, x, maxY, travel
    ("volume", "volume", "knob_volume.png", [0, 1], 1.0, 515, 930, 110),
    ("pitch", "pitch", "knob_pitch.png", [-12, 12], 0.0, 571, 890, 164),
    ("reverb", "reverb", "knob_reverb.png", [0, 1], 0.0, 628, 904, 164),
    ("eq1", "eq0", "knob_eq_1.png", [-12, 12], 0.0, 664, 868, 177),
    ("eq2", "eq1", "knob_eq_2.png", [-12, 12], 0.0, 719, 1018, 89),
    ("eq3", "eq2", "knob_eq_3.png", [-12, 12], 0.0, 755, 990, 108),
    ("eq4", "eq3", "knob_eq_4.png", [-12, 12], 0.0, 856, 850, 196),
    ("eq5", "eq4", "knob_eq_5.png", [-12, 12], 0.0, 905, 855, 290),
    ("eq6", "eq5", "knob_eq_6.png", [-12, 12], 0.0, 956, 856, 280),
    ("eq7", "eq6", "knob_eq_7.png", [-12, 12], 0.0, 1002, 942, 219),
    ("eq8", "eq7", "knob_eq_8.png", [-12, 12], 0.0, 1042, 854, 292),
    ("eq9", "eq8", "knob_eq_9.png", [-12, 12], 0.0, 1103, 922, 236),
    ("eq10", "eq9", "knob_eq_10.png", [-12, 12], 0.0, 1111, 965, 190),
    ("tempo", "speed", "knob_tempo.png", [0.5, 2], 1.0, 1164, 936, 140),
]

from PIL import Image

UI = REPO / "skins/misima-hybrid/sprites/ui"
faders = []
for fid, param, png, rng, val, x, maxY, travel in FADERS:
    w, h = Image.open(UI / png).size
    faders.append({
        "id": fid,
        "param": param,
        "orientation": "vertical",
        "origin": {"x": x, "y": maxY},
        "travel": travel,
        "knob": f"ui/{png}",
        "knobSize": {"w": w, "h": h},
        "knobHotspot": "top-left",
        "range": rng,
        "value": val,
    })

skin = json.loads(SKIN.read_text())
skin["faders"] = faders
for b in skin.get("buttons") or []:
    if b.get("id") == "fx_enable":
        b["action"] = "fx_enable"
    if b.get("id") == "fx_reset":
        b["action"] = "fx_reset"
    if b.get("id") == "power":
        b["action"] = "power"
text = json.dumps(skin, indent=2)
SKIN.write_text(text)
PUB.write_text(text)
print("X set from UI_elements (NOT slots):")
for f in faders:
    print(f"  {f['id']:8} x={f['origin']['x']} maxY={f['origin']['y']} travel={f['travel']} val={f['value']}")
