"""Shared helpers of the asset pipeline: source registry, archive access, image and TOML utilities.

Everything here is deterministic: the same cached sources always produce byte-identical outputs.
"""

from __future__ import annotations

import hashlib
import io
import tomllib
import zipfile
from collections.abc import Iterable, Mapping, Sequence
from dataclasses import dataclass
from pathlib import Path

from PIL import Image

TOOLS_DIR = Path(__file__).resolve().parent
REPO_DIR = TOOLS_DIR.parent.parent
CACHE_DIR = TOOLS_DIR / ".cache"
PACK_DIR = REPO_DIR / "data" / "base"

RGBA = tuple[int, int, int, int]
TRANSPARENT: RGBA = (0, 0, 0, 0)

# ---------------------------------------------------------------------------------------------
# Source registry


@dataclass(frozen=True)
class Source:
    id: str
    title: str
    author: str
    license: str
    license_url: str
    homepage: str
    file: str
    size: int
    sha256: str
    url: str = ""
    itch_page: str = ""
    itch_upload: int = 0

    @property
    def path(self) -> Path:
        return CACHE_DIR / self.file


def load_sources(path: Path = TOOLS_DIR / "sources.toml") -> dict[str, Source]:
    with path.open("rb") as f:
        doc = tomllib.load(f)
    out: dict[str, Source] = {}
    for sid, entry in doc["sources"].items():
        itch = entry.get("itch", {})
        out[sid] = Source(
            id=sid,
            title=entry["title"],
            author=entry["author"],
            license=entry["license"],
            license_url=entry["license_url"],
            homepage=entry["homepage"],
            file=entry["file"],
            size=int(entry["size"]),
            sha256=entry["sha256"].lower(),
            url=entry.get("url", ""),
            itch_page=itch.get("page", ""),
            itch_upload=int(itch.get("upload", 0)),
        )
    return out


class SourceError(Exception):
    """A cached source is missing, altered or lacks a requested member."""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


class Sources:
    """Read-only access to the verified files in the cache (zip members or plain files)."""

    NA_ROOT = "Ninja Adventure - Asset Pack/"

    def __init__(self) -> None:
        self.meta = load_sources()
        self._zips: dict[str, zipfile.ZipFile] = {}
        self.used: set[str] = set()

    def verify(self) -> None:
        """Check every cached file against sources.toml before anything is built from it."""
        problems = []
        for src in self.meta.values():
            if not src.path.exists():
                problems.append(f"{src.id}: {src.path} missing (run tools/assets/fetch.py)")
            elif src.path.stat().st_size != src.size or sha256_file(src.path) != src.sha256:
                problems.append(f"{src.id}: {src.path} does not match the pinned size/sha256")
        if problems:
            raise SourceError("\n".join(problems))

    def _zip(self, sid: str) -> zipfile.ZipFile:
        if sid not in self._zips:
            self._zips[sid] = zipfile.ZipFile(self.meta[sid].path)
        return self._zips[sid]

    def member(self, sid: str, name: str) -> bytes:
        self.used.add(sid)
        try:
            return self._zip(sid).read(name)
        except KeyError as e:
            raise SourceError(f"{sid}: archive has no member {name!r}") from e

    def file(self, sid: str) -> bytes:
        self.used.add(sid)
        return self.meta[sid].path.read_bytes()

    def image(self, sid: str, name: str | None = None) -> Image.Image:
        data = self.file(sid) if name is None else self.member(sid, name)
        with Image.open(io.BytesIO(data)) as im:
            return im.convert("RGBA")

    def na(self, name: str) -> Image.Image:
        """An image from the Ninja Adventure pack, path relative to the pack root."""
        return self.image("ninja_adventure", self.NA_ROOT + name)

    def na_bytes(self, name: str) -> bytes:
        return self.member("ninja_adventure", self.NA_ROOT + name)

    def na_palette(self) -> list[RGBA]:
        """The Ninja Adventure master palette (`Palette.png`), used to fit other packs to its look."""
        return colors(self.na("Palette.png"))

    def close(self) -> None:
        for z in self._zips.values():
            z.close()
        self._zips.clear()


