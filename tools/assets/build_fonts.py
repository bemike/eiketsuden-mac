"""Fonts (`fonts/`): Galmuri11 and Galmuri9, completed with the Hanja the pack's text needs.

Galmuri lacks some Traditional Chinese characters used in officer names (e.g. 彧 in 荀彧, 龐 in
龐統). The build collects every CJK ideograph in the pack's text files (plus EXTRA_HANJA, needed
by names the campaign will use) and copies the ones Galmuri does not have from Fusion Pixel Font's
Korean build of the same pixel size: the 12 px font for Galmuri11 (12 px em), the 10 px font for
Galmuri9 (10 px em). Both are outline fonts drawn on a pixel grid of PIXEL units, so glyphs are
copied unscaled and only moved onto Galmuri's Hanja grid (the offset is measured on the
characters both fonts have).

A rare character missing from the donor as well can be composed from the pixel columns of two
glyphs the base font has (`FontJob.composed`: 豨 for Galmuri9 is 豕 from 豬 beside 希 from 稀).

Neither font has a Reserved Font Name (see their OFL texts), so the modified fonts keep their file
names, which the game loads, but carry their own family names ("Galmuri11 ER", "Galmuri9 ER") as
the OFL asks of modified versions. Both licences ship next to the fonts.

When the pack's text gains a Hanja the fonts lack, rebuild this step; a character that neither
font has and that has no composition stops the build before any font is written.
"""

from __future__ import annotations

import io
import itertools
import re
import statistics
from collections.abc import Mapping
from dataclasses import dataclass, field
from pathlib import Path

from fontTools.pens.recordingPen import RecordingPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables._g_l_y_f import Glyph

from assetlib import PACK_DIR, SourceError, Sources, write_text

# Needed by officer names of later chapters (李傕, 郭汜, 貂蟬, the chapter-1 bandit chief 昌豨, ...),
# whether or not the pack's text uses them yet.
EXTRA_HANJA = "傕彧昱汜蟬褚郃龐豨"

CJK = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")  # ext. A, unified, compatibility
# The pack's own text: data files, dramas and the credits (not the licence texts in fonts/).
TEXT_SUFFIXES = {".toml", ".drama"}
TEXT_FILES = {"credits.txt"}

FUSION_VERSION = "2026.09.01"
PIXEL = 100  # font units per pixel in Galmuri and Fusion Pixel (1000 units per em)

# A composed character: (component character, first pixel column, end column) of each part.
Composition = tuple[tuple[str, int, int], ...]


@dataclass(frozen=True)
class FontJob:
    out: str
    member: str  # file in the Galmuri archive
    family: str  # family name of the modified font
    fusion: str  # source id of the Fusion Pixel archive
    fusion_member: str
    composed: Mapping[str, Composition] = field(default_factory=dict)  # for characters the donor lacks


FONTS = [
    FontJob(
        "fonts/Galmuri11.ttf",
        "Galmuri11.ttf",
        "Galmuri11 ER",
        "fusion_pixel_12",
        "fusion-pixel-12px-proportional-ko.ttf.woff2",
    ),
    FontJob(
        "fonts/Galmuri9.ttf",
        "Galmuri9.ttf",
        "Galmuri9 ER",
        "fusion_pixel_10",
        "fusion-pixel-10px-proportional-ko.ttf.woff2",
        # Fusion Pixel's 10 px font has no 豨 (昌豨); Galmuri9 draws 豕 in columns 0-3 of 豬 and
        # 希 in columns 4-8 of 稀.
        composed={"豨": (("豬", 0, 4), ("稀", 4, 9))},
    ),
]


def pack_hanja(pack: Path = PACK_DIR) -> set[str]:
    """Every CJK ideograph in the pack's text files, plus EXTRA_HANJA."""
    chars = set(EXTRA_HANJA)
    for path in sorted(pack.rglob("*")):
        if (path.suffix in TEXT_SUFFIXES or path.name in TEXT_FILES) and path.is_file():
            chars.update(CJK.findall(path.read_text(encoding="utf-8")))
    return chars


