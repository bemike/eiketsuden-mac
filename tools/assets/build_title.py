"""Title screen artwork `gfx/ui/title.png` (960x540, no text; the game draws the logo).

A battle diorama composed from the pack's own generated media: the terrain tileset (castle, river,
bridge, mountains, forests), unit sheets of both armies facing each other across the river and the
animated side banners. Forests and mountains frame the top edge behind the logo. It is drawn at the
480x270 virtual resolution and scaled 2x with nearest-neighbour sampling.
"""

from __future__ import annotations

import tomllib
from pathlib import Path

from PIL import Image

from assetlib import Sources, paste, save_png
from tilemap import Tileset, parse_map, render

VIRTUAL = (480, 270)
SCALE = 2

TITLE_MAP = """
^^^^^^TTTTTTT~~~TTTTTTTT^^^^^^
^^^^TTTTT,,,,~~~,,TTTTTTTT^^^^
^^TTTT,,,....~~~.....#########
TTTT,,,......~~~.....#cHHcgcc#
TT,,,....v...~~~.....#cHHcccc#
,,,,.........~~~.....#cccc$HH#
,,,..........~~~.....#ccccccc#
,,,..........~~~.....####G####
,,...........~~~........._....
..___________===__________....
..,,.........~~~..............
.,,,.........~~~.....::::.....
,,,,.........~~~.....::::::...
,,,TT........~~~......::::....
,,TTTT.......~~~...........XXX
,TTTTT.......~~~....b.....XXXX
TTTTTT.......~~~...........XXX
"""

# (class, side, column, row, facing column of the sheet: 0 down, 1 up, 2 left, 3 right)
ARMY = [
    # player army on the west bank, facing east
    ("guard_cavalry", "player", 6, 11, 3),
    ("light_cavalry", "player", 7, 10, 3),
    ("heavy_cavalry", "player", 7, 12, 3),
    ("long_infantry", "player", 10, 10, 3),
    ("long_infantry", "player", 10, 11, 3),
    ("short_infantry", "player", 9, 12, 3),
    ("short_infantry", "player", 8, 13, 3),
    ("archer", "player", 5, 13, 3),
    ("crossbow", "player", 5, 10, 3),
    ("catapult", "player", 3, 11, 3),
    ("supply", "player", 6, 14, 3),
    ("band", "player", 4, 12, 3),
    # allied troops by the village
    ("beast", "ally", 11, 5, 0),
    ("outlaw", "ally", 10, 6, 0),
    ("civilian", "ally", 8, 5, 0),
    # enemy army on the east bank, facing west
    ("guard_cavalry", "enemy", 20, 11, 2),
    ("heavy_cavalry", "enemy", 19, 10, 2),
    ("chariot", "enemy", 19, 12, 2),
    ("short_infantry", "enemy", 16, 10, 2),
    ("long_infantry", "enemy", 16, 11, 2),
    ("brigand", "enemy", 17, 12, 2),
    ("bandit", "enemy", 18, 13, 2),
    ("archer", "enemy", 22, 10, 2),
    ("sorcerer", "enemy", 22, 13, 2),
    ("tribe", "enemy", 24, 11, 2),
    ("martial", "enemy", 21, 14, 2),
    ("crossbow", "enemy", 25, 8, 0),
]

# Banners beside the commanders and on the castle: (side row in flags.png, column, row, frame).
BANNERS = [(0, 5, 10, 0), (2, 21, 10, 1), (2, 24, 7, 2), (2, 26, 7, 3), (1, 12, 4, 2)]

def compose(pack: Path) -> Image.Image:
    tileset = Tileset(pack / "gfx" / "tiles" / "terrain.toml")
    ts = tileset.size
    scene = render(tileset, parse_map(TITLE_MAP))
    with (pack / "gfx" / "units" / "units.toml").open("rb") as f:
        specs = tomllib.load(f)["sprites"]
    placed = sorted(ARMY, key=lambda u: (u[3], u[2]))  # back rows first
    for key, side, col, row, facing in placed:
        spec = specs[key]
        fw, fh = spec["frame"]
        ax, ay = spec["anchor"]
        with Image.open(pack / "gfx" / "units" / f"{key}_{side}.png") as im:
            frame = im.convert("RGBA").crop((facing * fw, 0, facing * fw + fw, fh))
        paste(scene, frame, col * ts + ts // 2 - ax, row * ts + ts - 1 - ay)
    with Image.open(pack / "gfx" / "ui" / "flags.png") as im:
        flags = im.convert("RGBA")
    for side_row, col, row, frame_i in BANNERS:
        flag = flags.crop((frame_i * 16, side_row * 16, frame_i * 16 + 16, side_row * 16 + 16))
        paste(scene, flag, col * ts + 2, row * ts - 10)
    scene = scene.crop((0, 0, *VIRTUAL))
    return scene.resize((VIRTUAL[0] * SCALE, VIRTUAL[1] * SCALE), Image.Resampling.NEAREST)


def build_title(src: Sources, pack: Path) -> list[str]:
    del src  # composed from the pack's generated media only
    save_png(compose(pack), pack / "gfx" / "ui" / "title.png")
    return ["gfx/ui/title.png"]
