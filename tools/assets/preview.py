#!/usr/bin/env python3
"""Render preview sheets of the built pack for visual review (not part of the pack).

Writes into tools/assets/.cache/preview/ (or --out):
  map.png     a sample battle map drawn with gfx/tiles/terrain.toml, units of every class and
              side standing on it, FX and flags, at 1x and 2x
  tiles.png   every tile key: its variants and all 16 autotile masks
  units.png   every unit sheet (all frames) of every side

Usage:
    python tools/assets/preview.py [--pack data/base] [--out DIR]
"""

from __future__ import annotations

import argparse
import sys
import tomllib
from pathlib import Path

from PIL import Image, ImageDraw

from assetlib import CACHE_DIR, PACK_DIR, paste
from tilemap import Tileset, parse_map, render

SIDES = ("player", "ally", "enemy")

SAMPLE_MAP = """
^^^^^TTTT,,,,.....~~........::::::XXXX
^^^^TTTT,,,,,.....~~.......:::f:::XXXX
^^^TTT,,,,.....v..~~......::::::::::XX
^^TTT,,,,.._______=_______.......::::X
^TTT,,,...._.....~~......_.........::X
TTT,,,,...._..v..~~......_...b.......:
TT,,,,....._.....~~......_...........:
T,,,,......_....~~~......_..||||||....
,,,,.......____~~~.......______.......
,,,.......#####G####......_...,,,,....
,,.......##ccccccccc##....._..,,TTT...
,........#cHHcccccc$c#....._.,,TTTTT..
.........#cHHcccgcccc#.....v..,,TTT,,.
.........#ccccccccHHc#.........,,,,,..
.........######G######.........XX.....
...............______.........XXX.....
"""


def load_units(pack: Path) -> tuple[dict, dict[tuple[str, str], Image.Image]]:
    with (pack / "gfx" / "units" / "units.toml").open("rb") as f:
        doc = tomllib.load(f)
    sheets = {}
    for key in doc["sprites"]:
        for side in SIDES:
            with Image.open(pack / "gfx" / "units" / f"{key}_{side}.png") as im:
                sheets[key, side] = im.convert("RGBA")
    return doc["sprites"], sheets


def frame(sheet: Image.Image, spec: dict, col: int, row: int) -> Image.Image:
    fw, fh = spec["frame"]
    return sheet.crop((col * fw, row * fh, col * fw + fw, row * fh + fh))


def draw_unit(canvas: Image.Image, spec: dict, img: Image.Image, tx: int, ty: int, ts: int) -> None:
    ax, ay = spec["anchor"]
    paste(canvas, img, tx * ts + ts // 2 - ax, ty * ts + ts - 1 - ay)


def map_preview(pack: Path, out: Path) -> None:
    tileset = Tileset(pack / "gfx" / "tiles" / "terrain.toml")
    grid = parse_map(SAMPLE_MAP)
    base = render(tileset, grid)
    ts = tileset.size
    units_path = pack / "gfx" / "units" / "units.toml"
    if units_path.exists():
        specs, sheets = load_units(pack)
        keys = list(specs)
        # passable spots for a parade of every class: player row, ally row, enemy row
        spots = [(x, y) for y in range(len(grid)) for x in range(len(grid[0])) if grid[y][x] in ("plain", "grass")]
        spots.sort(key=lambda p: (p[1], p[0]))
        chosen: list[tuple[int, int]] = []
        for p in spots:
            if all(abs(p[0] - q[0]) + abs(p[1] - q[1]) >= 2 for q in chosen):
                chosen.append(p)
        i = 0
        for side_i, side in enumerate(SIDES):
            for j, key in enumerate(keys):
                if i >= len(chosen):
                    break
                x, y = chosen[i]
                i += 1
                facing = (j + side_i) % 4
                row = 4 if j % 5 == 0 else 5 if j % 7 == 3 else 0
                draw_unit(base, specs[key], frame(sheets[key, side], specs[key], facing, row), x, y, ts)
    flags = pack / "gfx" / "ui" / "flags.png"
    if flags.exists():
        with Image.open(flags) as im:
            fl = im.convert("RGBA")
        for r in range(3):
            paste(base, fl.crop((16 * r, 16 * r, 16 * r + 16, 16 * r + 16)), 16 * (2 + r), 16 * 14)
    base.save(out / "map_1x.png")
    base.resize((base.width * 2, base.height * 2), Image.Resampling.NEAREST).save(out / "map_2x.png")


def tiles_preview(pack: Path, out: Path) -> None:
    tileset = Tileset(pack / "gfx" / "tiles" / "terrain.toml")
    ts = tileset.size
    rows = []
    for key, layers in tileset.tiles.items():
        strip = []
        for layer in layers:
            if layer.auto is not None:
                strip.extend(layer.auto)
            elif layer.cells is not None:
                strip.extend(layer.cells)
        rows.append((key, strip))
    width = 90 + max(len(s) for _, s in rows) * (ts + 1)
    sheet = Image.new("RGBA", (width, len(rows) * (ts + 3)), (255, 0, 255, 255))
    d = ImageDraw.Draw(sheet)
    for i, (key, strip) in enumerate(rows):
        d.text((2, i * (ts + 3) + 3), key, fill=(0, 0, 0, 255))
        for j, pos in enumerate(strip):
            paste(sheet, tileset.cell(pos), 90 + j * (ts + 1), i * (ts + 3))
    sheet.resize((sheet.width * 3, sheet.height * 3), Image.Resampling.NEAREST).save(out / "tiles.png")


def units_preview(pack: Path, out: Path) -> None:
    units_path = pack / "gfx" / "units" / "units.toml"
    if not units_path.exists():
        return
    specs, sheets = load_units(pack)
    blocks = [(key, [sheets[key, side] for side in SIDES]) for key in specs]
    bw = max(s.width for _, ss in blocks for s in ss)
    bh = max(s.height for _, ss in blocks for s in ss)
    cols = 6
    per = 3
    cw = per * (bw + 4) + 6
    ch = bh + 14
    rows = (len(blocks) + cols - 1) // cols
    sheet = Image.new("RGBA", (cols * cw, rows * ch), (136, 170, 96, 255))
    d = ImageDraw.Draw(sheet)
    for i, (key, ss) in enumerate(blocks):
        x0, y0 = (i % cols) * cw, (i // cols) * ch
        d.text((x0 + 2, y0), key, fill=(0, 0, 0, 255))
        for j, s in enumerate(ss):
            sheet.alpha_composite(s, (x0 + j * (bw + 4), y0 + 12))
    sheet.resize((sheet.width * 2, sheet.height * 2), Image.Resampling.NEAREST).save(out / "units.png")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pack", type=Path, default=PACK_DIR)
    parser.add_argument("--out", type=Path, default=CACHE_DIR / "preview")
    args = parser.parse_args(argv)
    args.out.mkdir(parents=True, exist_ok=True)
    tiles_preview(args.pack, args.out)
    map_preview(args.pack, args.out)
    units_preview(args.pack, args.out)
    print(f"previews written to {args.out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
