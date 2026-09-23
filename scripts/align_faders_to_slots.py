import json
from pathlib import Path
import numpy as np
from PIL import Image

REPO = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp")
BG = np.asarray(Image.open(REPO / "skins/misima-hybrid/sprites/bg/bg.png").convert("RGBA"), dtype=np.float32)

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
slots.sort(key=lambda s: s["x"])
print("slots L→R:")
for s in slots:
    print(f"  x={s['x']:4} y0={s['y0']} y1={s['y1']}")

skin_path = REPO / "skins/misima-hybrid/skin.json"
pub_path = REPO / "app/public/sprite/skin.json"
skin = json.loads(skin_path.read_text())

# Order: volume pitch reverb eq1..eq10 tempo  == 14 slots L→R
order = [
    "volume", "pitch", "reverb",
    "eq1", "eq2", "eq3", "eq4", "eq5", "eq6", "eq7", "eq8", "eq9", "eq10",
    "tempo",
]
by = {f["id"]: f for f in skin["faders"]}
for i, fid in enumerate(order):
    if i >= len(slots) or fid not in by:
        continue
    f = by[fid]
    s = slots[i]
    kw, kh = f["knobSize"]["w"], f["knobSize"]["h"]
    f["origin"] = {"x": s["x"] - kw // 2, "y": s["y0"]}
    f["travel"] = max(60, s["y1"] - s["y0"] - kh)
    print(f"{fid:8} -> slot {i} x={s['x']} origin={f['origin']} travel={f['travel']}")

skin["faders"] = [by[i] for i in order if i in by]
skin["visuals"]["waterfall"] = {
    "origin": {"x": 0, "y": 0},
    "size": {"w": 0, "h": 0},
    "mode": "off",
    "color": "#3dffb5",
}
# Keep spectrum strictly in the TOP vis area (y < 700) so it cannot paint EQ
for band in skin["visuals"]["spectrum"].get("bands") or []:
    for seg in band.get("segments") or []:
        if seg["origin"]["y"] > 700:
            seg["origin"]["y"] = 585 - (seg.get("reveal", 0) * 200)
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub_path.write_text(text)
print("saved")