# ---------------------------------------------------------------------------------------------
# Colours


def rgba(value: str | Sequence[int]) -> RGBA:
    """`"#rrggbb"`, `"rrggbb"`, `"#rrggbbaa"` or an RGB(A) tuple -> RGBA tuple."""
    if isinstance(value, str):
        h = value.lstrip("#")
        if len(h) not in (6, 8):
            raise ValueError(f"bad colour {value!r}")
        vals = [int(h[i : i + 2], 16) for i in range(0, len(h), 2)]
        if len(vals) == 3:
            vals.append(255)
        return (vals[0], vals[1], vals[2], vals[3])
    v = tuple(value)
    if len(v) == 3:
        return (v[0], v[1], v[2], 255)
    if len(v) == 4:
        return (v[0], v[1], v[2], v[3])
    raise ValueError(f"bad colour {value!r}")


def luma(c: Sequence[int]) -> float:
    return 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]


def mix(a: Sequence[int], b: Sequence[int], t: float) -> RGBA:
    """Linear blend of two colours (alpha taken from `a`)."""
    return (
        round(a[0] + (b[0] - a[0]) * t),
        round(a[1] + (b[1] - a[1]) * t),
        round(a[2] + (b[2] - a[2]) * t),
        a[3] if len(a) > 3 else 255,
    )


def colors(img: Image.Image) -> list[RGBA]:
    """Distinct opaque colours of an image, sorted for determinism."""
    return sorted({c for c in pixels(img) if c[3] > 0})


def pixels(img: Image.Image) -> list[RGBA]:
    rgba_img = img if img.mode == "RGBA" else img.convert("RGBA")
    raw = rgba_img.tobytes()
    return [tuple(raw[i : i + 4]) for i in range(0, len(raw), 4)]  # type: ignore[misc]


def recolor(img: Image.Image, mapping: Mapping[Sequence[int], Sequence[int]], strict: bool = True) -> Image.Image:
    """Replace exact RGB colours (alpha preserved). Keys/values may be RGB or RGBA.

    With `strict`, every key must occur in the image, which catches stale colour tables when a
    source file changes.
    """
    table = {tuple(k[:3]): tuple(v[:3]) for k, v in mapping.items()}
    out = img.copy()
    px = out.load()
    seen: set[tuple[int, ...]] = set()
    for y in range(out.height):
        for x in range(out.width):
            c = px[x, y]
            if c[3] and c[:3] in table:
                seen.add(c[:3])
                r, g, b = table[c[:3]]
                px[x, y] = (r, g, b, c[3])
    if strict and len(seen) != len(table):
        missing = ", ".join("#{:02x}{:02x}{:02x}".format(*k) for k in table if k not in seen)
        raise ValueError(f"recolor: colour(s) not found in image: {missing}")
    return out


def ramp_mapping(sources: Iterable[Sequence[int]], ramp: Sequence[Sequence[int]]) -> dict[RGBA, RGBA]:
    """Map source colours onto a target ramp (dark -> light) by their luminance rank.

    With k source colours and n ramp entries, the i-th darkest source colour takes ramp entry
    round(i * (n - 1) / (k - 1)), so the darkest/lightest source colours always use the ends of
    the ramp and shading contrast is preserved.
    """
    src = sorted({rgba(c) for c in sources}, key=luma)
    n = len(ramp)
    out: dict[RGBA, RGBA] = {}
    for i, c in enumerate(src):
        j = 0 if len(src) == 1 else round(i * (n - 1) / (len(src) - 1))
        out[c] = rgba(ramp[j])
    return out


# ---------------------------------------------------------------------------------------------
# Image helpers


def new(w: int, h: int) -> Image.Image:
    return Image.new("RGBA", (w, h), TRANSPARENT)


def cell(img: Image.Image, col: int, row: int, w: int = 16, h: int | None = None) -> Image.Image:
    h = w if h is None else h
    return img.crop((col * w, row * h, col * w + w, row * h + h))


