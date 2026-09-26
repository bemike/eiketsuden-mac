"""Battle map terrain: `gfx/tiles/terrain.png` (16x16 atlas) + `gfx/tiles/terrain.toml`.

Every canonical terrain id of docs/ASSETS.md gets a tile key of the same name. Ground types are
4-bit autotiles cut from the Ninja Adventure (NA) floor/water tilesets, objects (trees, peaks,
buildings) are layered on top. Pieces from MiniWorld and Toen's pack are remapped to the NA
palette and given its dark contour so the whole map reads as one style.
"""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass, field
from pathlib import Path

from PIL import Image

import art
from assetlib import (
    RGBA,
    Atlas,
    Sources,
    cell,
    flip_h,
    hash2,
    inner_outline,
    map_pixels,
    new,
    paste,
    recolor,
    rgba,
    save_png,
    to_palette,
    toml_value,
    trim,
    write_text,
)

T = 16
N, E, S, W = 1, 2, 4, 8
OUTLINE = rgba("#141b1b")

NA_FLOOR = "Backgrounds/Tilesets/TilesetFloor.png"
NA_WATER = "Backgrounds/Tilesets/TilesetWater.png"
NA_NATURE = "Backgrounds/Tilesets/TilesetNature.png"
NA_DETAIL = "Backgrounds/Tilesets/TilesetFloorDetail.png"
NA_HOUSE = "Backgrounds/Tilesets/TilesetHouse.png"
NA_RELIEF = "Backgrounds/Tilesets/TilesetRelief.png"
NA_INTERIOR_FLOOR = "Backgrounds/Tilesets/Interior/TilesetInteriorFloor.png"
NA_CAMP = "Backgrounds/Tilesets/tileset_camp.png"
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

# Terrain ids (docs/ASSETS.md).
LAND = [
    "plain",
    "grass",
    "road",
    "forest",
    "mountain",
    "wasteland",
    "castle",
    "gate",
    "village",
    "barracks",
    "fort",
    "granary",
    "treasury",
    "house",
    "fence",
    "wall",
    "cliff",
]
ALL_IDS = LAND + ["bridge", "river"]


# ---------------------------------------------------------------------------------------------
# Tile description


@dataclass
class Layer:
    """One layer of a tile key: `cells` variants (weights by repetition) or a 16-cell `auto`."""

    cells: list[Image.Image] = field(default_factory=list)
    auto: dict[int, Image.Image] | None = None
    connect: Sequence[str] = ()
    offset: tuple[int, int] = (0, 0)


@dataclass
class Tile:
    key: str
    layers: list[Layer]
    comment: str = ""


def island(sheet: Image.Image, col: int, row: int) -> dict[int, Image.Image]:
    """The 16 mask cells of an NA island block whose top-left cell is (col, row)."""
    return {m: cell(sheet, col + dx, row + dy) for m, (dx, dy) in ISLAND.items()}


def swap(img: Image.Image, table: dict[str, str]) -> Image.Image:
    return recolor(img, {rgba(k): rgba(v) for k, v in table.items()}, strict=False)


def masks(auto: dict[int, Image.Image], fn: Callable[[int, Image.Image], Image.Image]) -> dict[int, Image.Image]:
    return {m: fn(m, img) for m, img in auto.items()}


def over(*imgs: Image.Image) -> Image.Image:
    """Alpha-composite 16x16 images bottom to top."""
    out = new(T, T)
    for im in imgs:
        out.alpha_composite(im)
    return out


def place(img: Image.Image, x: int, y: int) -> Image.Image:
    """A 16x16 cell with `img` pasted at (x, y) (clipped)."""
    out = new(T, T)
    paste(out, img, x, y)
    return out


def bottom_centre(img: Image.Image, dx: int = 0, dy: int = 0) -> Image.Image:
    img = trim(img)
    return place(img, (T - img.width) // 2 + dx, T - img.height + dy)

# ---------------------------------------------------------------------------------------------
# Ground


class Kit:
    """Source images shared by the tile builders (loaded once)."""

    def __init__(self, src: Sources) -> None:
        self.src = src
        self.floor = src.na(NA_FLOOR)
        self.water = src.na(NA_WATER)
        self.nature = src.na(NA_NATURE)
        self.detail = src.na(NA_DETAIL)
        self.house = src.na(NA_HOUSE)
        self.relief = src.na(NA_RELIEF)
        self.camp = src.na(NA_CAMP)
        self.toen = src.image("toen", TOEN_SHEET)
        self.palette = src.na_palette()

    def na_cell(self, sheet: Image.Image, col: int, row: int) -> Image.Image:
        return cell(sheet, col, row)

    def fit(self, img: Image.Image, overrides: dict[str, str] | None = None, contour: bool = True) -> Image.Image:
        """Remap a MiniWorld/Toen sprite to the NA palette and give it the NA dark contour."""
        out = to_palette(img, self.palette, overrides)
        return inner_outline(out, OUTLINE) if contour else out


# Colours of NA's dirt-on-grass island block (TilesetFloor.png, cells (0,7)-(3,10)).
DIRT, DIRT_STREAK = "#d3865f", "#bd7959"
GRASS_LIGHT, GRASS_TUFT = "#adbc3a", "#a8a129"
GRASS_DARK, GRASS_DARK_TUFT = "#74a334", "#56864c"


def plain_cells(k: Kit) -> list[Image.Image]:
    base = cell(k.floor, 0, 12)
    tufts = [cell(k.floor, c, 12) for c in range(1, 5)]
    flowers = over(base, cell(k.detail, 5, 2))
    return [base] * 6 + tufts + [flowers]


def grass_auto(k: Kit) -> dict[int, Image.Image]:
    """Dark meadow patches on the light plain: NA's dirt island with the dirt turned dark green."""
    shapes = island(k.floor, 0, 7)
    return masks(shapes, lambda m, im: swap(im, {DIRT: GRASS_DARK, DIRT_STREAK: GRASS_DARK}))


def grass_decor(k: Kit) -> list[Image.Image]:
    blank = new(T, T)
    tufts = [cell(k.detail, c, 2) for c in (0, 1, 2, 3)]
    return [blank] * 2 + tufts


def road_auto(k: Kit) -> dict[int, Image.Image]:
    return island(k.floor, 0, 7)


def wasteland_auto(k: Kit) -> dict[int, Image.Image]:
    shapes = island(k.floor, 0, 7)
    return masks(shapes, lambda m, im: swap(im, {DIRT: "#ffad5d", DIRT_STREAK: "#ef914f"}))


def wasteland_decor(k: Kit) -> list[Image.Image]:
    blank = new(T, T)
    return [blank] + [cell(k.detail, c, 0) for c in (4, 5, 6, 7, 1, 14)]


def river_auto(k: Kit) -> dict[int, Image.Image]:
    return island(k.water, 0, 6)
