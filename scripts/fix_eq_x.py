import json
from pathlib import Path

p = Path(r"C:\Users\schra\Developer\misima-hybrid-winamp\.worktrees\skinnable-player-mvp\app\public\sprite\skin.json")
s = json.loads(p.read_text())
xs = {
    "volume": 515, "pitch": 571, "reverb": 628,
    "eq1": 664, "eq2": 714, "eq3": 764,
    "eq4": 814, "eq5": 864, "eq6": 914,
    "eq7": 964, "eq8": 1014, "eq9": 1064,
    "eq10": 1114, "tempo": 1164,
}
for f in s["faders"]:
    if f["id"] in xs:
        f["origin"]["x"] = xs[f["id"]]
p.write_text(json.dumps(s, indent=2))
for f in s["faders"]:
    print(f"{f['id']:8} x={f['origin']['x']}")
