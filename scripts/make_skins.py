from pathlib import Path
from PIL import Image, ImageDraw
import zipfile
import json

root = Path(__file__).resolve().parents[1]


def plate(w: int, h: int, out: Path, accent=(61, 255, 181)) -> None:
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.rounded_rectangle([4, 4, w - 5, h - 5], radius=28, fill=(18, 36, 40, 235), outline=(*accent, 180), width=2)
    d.ellipse([w // 3, h // 4, w // 3 + 40, h // 4 + 40], outline=(94, 200, 255, 160), width=2)
    img.save(out)


def main() -> None:
    native = root / "skins" / "misima-hybrid"
    (native / "assets").mkdir(parents=True, exist_ok=True)
    plate(920, 280, native / "assets" / "panel_main.png")
    plate(920, 160, native / "assets" / "panel_eq.png", accent=(255, 79, 216))
    plate(520, 320, native / "assets" / "panel_playlist.png", accent=(179, 136, 255))

    png = root / "skins" / "winamp5-misima" / "png"
    png.mkdir(parents=True, exist_ok=True)
    plate(600, 420, png / "player_bg.png")
    plate(600, 240, png / "eq_bg.png", accent=(255, 79, 216))
    plate(480, 520, png / "playlist_bg.png", accent=(179, 136, 255))
    for name in ["prev", "play", "pause", "stop", "next"]:
        b = Image.new("RGBA", (28, 28), (0, 0, 0, 0))
        d = ImageDraw.Draw(b)
        d.rounded_rectangle([1, 1, 26, 26], radius=6, fill=(61, 255, 181, 200))
        b.save(png / f"cbutton_{name}.png")
    Image.new("RGBA", (12, 12), (255, 79, 216, 230)).save(png / "volume_thumb.png")
    Image.new("RGBA", (120, 12), (20, 40, 44, 200)).save(png / "volume_bar.png")

    # pack native skin
    mskin = root / "skins" / "misima-hybrid.mskin"
    with zipfile.ZipFile(mskin, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in native.rglob("*"):
            if p.is_file():
                zf.write(p, p.relative_to(native).as_posix())

    # pack winamp modern skin as .wal
    wal_src = root / "skins" / "winamp5-misima"
    wal = root / "skins" / "winamp5-misima.wal"
    with zipfile.ZipFile(wal, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        for p in wal_src.rglob("*"):
            if p.is_file() and p.name != "README.md":
                zf.write(p, p.relative_to(wal_src).as_posix())

    # validate native skin.json parses
    manifest = json.loads((native / "skin.json").read_text())
    assert manifest["formatVersion"] == 1
    print("packed", mskin, wal)


if __name__ == "__main__":
    main()
