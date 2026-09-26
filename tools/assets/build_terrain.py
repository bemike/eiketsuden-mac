"""Battle map terrain: `gfx/tiles/terrain.png` (16x16 atlas) + `gfx/tiles/terrain.toml`.

Every canonical terrain id of docs/ASSETS.md gets a tile key of the same name. Ground types are
4-bit autotiles cut from the Ninja Adventure (NA) floor/water/relief tilesets; objects (trees,
peaks, buildings, walls) are layered on top. Trees and peaks come from Toen's pack with their
colours mapped onto the NA palette; buildings are drawn for this project (art.py) and walls,
fences, bridges and paving are generated here in the same palette.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field
from pathlib import Path

from PIL import Image

import art
from assetlib import (
    Atlas,
    Sources,
    cell,
    flip_h,
    hash2,
    inner_outline,
    new,
    paste,
    recolor,
    rgba,
    save_png,
    toml_value,
    write_text,
)

T = 16
N, E, S, W = 1, 2, 4, 8

NA_FLOOR = "Backgrounds/Tilesets/TilesetFloor.png"
NA_WATER = "Backgrounds/Tilesets/TilesetWater.png"
NA_DETAIL = "Backgrounds/Tilesets/TilesetFloorDetail.png"
NA_RELIEF = "Backgrounds/Tilesets/TilesetRelief.png"
TOEN_SHEET = "Tile-set - Toen's Medieval Strategy (16x16) - v.1.0.png"

# Position (dx, dy) inside NA's 4x4 "island" blocks for each neighbour mask: a 3x3 island
# (corners, edges, centre), a vertical strip in the fourth column and a horizontal strip plus a
# single blob in the fourth row.
ISLAND: dict[int, tuple[int, int]] = {
    0: (3, 3),
    N: (3, 2),
    S: (3, 0),
    N | S: (3, 1),
    E: (0, 3),
    W: (2, 3),
    E | W: (1, 3),
    E | S: (0, 0),
    S | W: (2, 0),
    N | E: (0, 2),
    N | W: (2, 2),
    N | E | S: (0, 1),
    N | S | W: (2, 1),
    E | S | W: (1, 0),
    N | E | W: (1, 2),
    N | E | S | W: (1, 1),
}

# Terrain ids of docs/ASSETS.md; the tile key of each is its id.
TERRAIN_IDS = [
    "plain",
    "grass",
    "road",
    "forest",
    "mountain",
    "wasteland",
    "bridge",
    "castle",
    "gate",
    "village",
    "barracks",
    "fort",
    "granary",
    "treasury",
    "river",
    "wall",
    "cliff",
    "house",
    "fence",
]

# Which neighbours an autotile layer joins with.
GREEN = ["grass", "forest", "mountain"]
DIRT = ["road", "bridge", "gate", "village", "barracks", "fort"]
YARD = ["village", "barracks", "fort", "road"]
WATER = ["river", "bridge"]
WALLS = ["wall", "gate"]

# ---------------------------------------------------------------------------------------------
# Tile description


@dataclass
class Layer:
    """One layer of a tile key: `cells` variants (weights by repetition) or a 16-cell `auto`."""

    cells: list[Image.Image] = field(default_factory=list)
    auto: dict[int, Image.Image] | None = None
    connect: Sequence[str] = ()


@dataclass
class Tile:
    key: str
    layers: list[Layer]
    comment: str


def island(sheet: Image.Image, col: int, row: int) -> dict[int, Image.Image]:
    """The 16 mask cells of an NA island block whose top-left cell is (col, row)."""
    return {m: cell(sheet, col + dx, row + dy) for m, (dx, dy) in ISLAND.items()}


def quad_auto(block: Sequence[Sequence[Image.Image]]) -> dict[int, Image.Image]:
    """The 16 mask cells assembled from 8x8 quadrants of a 3x3 blob (rows of corner/edge/centre).

    Each quadrant only depends on its two orthogonal neighbours: e.g. the north-west quadrant
    comes from the centre if north and west are connected, from the west edge if only north is,
    from the north edge if only west is, and from the north-west corner otherwise.
    """
    h = T // 2
    out: dict[int, Image.Image] = {}
    for m in range(16):
        img = new(T, T)
        for qx in (0, 1):
            for qy in (0, 1):
                col = 1 if m & (W if qx == 0 else E) else (0 if qx == 0 else 2)
                row = 1 if m & (N if qy == 0 else S) else (0 if qy == 0 else 2)
                img.paste(block[row][col].crop((qx * h, qy * h, qx * h + h, qy * h + h)), (qx * h, qy * h))
        out[m] = img
    return out


def swap(img: Image.Image, table: dict[str, str]) -> Image.Image:
    return recolor(img, {rgba(k): rgba(v) for k, v in table.items()}, strict=False)


def over(*imgs: Image.Image) -> Image.Image:
    """Alpha-composite 16x16 images bottom to top."""
    out = new(T, T)
    for im in imgs:
        out.alpha_composite(im)
    return out


def place(*parts: tuple[Image.Image, int, int]) -> Image.Image:
    """A 16x16 cell with each (image, x, y) pasted in order (clipped)."""
    out = new(T, T)
    for img, x, y in parts:
        paste(out, img, x, y)
    return out


def canvas(fill: str) -> Image.Image:
    return Image.new("RGBA", (T, T), rgba(fill))


def put(img: Image.Image, x: int, y: int, color: str) -> None:
    if 0 <= x < img.width and 0 <= y < img.height:
        img.putpixel((x, y), rgba(color))


# ---------------------------------------------------------------------------------------------
# Sources


class Kit:
    """Source images shared by the tile builders (loaded once)."""

    def __init__(self, src: Sources) -> None:
        self.floor = src.na(NA_FLOOR)
        self.water = src.na(NA_WATER)
        self.detail = src.na(NA_DETAIL)
        self.relief = src.na(NA_RELIEF)
        self.toen = src.image("toen", TOEN_SHEET)


# ---------------------------------------------------------------------------------------------
# Ground

# Colours of NA's dirt-on-grass island block (TilesetFloor.png, cells (0,7)-(3,10)).
DIRT_C, DIRT_STREAK = "#d3865f", "#bd7959"
GRASS_DARK, GRASS_DARK_TUFT = "#74a334", "#56864c"


def plain_cells(k: Kit) -> list[Image.Image]:
    base = cell(k.floor, 0, 12)
    tufts = [cell(k.floor, c, 12) for c in range(1, 5)]
    return [base] * 6 + tufts


def grass_auto(k: Kit) -> dict[int, Image.Image]:
    """Dark meadow on the light plain: NA's dirt island with the dirt turned dark green."""
    shapes = island(k.floor, 0, 7)
    return {m: swap(im, {DIRT_C: GRASS_DARK, DIRT_STREAK: GRASS_DARK_TUFT}) for m, im in shapes.items()}


