import json
from pathlib import Path

skin_path = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp\skins\misima-hybrid\skin.json")
pub_path = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp\app\public\sprite\skin.json")
skin = json.loads(skin_path.read_text())
# Track bottoms on the artboard sit around y≈1180 (EQ plate).
TRACK_BOTTOM = 1180
for f in skin["faders"]:
    y = f["origin"]["y"]
    h = f["knobSize"]["h"]
    f["travel"] = max(80, TRACK_BOTTOM - y - h)
    print(f"{f['id']:8} ({f['origin']['x']},{y}) travel={f['travel']}")
text = json.dumps(skin, indent=2)
skin_path.write_text(text)
pub_path.write_text(text)
print("ok")