def paste(dst: Image.Image, src: Image.Image, x: int, y: int) -> None:
    """Alpha-composite `src` onto `dst` at (x, y); parts outside `dst` are clipped."""
    x0, y0 = max(0, x), max(0, y)
    x1, y1 = min(dst.width, x + src.width), min(dst.height, y + src.height)
    if x0 >= x1 or y0 >= y1:
        return
    part = src.crop((x0 - x, y0 - y, x1 - x, y1 - y))
    dst.alpha_composite(part, (x0, y0))


def flip_h(img: Image.Image) -> Image.Image:
    return img.transpose(Image.Transpose.FLIP_LEFT_RIGHT)


def bbox(img: Image.Image) -> tuple[int, int, int, int]:
    box = img.getchannel("A").getbbox()
    if box is None:
        raise ValueError("image is fully transparent")
    return box


def trim(img: Image.Image) -> Image.Image:
    return img.crop(bbox(img))


def outline(img: Image.Image, color: Sequence[int], diagonal: bool = False) -> Image.Image:
    """Add a 1 px outline around the opaque pixels (image grows by 1 px on each side)."""
    w, h = img.size
    out = new(w + 2, h + 2)
    src = img.load()
    dst = out.load()
    c = rgba(color)
    offs = [(-1, 0), (1, 0), (0, -1), (0, 1)]
    if diagonal:
        offs += [(-1, -1), (1, -1), (-1, 1), (1, 1)]
    for y in range(h):
        for x in range(w):
            if src[x, y][3]:
                for dx, dy in offs:
                    dst[x + 1 + dx, y + 1 + dy] = c
    out.alpha_composite(img, (1, 1))
    return out


def silhouette(img: Image.Image, color: Sequence[int]) -> Image.Image:
    """Every opaque pixel painted `color` (alpha kept)."""
    out = img.copy()
    px = out.load()
    c = rgba(color)
    for y in range(out.height):
        for x in range(out.width):
            a = px[x, y][3]
            if a:
                px[x, y] = (c[0], c[1], c[2], a)
    return out


def inner_outline(img: Image.Image, color: Sequence[int]) -> Image.Image:
    """Paint opaque pixels that touch transparency (4-neighbourhood, image border counts) in `color`.

    Unlike `outline` the image keeps its size, so sprites that already fill a 16x16 cell get the
    dark contour of the Ninja Adventure style without growing.
    """
    out = img.copy()
    src = img.load()
    dst = out.load()
    w, h = img.size
    c = rgba(color)
    for y in range(h):
        for x in range(w):
            if not src[x, y][3]:
                continue
            for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, ny = x + dx, y + dy
                if not (0 <= nx < w and 0 <= ny < h) or not src[nx, ny][3]:
                    dst[x, y] = c
                    break
    return out


def nearest_color(c: Sequence[int], palette: Sequence[Sequence[int]]) -> RGBA:
    """The palette entry closest to `c` (weighted RGB distance, ties broken by palette order)."""
    best = None
    best_d = None
    for p in palette:
        dr, dg, db = c[0] - p[0], c[1] - p[1], c[2] - p[2]
        d = 2 * dr * dr + 4 * dg * dg + 3 * db * db
        if best_d is None or d < best_d:
            best, best_d = p, d
    assert best is not None
    return (best[0], best[1], best[2], c[3] if len(c) > 3 else 255)


def to_palette(
    img: Image.Image,
    palette: Sequence[Sequence[int]],
    overrides: Mapping[str, str] | None = None,
) -> Image.Image:
    """Remap every opaque colour to its nearest palette colour; `overrides` ("#src" -> "#dst") win."""
    table = {rgba(k)[:3]: rgba(v) for k, v in (overrides or {}).items()}
    cache: dict[tuple[int, ...], RGBA] = {}

    def fn(c: RGBA) -> RGBA:
        key = c[:3]
        if key not in cache:
            cache[key] = table[key] if key in table else nearest_color(c, palette)
        r, g, b, _ = cache[key]
        return (r, g, b, c[3])

    return map_pixels(img, fn)