def _bbox(font: TTFont, glyph: str) -> tuple[int, int, int, int]:
    g = font["glyf"][glyph]
    return (g.xMin, g.yMin, g.xMax, g.yMax)


def _grid_offset(base: TTFont, donor: TTFont, chars: set[str]) -> tuple[int, int]:
    """(dx, dy) moving the donor's ideographs onto the base font's grid, from shared characters."""
    bmap, dmap = base.getBestCmap(), donor.getBestCmap()
    shared = sorted(c for c in chars if ord(c) in bmap and ord(c) in dmap)
    if len(shared) < 5:
        raise SourceError("fonts: too few shared ideographs to align the donor font")
    dxs, dys = [], []
    for c in shared:
        b, d = _bbox(base, bmap[ord(c)]), _bbox(donor, dmap[ord(c)])
        dxs.append(b[0] - d[0])
        dys.append(b[3] - d[3])  # tops: the ideographs of both fonts fill the full box height
    return round(statistics.median(dxs)), round(statistics.median(dys))


def _add_glyph(base: TTFont, char: str, glyph: Glyph, advance: int) -> None:
    name = f"uni{ord(char):04X}"
    if name in base["glyf"].glyphs:
        raise SourceError(f"fonts: glyph {name} already exists")
    glyph.recalcBounds(None)
    base["glyf"][name] = glyph  # also appends the name to the glyph order
    base["hmtx"][name] = (advance, glyph.xMin if glyph.numberOfContours else 0)
    for table in base["cmap"].tables:
        if table.isUnicode():
            table.cmap[ord(char)] = name


def _copied_glyph(donor: TTFont, char: str, offset: tuple[int, int]) -> Glyph:
    pen = TTGlyphPen(None)
    source = donor.getBestCmap()[ord(char)]
    donor.getGlyphSet()[source].draw(TransformPen(pen, (1, 0, 0, 1, offset[0], offset[1])))
    return pen.glyph()


def _contours(font: TTFont, char: str) -> list[list[tuple[float, float]]]:
    """The outline of `char` as closed polygons; refuses curves (only pixel outlines are expected)."""
    pen = RecordingPen()
    font.getGlyphSet()[font.getBestCmap()[ord(char)]].draw(pen)
    contours: list[list[tuple[float, float]]] = []
    for op, args in pen.value:
        if op == "moveTo":
            contours.append([args[0]])
        elif op == "lineTo":
            contours[-1].append(args[0])
        elif op not in ("closePath", "endPath"):
            raise SourceError(f"fonts: {char} is not a pixel outline ({op})")
    return contours


def _winding(contours: list[list[tuple[float, float]]], x: float, y: float) -> int:
    """Winding number of the point (x, y); non-zero inside the outline."""
    w = 0
    for contour in contours:
        for (x0, y0), (x1, y1) in zip(contour, contour[1:] + contour[:1], strict=True):
            if (y0 <= y < y1 or y1 <= y < y0) and x < x0 + (y - y0) * (x1 - x0) / (y1 - y0):
                w += 1 if y1 > y0 else -1
    return w


