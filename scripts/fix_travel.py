import json
from pathlib import Path
from PIL import Image
import numpy as np

layer = np.asarray(Image.open(r"C:\Users\schra\Developer\misima-hybrid-winamp\gfx\UI_elements.png").convert("RGBA"), dtype=np.int16)
al = layer[:, :, 3]
skin_path = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp\skins\misima-hybrid\skin.json")
pub_path = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp\app\public\sprite\skin.json")
skin = json.loads(skin_path.read_text())
for f in skin["faders"]:
    x, y = f["origin"]["x"], f["origin"]["y"]
    w, h = f["knobSize"]["w"], f["knobSize"]["h"]
    cx = x + w // 2
    last = y
    for yy in range(y + h, min(2060, y + h + 520)):
        window = al[yy, max(0, cx - 8) : cx + 9]
        if window.max() > 30:
            last = yy
    track_len = max(48, last - y)
    travel = max(48, track_len - h)
    print(f"{f['id']:8} origin=({x},{y}) track_bottom~{last} travel={travel}")
    f["travel"] = travel
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub_path.write_text(text)
print("travel updated")