def map_pixels(img: Image.Image, fn) -> Image.Image:
    """Apply `fn(rgba) -> rgba` to every opaque pixel."""
    out = img.copy()
    px = out.load()
    for y in range(out.height):
        for x in range(out.width):
            c = px[x, y]
            if c[3]:
                px[x, y] = fn(c)
    return out


def hash2(x: int, y: int, seed: int = 0) -> int:
    """Small deterministic integer hash (0..2^32-1) used for texture noise."""
    h = (x * 374761393 + y * 668265263 + seed * 2246822519) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return h ^ (h >> 16)


def sprite(art: str, palette: Mapping[str, str | None]) -> Image.Image:
    """Build an image from text art: one character per pixel, `.` and space are transparent.

    Rows are the non-empty lines of `art` after stripping a common indentation; all rows must
    have the same length. Every other character must be a key of `palette` (value: colour).
    """
    lines = [ln for ln in art.strip("\n").splitlines()]
    lines = [ln.strip() for ln in lines if ln.strip()]
    width = len(lines[0])
    if any(len(ln) != width for ln in lines):
        raise ValueError("sprite rows have different lengths:\n" + "\n".join(lines))
    img = new(width, len(lines))
    px = img.load()
    for y, ln in enumerate(lines):
        for x, ch in enumerate(ln):
            if ch == ".":
                continue
            if ch not in palette:
                raise ValueError(f"sprite uses undefined palette key {ch!r}")
            value = palette[ch]
            if value is not None:
                px[x, y] = rgba(value)
    return img


# ---------------------------------------------------------------------------------------------
# Output


class Atlas:
    """A grid of equally sized cells; identical cells are stored once."""

    def __init__(self, columns: int, cell_w: int = 16, cell_h: int | None = None) -> None:
        self.columns = columns
        self.cw = cell_w
        self.ch = cell_w if cell_h is None else cell_h
        self.cells: list[Image.Image] = []
        self._index: dict[bytes, int] = {}

    def add(self, img: Image.Image) -> list[int]:
        if img.size != (self.cw, self.ch):
            raise ValueError(f"atlas cell must be {self.cw}x{self.ch}, got {img.size}")
        key = img.tobytes()
        if key not in self._index:
            self._index[key] = len(self.cells)
            self.cells.append(img.copy())
        i = self._index[key]
        return [i % self.columns, i // self.columns]

    def image(self) -> Image.Image:
        rows = max(1, (len(self.cells) + self.columns - 1) // self.columns)
        out = new(self.columns * self.cw, rows * self.ch)
        for i, c in enumerate(self.cells):
            out.alpha_composite(c, ((i % self.columns) * self.cw, (i // self.columns) * self.ch))
        return out


def save_png(img: Image.Image, path: Path) -> None:
    """Write a PNG deterministically; images with <= 256 RGBA colours are stored indexed."""
    path.parent.mkdir(parents=True, exist_ok=True)
    rgba_img = img.convert("RGBA")
    # Fully transparent pixels all become (0,0,0,0) so they share one palette entry.
    px = pixels(rgba_img)
    px = [p if p[3] else TRANSPARENT for p in px]
    palette = sorted(set(px))
    if len(palette) <= 256:
        index = {c: i for i, c in enumerate(palette)}
        out = Image.new("P", rgba_img.size)
        out.putdata([index[p] for p in px])
        flat: list[int] = []
        for c in palette:
            flat.extend(c[:3])
        out.putpalette(flat, rawmode="RGB")
        alpha = bytes(c[3] for c in palette)
        out.save(path, format="PNG", optimize=True, transparency=alpha)
    else:
        clean = Image.new("RGBA", rgba_img.size)
        clean.putdata(px)
        clean.save(path, format="PNG", optimize=True)


def toml_str(s: str) -> str:
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def toml_value(v: object) -> str:
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, (int, float)):
        return repr(v)
    if isinstance(v, str):
        return toml_str(v)
    if isinstance(v, (list, tuple)):
        return "[" + ", ".join(toml_value(x) for x in v) + "]"
    if isinstance(v, dict):
        return "{ " + ", ".join(f"{k} = {toml_value(x)}" for k, x in v.items()) + " }"
    raise TypeError(f"unsupported TOML value {v!r}")


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))