def grass_decor(k: Kit) -> list[Image.Image]:
    tufts = [
        swap(cell(k.detail, c, 2), {"#adbc3a": "#56864c", "#a8a129": "#345a52", "#74a334": "#56864c"})
        for c in (0, 1, 2, 3)
    ]
    return [new(T, T)] * 4 + tufts


def dirt_auto(k: Kit) -> dict[int, Image.Image]:
    return island(k.floor, 0, 7)


def wasteland_auto(k: Kit) -> dict[int, Image.Image]:
    """Dry, stony ground: the dirt island in a pale grey-brown."""
    shapes = island(k.floor, 0, 7)
    return {m: swap(im, {DIRT_C: "#b3957f", DIRT_STREAK: "#8e7c73"}) for m, im in shapes.items()}


def wasteland_decor(k: Kit) -> list[Image.Image]:
    stones, tuft = art.terrain("stones"), art.terrain("dry_tuft")
    rock = cell(k.detail, 15, 0)
    return [
        new(T, T),
        place((stones, 3, 5)),
        place((tuft, 8, 9)),
        place((stones, 6, 9), (tuft, 1, 2)),
        place((tuft, 2, 4), (tuft, 9, 10)),
        rock,
    ]


# Sand-coloured bits NA put on a few river banks; they become plain grass so banks stay uniform.
WATER_SAND = {"#ffcb8d": "#adbc3a", "#d78b4a": "#a8a129", "#ffad5d": "#adbc3a"}


