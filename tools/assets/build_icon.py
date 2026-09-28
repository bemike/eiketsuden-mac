"""Build the application icon of the game executable.

The icon is the web page's favicon (web/index.html): a red banner on a flagpole inside a gold
frame on a blue field. It is drawn here as pixel art at 16 px (the favicon, pixel for pixel) and
32 px (the same motif with a little more detail); larger sizes are integer enlargements, so the
pixels stay crisp. Everything is drawn from rectangles in this script: no third-party source.

Outputs (committed, in crates/hero-game/icon/):

* ``eiketsuden.ico`` — 16, 32, 48, 64, 128 and 256 px (PNG-compressed entries), embedded into the
  Windows executable by ``crates/hero-game/build.rs`` (Explorer, shortcuts, the taskbar);
* ``icon_16.rgba``, ``icon_32.rgba``, ``icon_64.rgba`` — raw RGBA pixels the game gives the
  window at start-up (the window and taskbar icon);
* ``icon_256.png`` — for documentation and packaging.

Run ``python tools/assets/build_icon.py`` to rebuild, ``--check`` to compare with the committed
files. The pixels are deterministic; the PNG bytes are not (zlib builds differ between Pillow
wheels), so ``--check`` compares the decoded pixels and the ICO directory.
"""

from __future__ import annotations

import io
import struct
import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "crates" / "hero-game" / "icon"

Color = tuple[int, int, int, int]
Rect = tuple[int, int, int, int]  # x, y, width, height

FIELD: Color = (0x1C, 0x2F, 0x78, 255)
FIELD_DARK: Color = (0x14, 0x22, 0x5A, 255)
GOLD: Color = (0xE8, 0xD9, 0xA8, 255)
GOLD_DARK: Color = (0xB8, 0xA0, 0x60, 255)
RED: Color = (0xC8, 0x40, 0x2F, 255)
RED_DARK: Color = (0x96, 0x2C, 0x22, 255)


def fill(img: Image.Image, rect: Rect, color: Color) -> None:
    x, y, w, h = rect
    for yy in range(y, y + h):
        for xx in range(x, x + w):
            img.putpixel((xx, yy), color)


def frame(img: Image.Image, rect: Rect, color: Color) -> None:
    x, y, w, h = rect
    fill(img, (x, y, w, 1), color)
    fill(img, (x, y + h - 1, w, 1), color)
    fill(img, (x, y, 1, h), color)
    fill(img, (x + w - 1, y, 1, h), color)


def icon16() -> Image.Image:
    """The favicon, pixel for pixel (its SVG strokes at x.5 cover one pixel)."""
    img = Image.new("RGBA", (16, 16), FIELD)
    frame(img, (1, 1, 14, 14), GOLD)
    fill(img, (5, 3, 1, 10), GOLD)
    fill(img, (6, 4, 6, 5), RED)
    return img


def icon32() -> Image.Image:
    """The same motif at twice the size, with a shaded rim, a finial, a hem and an emblem."""
    img = Image.new("RGBA", (32, 32), FIELD)
    frame(img, (0, 0, 32, 32), FIELD_DARK)
    frame(img, (2, 2, 28, 28), GOLD)
    frame(img, (3, 3, 26, 26), GOLD_DARK)
    # Flagpole with a finial.
    fill(img, (10, 7, 2, 19), GOLD)
    fill(img, (11, 7, 1, 19), GOLD_DARK)
    fill(img, (9, 5, 4, 2), GOLD)
    # Banner: a red field with a darker hem and a swallow-tail notch.
    fill(img, (12, 8, 12, 10), RED)
    fill(img, (12, 16, 12, 2), RED_DARK)
    fill(img, (23, 9, 1, 8), FIELD)
    fill(img, (22, 10, 1, 6), FIELD)
    fill(img, (21, 11, 1, 4), FIELD)
    fill(img, (20, 12, 1, 2), FIELD)
    # A gold emblem on the banner.
    fill(img, (15, 10, 4, 4), GOLD)
    fill(img, (16, 11, 2, 2), RED_DARK)
    return img


def enlarge(img: Image.Image, factor: int) -> Image.Image:
    return img.resize((img.width * factor, img.height * factor), Image.Resampling.NEAREST)


def png_bytes(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    img.save(buf, format="PNG", compress_level=9)
    return buf.getvalue()


def ico_bytes(images: list[Image.Image]) -> bytes:
    """An ICO file with one PNG-compressed entry per image (Windows Vista and later)."""
    entries = [png_bytes(img) for img in images]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    directory = b""
    for img, data in zip(images, entries, strict=True):
        size = img.width if img.width < 256 else 0  # 0 means 256
        directory += struct.pack("<BBBBHHII", size, size, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return header + directory + b"".join(entries)


def outputs() -> dict[str, bytes]:
    small, medium = icon16(), icon32()
    sizes = [
        small,
        medium,
        enlarge(small, 3),
        enlarge(medium, 2),
        enlarge(medium, 4),
        enlarge(medium, 8),
    ]
    return {
        "eiketsuden.ico": ico_bytes(sizes),
        "icon_16.rgba": small.tobytes(),
        "icon_32.rgba": medium.tobytes(),
        "icon_64.rgba": enlarge(medium, 2).tobytes(),
        "icon_256.png": png_bytes(enlarge(medium, 8)),
    }


def pixels(data: bytes) -> tuple[tuple[int, int], bytes]:
    """Size and RGBA pixels of a PNG."""
    img = Image.open(io.BytesIO(data)).convert("RGBA")
    return img.size, img.tobytes()


def ico_content(data: bytes) -> list[tuple[bytes, tuple[tuple[int, int], bytes]]]:
    """The directory fields (without sizes and offsets) and the pixels of every ICO entry."""
    _, _, count = struct.unpack_from("<HHH", data)
    out = []
    for i in range(count):
        entry = data[6 + 16 * i : 6 + 16 * (i + 1)]
        length, offset = struct.unpack_from("<II", entry, 8)
        out.append((entry[:8], pixels(data[offset : offset + length])))
    return out


def same(name: str, committed: bytes, built: bytes) -> bool:
    """Whether a committed output matches a fresh build: the pixels for images."""
    try:
        if name.endswith(".ico"):
            return ico_content(committed) == ico_content(built)
        if name.endswith(".png"):
            return pixels(committed) == pixels(built)
    except (OSError, struct.error, ValueError):
        return False
    return committed == built


def main(argv: list[str]) -> int:
    out = Path(argv[argv.index("--out") + 1]) if "--out" in argv else OUT
    check = "--check" in argv
    files = outputs()
    stale = []
    for name, data in files.items():
        path = out / name
        if check:
            if not path.is_file() or not same(name, path.read_bytes(), data):
                stale.append(name)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print(f"out of date: {', '.join(stale)} (run python tools/assets/build_icon.py)")
        return 1
    print("icon up to date" if check else f"wrote {len(files)} files to {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
