import json
from pathlib import Path

p = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp\app\public\sprite\skin.json")
s = json.loads(p.read_text())
print("=== faders (artist) ===")
for f in s["faders"]:
    print(f"{f['id']:8} origin={f['origin']} travel={f['travel']} value={f['value']} range={f['range']}")
print("=== buttons ===")
for b in s["buttons"]:
    print(f"{b['id']:16} origin={b['origin']} action={b.get('action')}")
