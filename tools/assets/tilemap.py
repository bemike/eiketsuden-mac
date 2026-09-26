"""A reference renderer for `gfx/tiles/terrain.toml` (docs/ASSETS.md), used by preview.py and the
title artwork.

It follows the documented rules: each tile key is a stack of layers drawn bottom to top; `cells`
picks one variant per map position with a stable hash; `auto` picks one of 16 cells from the 4-bit
mask of orthogonal neighbours whose terrain id is in `connect` (1 = north, 2 = east, 4 = south,
8 = west; outside the map counts as connected); `offset` shifts a layer; `frames` + `fps` animate
it. The hash is this tool's own - the engine may pick different variants, which is fine because
every variant is valid everywhere.
"""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from pathlib import Path

from PIL import Image

from assetlib import cell, hash2, new, paste

NORTH, EAST, SOUTH, WEST = 1, 2, 4, 8

# Map glyphs of docs/ASSETS.md (canonical terrain ids).
GLYPHS: dict[str, str] = {
    ".": "plain",
    ",": "grass",
    "_": "road",
    "T": "forest",
    "^": "mountain",
    ":": "wasteland",
    "=": "bridge",
    "c": "castle",
    "G": "gate",
    "v": "village",
    "b": "barracks",
    "f": "fort",
    "g": "granary",
    "$": "treasury",
    "~": "river",
    "#": "wall",
    "X": "cliff",
    "H": "house",
    "|": "fence",
}


@dataclass(frozen=True)
class Layer:
    cells: list[tuple[int, int]] | None
    auto: list[tuple[int, int]] | None
    connect: frozenset[str]
    offset: tuple[int, int]
    frames: list[list[tuple[int, int]]] | None
    fps: float


class Tileset:
    def __init__(self, toml_path: Path) -> None:
        with toml_path.open("rb") as f:
            doc = tomllib.load(f)
        self.size: int = doc["tile_size"]
        with Image.open(toml_path.parent / doc["image"]) as im:
            self.image = im.convert("RGBA")
        self.tiles: dict[str, list[Layer]] = {}
        for key, tile in doc["tiles"].items():
            layers = []
            for raw in tile["layers"]:
                auto = raw.get("auto")
                if auto is not None and len(auto) != 16:
                    raise ValueError(f"tiles.{key}: auto layer needs 16 cells, has {len(auto)}")
                layers.append(
                    Layer(
                        cells=[tuple(c) for c in raw["cells"]] if "cells" in raw else None,
                        auto=[tuple(c) for c in auto] if auto is not None else None,
                        connect=frozenset(raw.get("connect", [])),
                        offset=tuple(raw.get("offset", [0, 0])),
                        frames=[[tuple(c) for c in f] for f in raw["frames"]] if "frames" in raw else None,
                        fps=float(raw.get("fps", 0)),
                    )
                )
            self.tiles[key] = layers
        self._cells: dict[tuple[int, int], Image.Image] = {}

    def cell(self, pos: tuple[int, int]) -> Image.Image:
        if pos not in self._cells:
            self._cells[pos] = cell(self.image, pos[0], pos[1], self.size)
        return self._cells[pos]


def parse_map(text: str) -> list[list[str]]:
    """Rows of glyphs -> rows of terrain ids (unknown glyphs raise)."""
    rows = [ln.strip() for ln in text.strip("\n").splitlines() if ln.strip()]
    width = len(rows[0])
    if any(len(r) != width for r in rows):
        raise ValueError("map rows have different lengths")
    return [[GLYPHS[ch] for ch in r] for r in rows]


def mask_at(grid: list[list[str]], x: int, y: int, connect: frozenset[str]) -> int:
    h, w = len(grid), len(grid[0])
    m = 0
    for bit, (dx, dy) in ((NORTH, (0, -1)), (EAST, (1, 0)), (SOUTH, (0, 1)), (WEST, (-1, 0))):
        nx, ny = x + dx, y + dy
        if not (0 <= nx < w and 0 <= ny < h) or grid[ny][nx] in connect:
            m |= bit
    return m


def render(tileset: Tileset, grid: list[list[str]], time: float = 0.0) -> Image.Image:
    """Draw a map (rows of terrain ids; the tile key is the id) at 1x."""
    ts = tileset.size
    h, w = len(grid), len(grid[0])
    out = new(w * ts, h * ts)
    for y in range(h):
        for x in range(w):
            for i, layer in enumerate(tileset.tiles[grid[y][x]]):
                if layer.auto is not None:
                    pos = layer.auto[mask_at(grid, x, y, layer.connect)]
                elif layer.frames is not None:
                    frame = layer.frames[int(time * layer.fps) % len(layer.frames)]
                    pos = frame[hash2(x, y, i) % len(frame)]
                else:
                    assert layer.cells is not None
                    pos = layer.cells[hash2(x, y, i) % len(layer.cells)]
                paste(out, tileset.cell(pos), x * ts + layer.offset[0], y * ts + layer.offset[1])
    return out