def river_auto(k: Kit) -> dict[int, Image.Image]:
    """Water with a white foam line and earthen banks, from NA's pond-on-grass island block.

    That block's single-blob cell is a sand patch, so the lone pond (mask 0) is made of the
    rounded top and bottom ends of the vertical strip.
    """
    shapes = {m: swap(im, WATER_SAND) for m, im in island(k.water, 0, 6).items()}
    lone = new(T, T)
    lone.paste(shapes[S].crop((0, 0, T, T // 2)), (0, 0))
    lone.paste(shapes[N].crop((0, T // 2, T, T)), (0, T // 2))
    shapes[0] = lone
    return shapes


# ---------------------------------------------------------------------------------------------
# Relief


def _rock_texture(x: int, y: int) -> str:
    h = hash2(x // 2, y, 7) % 24
    return "#f2eaf1" if h == 0 else "#8d977f" if h in (1, 2, 3) else "#abc2bc"


def cliff_auto(k: Kit) -> dict[int, Image.Image]:
    """A raised rock mass with a front face: NA's grey plateau block, its snowy top made rocky."""
    block = [[cell(k.relief, c, r) for c in range(1, 4)] for r in range(3)]
    shapes = quad_auto(block)
    snow = rgba("#f2eaf1")
    out = {}
    for m, im in shapes.items():
        px = im.load()
        for y in range(T):
            for x in range(T):
                if px[x, y][:3] == snow[:3]:
                    px[x, y] = rgba(_rock_texture(x, y))
        out[m] = im
    return out


# Toen's colours -> NA palette (explicit so shading contrast and hue families are kept).
TOEN_TREES = {
    "#383827": "#141b1b",
    "#264928": "#141b1b",
    "#524a28": "#695953",
    "#475839": "#23604a",
    "#376a25": "#23604a",
    "#378c38": "#345a52",
    "#37ad40": "#56864c",
    "#3fc94f": "#74a334",
}
TOEN_PEAKS = {
    "#5a3825": "#141b1b",
    "#583838": "#3b3643",
    "#594848": "#4e484a",
    "#7d6862": "#695953",
    "#9b685b": "#8e7c73",
    "#ab948d": "#b3957f",
    "#d7d8c0": "#f2eaf1",
}


def forest_trees(k: Kit) -> list[Image.Image]:
    trees = [swap(cell(k.toen, c, 0), TOEN_TREES) for c in (4, 5, 6)]
    return trees + [flip_h(t) for t in trees]


def mountain_peaks(k: Kit) -> list[Image.Image]:
    peaks = [inner_outline(swap(cell(k.toen, c, 1), TOEN_PEAKS), "#141b1b") for c in (3, 4, 5)]
    return peaks + [flip_h(p) for p in peaks]


# ---------------------------------------------------------------------------------------------
# Structures generated in the NA palette

OUTLINE = "#141b1b"


def paving_cells() -> list[Image.Image]:
    """Castle floor: warm flagstones in staggered rows, three shade variations."""
    out = []
    for variant in range(3):
        img = canvas("#d2b37d")
        for y in range(T):
            row = y // 4
            for x in range(T):
                sx = (x + (4 if row % 2 else 0)) % T
                stone = (row, (x + (4 if row % 2 else 0)) // 8)
                if y % 4 == 3 or sx % 8 == 7:
                    put(img, x, y, "#b3957f")
                elif y % 4 == 0 or sx % 8 == 0:
                    put(img, x, y, "#eecf9b")
                elif hash2(stone[0], stone[1], variant) % 5 == 0:
                    put(img, x, y, "#c8966b" if (x + y) % 2 else "#d2b37d")
        out.append(img)
    return out


# Castle wall colours.
PARAPET, MERLON, WALKWAY, FACE, MORTAR, SHADE = "#abc2bc", "#f2eaf1", "#8d977f", "#5f7160", "#4e484a", "#3b3643"


def wall_cell(m: int) -> Image.Image:
    """Stone city wall seen from the south-west-above: walkway, crenellated parapets on the open
    sides and a brick front face where the wall does not continue southwards."""
    img = canvas(WALKWAY)
    # walkway texture: a few paving joints
    for y in range(T):
        for x in range(T):
            if hash2(x, y, 3) % 11 == 0:
                put(img, x, y, "#8e7c73")
    bottom = T if m & S else 10
    if not m & N:
        for x in range(T):
            put(img, x, 0, OUTLINE)
            put(img, x, 1, MERLON if x % 3 != 2 else FACE)
            put(img, x, 2, PARAPET)
            put(img, x, 3, FACE)
    if not m & S:
        for x in range(T):
            put(img, x, 6, MERLON if x % 3 != 2 else FACE)
            put(img, x, 7, PARAPET)
            put(img, x, 8, PARAPET)
            put(img, x, 9, OUTLINE)
            for y in range(10, T):
                if y == T - 1:
                    c = SHADE
                elif y in (12,) or (y < 12 and (x + 1) % 6 == 0) or (y > 12 and (x + 4) % 6 == 0):
                    c = MORTAR
                else:
                    c = FACE
                put(img, x, y, c)
    for side, xs in ((W, (0, 1, 2, 3)), (E, (15, 14, 13, 12))):
        if m & side:
            continue
        o, mer, par, inner = xs
        for y in range(bottom):
            put(img, o, y, OUTLINE)
            put(img, mer, y, MERLON if y % 3 != 2 else FACE)
            put(img, par, y, PARAPET)
            put(img, inner, y, FACE)
        if not m & S:
            for y in range(10, T):
                put(img, o, y, OUTLINE)
    return img


def wall_auto() -> dict[int, Image.Image]:
    return {m: wall_cell(m) for m in range(16)}


# Palisade colours.
STAKE_LIGHT, STAKE, STAKE_DARK = "#c8966b", "#a3754e", "#816855"


def _stakes_row(img: Image.Image, x0: int, x1: int, top: int, base: int) -> None:
    """A row of pointed stakes (3 px wide, 1 px outline between) filling columns x0..x1-1."""
    for x in range(x0, x1):
        p = (x - x0) % 4
        if p == 0:
            for y in range(top + 1, base + 1):
                put(img, x, y, OUTLINE)
            continue
        if p == 2:
            put(img, x, top, OUTLINE)
            put(img, x, top + 1, STAKE_LIGHT)
        else:
            put(img, x, top + 1, OUTLINE)
        for y in range(top + 2, base):
            put(img, x, y, STAKE_LIGHT if p == 1 else STAKE if p == 2 else STAKE_DARK)
        put(img, x, base, OUTLINE)
    for x in range(x0, x1):  # binding rope
        if (x - x0) % 4:
            put(img, x, base - 3, STAKE_DARK)


def _stakes_column(img: Image.Image, y0: int, y1: int) -> None:
    """A north-south run of stakes seen from above: a column of log tops."""
    for y in range(y0, y1):
        q = y % 4
        put(img, 6, y, OUTLINE)
        put(img, 9, y, OUTLINE)
        if q == 0:
            put(img, 7, y, OUTLINE)
            put(img, 8, y, OUTLINE)
        else:
            put(img, 7, y, STAKE_LIGHT if q == 1 else STAKE)
            put(img, 8, y, STAKE if q == 1 else STAKE_DARK)


def fence_cell(m: int) -> Image.Image:
    img = new(T, T)
    vertical = bool(m & (N | S))
    horizontal = bool(m & (E | W)) or not vertical
    if vertical:
        _stakes_column(img, 0 if m & N else 6, T if m & S else 11)
    if horizontal:
        if m & (E | W):
            x0 = 0 if m & W else 6
            x1 = T if m & E else 10
        else:
            x0, x1 = 2, 14
        _stakes_row(img, x0, x1, 3, 12)
    return img


def fence_auto() -> dict[int, Image.Image]:
    return {m: fence_cell(m) for m in range(16)}


def palisade_front() -> Image.Image:
    img = new(T, T)
    _stakes_row(img, 0, T, 9, T - 1)
    return img


def bridge_deck(east_west: bool) -> Image.Image:
    """Wooden deck with side rails; boards run across the direction of travel."""
    img = new(T, T)
    for x in range(T):
        put(img, x, 1, OUTLINE)
        put(img, x, 2, STAKE_LIGHT)
        put(img, x, 3, STAKE_DARK)
        for y in range(4, 12):
            if x % 4 == 3:
                c = "#695953"
            elif y == 4:
                c = STAKE_LIGHT
            else:
                c = STAKE
            put(img, x, y, c)
        put(img, x, 12, STAKE_LIGHT)
        put(img, x, 13, STAKE_DARK)
        put(img, x, 14, OUTLINE)
    for x in (0, 1, 7, 8, 14, 15):  # rail posts
        for y in (1, 12):
            put(img, x, y, "#695953")
    return img if east_west else img.transpose(Image.Transpose.TRANSPOSE)


def bridge_auto() -> dict[int, Image.Image]:
    """Deck orientation from the neighbouring water: water north/south -> deck spans east-west."""
    ew, ns = bridge_deck(True), bridge_deck(False)
    out = {}
    for m in range(16):
        across_ns = bool(m & (E | W)) and not m & (N | S)
        out[m] = ns if across_ns else ew
    return out


# ---------------------------------------------------------------------------------------------
# Buildings (art.py) on their ground


def village_cells() -> list[Image.Image]:
    hut = art.terrain("hut")
    sack = art.terrain("sack")
    a = place((hut, 0, 0), (hut, 4, 6))
    b = place((flip_h(hut), 4, 0), (flip_h(hut), 0, 6))
    c = place((hut, 2, 2), (sack, 10, 10), (sack, 1, 10))
    return [a, b, c]


def barracks_cells() -> list[Image.Image]:
    tent = art.terrain("tent")
    flag = art.terrain("pennant")
    a = place((flag, 10, 0), (tent, 0, 1), (tent, 4, 6))
    b = place((flip_h(flag), 0, 0), (flip_h(tent), 4, 1), (flip_h(tent), 0, 6))
    return [a, b]


def fort_cells() -> list[Image.Image]:
    flag = art.terrain("pennant")
    return [place((flag, 11, 0), (art.terrain("watchtower"), 1, 1), (palisade_front(), 0, 0))]


def granary_cells() -> list[Image.Image]:
    bale = art.terrain("bale")
    return [place((art.terrain("granary"), 0, 0), (bale, 0, 11), (bale, 9, 11), (bale, 5, 9))]


def treasury_cells() -> list[Image.Image]:
    return [place((art.terrain("treasury"), 0, 0), (art.terrain("chest"), 8, 10))]


def house_cells() -> list[Image.Image]:
    return [art.terrain("house")]


def gate_cells() -> list[Image.Image]:
    return [art.terrain("gate")]


# ---------------------------------------------------------------------------------------------
# Tile keys


def tiles(k: Kit) -> list[Tile]:
    plain = Layer(cells=plain_cells(k))
    green = Layer(auto=grass_auto(k), connect=GREEN)
    paving = Layer(cells=paving_cells())
    yard = Layer(auto=dirt_auto(k), connect=YARD)
    water = Layer(auto=river_auto(k), connect=WATER)
    return [
        Tile("plain", [plain], "light grass"),
        Tile("grass", [green, Layer(cells=grass_decor(k))], "dark meadow patches, joined with forest/mountain"),
        Tile("road", [Layer(auto=dirt_auto(k), connect=DIRT)], "dirt road, joins bridges, gates and settlements"),
        Tile("forest", [green, Layer(cells=forest_trees(k))], "pine groves on the meadow"),
        Tile("mountain", [green, Layer(cells=mountain_peaks(k))], "rocky snow-capped peaks"),
        Tile(
            "wasteland",
            [Layer(auto=wasteland_auto(k), connect=["wasteland"]), Layer(cells=wasteland_decor(k))],
            "dry stony ground",
        ),
        Tile("bridge", [water, Layer(auto=bridge_auto(), connect=["river"])], "deck oriented across the water"),
        Tile("castle", [paving], "flagstone castle floor"),
        Tile("gate", [paving, Layer(cells=gate_cells())], "open gate tower"),
        Tile("village", [yard, Layer(cells=village_cells())], "thatched farmhouses on a dirt yard"),
        Tile("barracks", [yard, Layer(cells=barracks_cells())], "army tents and a pennant"),
        Tile("fort", [yard, Layer(cells=fort_cells())], "watchtower behind a palisade"),
        Tile("granary", [paving, Layer(cells=granary_cells())], "round granary with rice sacks"),
        Tile("treasury", [paving, Layer(cells=treasury_cells())], "treasure house with a gold coin sign"),
        Tile("river", [water], "water with foam and banks"),
        Tile("wall", [Layer(auto=wall_auto(), connect=WALLS)], "crenellated stone wall, joins gates"),
        Tile("cliff", [Layer(auto=cliff_auto(k), connect=["cliff"])], "raised rock with a front face"),
        Tile("house", [plain, Layer(cells=house_cells())], "town house with a tiled roof"),
        Tile("fence", [plain, Layer(auto=fence_auto(), connect=["fence"])], "wooden palisade"),
    ]


HEADER = """\
# Battle map terrain (generated by tools/assets/build.py - do not edit by hand).
# Format: docs/ASSETS.md. One tile key per canonical terrain id; each is a stack of layers drawn
# bottom to top. `cells` = variants picked by a stable hash of the map position, `auto` = 16
# cells indexed by the mask of orthogonal neighbours whose terrain id is in `connect`
# (1 = north, 2 = east, 4 = south, 8 = west; outside the map counts as connected).
"""


def build_terrain(src: Sources, pack: Path) -> list[str]:
    kit = Kit(src)
    atlas = Atlas(columns=16)
    blank = atlas.add(new(T, T))  # (0, 0) stays empty so unused/transparent cells share it
    assert blank == [0, 0]
    lines = [HEADER, '\ntile_size = 16\nimage = "terrain.png"\n']
    all_tiles = tiles(kit)
    missing = [t for t in TERRAIN_IDS if t not in {tile.key for tile in all_tiles}]
    if missing:
        raise ValueError(f"terrain ids without a tile: {missing}")
    for tile in all_tiles:
        lines.append(f"\n# {tile.comment}\n[tiles.{tile.key}]\nlayers = [\n")
        for layer in tile.layers:
            if layer.auto is not None:
                cells = [atlas.add(layer.auto[m]) for m in range(16)]
                entry: dict[str, object] = {"auto": cells, "connect": list(layer.connect)}
            else:
                entry = {"cells": [atlas.add(c) for c in layer.cells]}
            lines.append(f"  {toml_value(entry)},\n")
        lines.append("]\n")
    out_dir = pack / "gfx" / "tiles"
    save_png(atlas.image(), out_dir / "terrain.png")
    write_text(out_dir / "terrain.toml", "".join(lines))
    return ["gfx/tiles/terrain.png", "gfx/tiles/terrain.toml"]