def pixels(contours: list[list[tuple[float, float]]]) -> set[tuple[int, int]]:
    """Filled (column, row) pixels of a pixel-grid outline; row 0 sits on the baseline."""
    points = [p for c in contours for p in c]
    if not points:
        return set()
    cols = range(int(min(x for x, _ in points)) // PIXEL, -(-int(max(x for x, _ in points)) // PIXEL))
    rows = range(int(min(y for _, y in points)) // PIXEL, -(-int(max(y for _, y in points)) // PIXEL))
    return {(c, r) for c in cols for r in rows if _winding(contours, (c + 0.5) * PIXEL, (r + 0.5) * PIXEL)}


Point = tuple[int, int]


def outline(filled: set[tuple[int, int]]) -> list[list[Point]]:
    """The boundary loops of a pixel set, in pixel units, like Galmuri's own glyphs.

    Every pixel contributes its four edges clockwise (y up); edges shared by two pixels cancel,
    and the rest are linked into loops, turning right where two loops touch at a corner so they
    stay separate. Outer loops come out clockwise and holes anticlockwise, as TrueType wants.
    """
    edges: set[tuple[Point, Point]] = set()
    for c, r in filled:
        corners = [(c, r), (c, r + 1), (c + 1, r + 1), (c + 1, r)]
        for a, b in itertools.pairwise([*corners, corners[0]]):
            if (b, a) in edges:
                edges.remove((b, a))
            else:
                edges.add((a, b))
    outgoing: dict[Point, list[Point]] = {}
    for a, b in sorted(edges):
        outgoing.setdefault(a, []).append(b)
    loops = []
    while outgoing:
        start = min(outgoing)
        loop, prev, here = [start], None, start
        while True:
            options = outgoing[here]
            if prev is None or len(options) == 1:
                nxt = options[0]
            else:  # a corner shared by two loops: take the right turn
                dx, dy = here[0] - prev[0], here[1] - prev[1]
                nxt = next(p for p in options if (p[0] - here[0], p[1] - here[1]) == (dy, -dx))
            options.remove(nxt)
            if not options:
                del outgoing[here]
            prev, here = here, nxt
            if here == start:
                break
            loop.append(here)
        # drop the corners that are not turns
        n = len(loop)
        loops.append(
            [
                p
                for i, p in enumerate(loop)
                if (loop[i - 1][0] - p[0]) * (loop[(i + 1) % n][1] - p[1])
                != (loop[i - 1][1] - p[1]) * (loop[(i + 1) % n][0] - p[0])
            ]
        )
    return loops


def pixel_glyph(filled: set[tuple[int, int]]) -> Glyph:
    """A TrueType glyph drawing the pixels as the outline of their union."""
    pen = TTGlyphPen(None)
    for loop in outline(filled):
        pen.moveTo((loop[0][0] * PIXEL, loop[0][1] * PIXEL))
        for x, y in loop[1:]:
            pen.lineTo((x * PIXEL, y * PIXEL))
        pen.closePath()
    return pen.glyph()


def _composed_glyph(base: TTFont, char: str, parts: Composition) -> Glyph:
    bmap = base.getBestCmap()
    filled: set[tuple[int, int]] = set()
    for component, first, end in parts:
        if ord(component) not in bmap:
            raise SourceError(f"fonts: {char} is composed from {component}, which the base font lacks")
        filled |= {(c, r) for c, r in pixels(_contours(base, component)) if first <= c < end}
    return pixel_glyph(filled)


def _rename(font: TTFont, family: str, copied: str, composed: str) -> None:
    """Give the modified font its own names (OFL: a Modified Version must not pose as the original)."""
    name = font["name"]
    ps = family.replace(" ", "") + "-Regular"
    version = name.getDebugName(5) or ""
    replaced = {1, 3, 4, 6, 16, 17, 21, 22}
    name.names = [r for r in name.names if r.nameID not in replaced]
    copyright_ = name.getDebugName(0) or ""
    if copied:
        copyright_ += f". Hanja {copied}: Fusion Pixel Font, Copyright (c) 2022 TakWolf (https://takwolf.com)"
    added = [f"the Hanja {copied} added from Fusion Pixel Font"] if copied else []
    if composed:
        added.append(f"the Hanja {composed} composed from Galmuri's own glyphs")
    records = {
        0: copyright_,
        1: family,
        3: f"{ps};Eiketsuden Reloaded",
        4: f"{family} Regular",
        5: f"{version}; Eiketsuden Reloaded modification: Hanja from Fusion Pixel Font {FUSION_VERSION}",
        6: ps,
        10: f"Galmuri with {' and '.join(added)} for Eiketsuden Reloaded.",
    }
    for name_id, text in records.items():
        name.removeNames(nameID=name_id)
        name.setName(text, name_id, 3, 1, 0x409)
        if name_id in (1, 3, 4, 5, 6):  # ASCII-only records also for the Mac platform
            name.setName(text, name_id, 1, 0, 0)


@dataclass(frozen=True)
class Merged:
    data: bytes
    copied: str  # characters copied from Fusion Pixel Font
    composed: str  # characters composed from the base font's glyphs


def merge(base_bytes: bytes, donor_bytes: bytes, job: FontJob, needed: set[str]) -> Merged:
    """The base font with the needed characters it lacks copied from the donor or composed."""
    base = TTFont(io.BytesIO(base_bytes), recalcTimestamp=False)
    donor = TTFont(io.BytesIO(donor_bytes))
    bmap, dmap = base.getBestCmap(), donor.getBestCmap()
    missing = sorted(c for c in needed if ord(c) not in bmap)
    copied = "".join(c for c in missing if ord(c) in dmap)
    composed = "".join(c for c in missing if ord(c) not in dmap and c in job.composed)
    unavailable = "".join(c for c in missing if c not in copied and c not in composed)
    if unavailable:
        raise SourceError(
            f"fonts: {unavailable} missing from both {job.member} and {job.fusion_member} "
            "(add a composition to its FontJob in build_fonts.py)"
        )
    offset = _grid_offset(base, donor, needed)
    advance = base["hmtx"][bmap[ord("一")]][0]
    for c in copied:
        _add_glyph(base, c, _copied_glyph(donor, c, offset), advance)
    for c in composed:
        _add_glyph(base, c, _composed_glyph(base, c, job.composed[c]), advance)
    _rename(base, job.family, copied, composed)
    out = io.BytesIO()
    base.save(out)
    return Merged(out.getvalue(), copied, composed)


def _codes(chars: str) -> str:
    return " ".join(f"U+{ord(c):04X}" for c in chars) or "nothing"  # the console may not print Hanja


def build_fonts(src: Sources, pack: Path) -> list[str]:
    needed = pack_hanja()
    # merge every font before writing any, so a failure leaves the committed fonts untouched
    merged = [
        (job, merge(src.member("galmuri", job.member), src.member(job.fusion, job.fusion_member), job, needed))
        for job in FONTS
    ]
    written = []
    for job, m in merged:
        path = pack / job.out
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(m.data)
        written.append(job.out)
        note = f"; composed {_codes(m.composed)}" if m.composed else ""
        print(f"  {job.out}: added {_codes(m.copied)}{note}")
    galmuri = src.member("galmuri", "LICENSE.txt").decode("utf-8")
    write_text(pack / "fonts" / "OFL.txt", galmuri.replace("\r\n", "\n"))
    written.append("fonts/OFL.txt")
    write_text(pack / "fonts" / "OFL-FusionPixel.txt", _fusion_licences(src))
    written.append("fonts/OFL-FusionPixel.txt")
    return written


def _fusion_licences(src: Sources) -> str:
    """Fusion Pixel Font's OFL and the licences of the fonts it is built from, in one file."""
    parts = [
        "The Hanja copied into Galmuri11.ttf and Galmuri9.ttf (see CREDITS.md) come from Fusion Pixel\n"
        f"Font {FUSION_VERSION} (https://github.com/TakWolf/fusion-pixel-font), which is licensed as\n"
        "follows and itself includes glyphs of the fonts whose licences follow it.\n"
    ]
    seen = set()
    for sid in ("fusion_pixel_12", "fusion_pixel_10"):
        for member in ["OFL.txt", *sorted(n for n in src.members(sid) if n.startswith("LICENSES/"))]:
            if member.endswith("/") or member in seen or member.startswith("LICENSES/galmuri/"):
                continue  # Galmuri's own licence is fonts/OFL.txt
            seen.add(member)
            text = src.member(sid, member).decode("utf-8").replace("\r\n", "\n").strip()
            title = "Fusion Pixel Font" if member == "OFL.txt" else member.split("/")[1]
            parts.append(f"{'=' * 72}\n{title}\n{'=' * 72}\n\n{text}\n")
    return "\n".join(parts)
