"""Fonts (`fonts/`): Galmuri11 and Galmuri9, completed with the Hanja the pack's text needs.

Galmuri lacks some Traditional Chinese characters used in officer names (e.g. 彧 in 荀彧, 龐 in
龐統). The build collects every CJK ideograph in the pack's text files (plus EXTRA_HANJA, needed
by names the campaign will use) and copies the ones Galmuri does not have from Fusion Pixel Font's
Korean build of the same pixel size: the 12 px font for Galmuri11 (12 px em), the 10 px font for
Galmuri9 (10 px em). Both are outline fonts drawn on a pixel grid of 100 units, so glyphs are
copied unscaled and only moved onto Galmuri's Hanja grid (the offset is measured on the
characters both fonts have).

Neither font has a Reserved Font Name (see their OFL texts), so the modified fonts keep their file
names, which the game loads, but carry their own family names ("Galmuri11 ER", "Galmuri9 ER") as
the OFL asks of modified versions. Both licences ship next to the fonts.

When the pack's text gains a Hanja the fonts lack, rebuild this step; a character that neither
font has stops the build.
"""

from __future__ import annotations

import io
import re
import statistics
from dataclasses import dataclass
from pathlib import Path

from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

from assetlib import PACK_DIR, SourceError, Sources, write_text

# Needed by officer names of later chapters (李傕, 郭汜, 貂蟬, ...), whether or not the pack's
# text uses them yet.
EXTRA_HANJA = "傕彧昱汜蟬褚郃龐"

CJK = re.compile(r"[㐀-䶿一-鿿豈-﫿]")
# The pack's own text: data files, dramas and the credits (not the licence texts in fonts/).
TEXT_SUFFIXES = {".toml", ".drama"}
TEXT_FILES = {"credits.txt"}

FUSION_VERSION = "2026.09.01"


@dataclass(frozen=True)
class FontJob:
    out: str
    member: str  # file in the Galmuri archive
    family: str  # family name of the modified font
    fusion: str  # source id of the Fusion Pixel archive
    fusion_member: str


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


def _copy_glyph(base: TTFont, donor: TTFont, char: str, offset: tuple[int, int], advance: int) -> str:
    name = f"uni{ord(char):04X}"
    if name in base["glyf"].glyphs:
        raise SourceError(f"fonts: glyph {name} already exists")
    source = donor.getBestCmap()[ord(char)]
    pen = TTGlyphPen(None)
    donor.getGlyphSet()[source].draw(TransformPen(pen, (1, 0, 0, 1, offset[0], offset[1])))
    glyph = pen.glyph()
    glyph.recalcBounds(None)
    base["glyf"][name] = glyph  # also appends the name to the glyph order
    base["hmtx"][name] = (advance, glyph.xMin if glyph.numberOfContours else 0)
    for table in base["cmap"].tables:
        if table.isUnicode():
            table.cmap[ord(char)] = name
    return name


def _rename(font: TTFont, family: str, added: str) -> None:
    """Give the modified font its own names (OFL: a Modified Version must not pose as the original)."""
    name = font["name"]
    ps = family.replace(" ", "") + "-Regular"
    version = name.getDebugName(5) or ""
    replaced = {1, 3, 4, 6, 16, 17, 21, 22}
    name.names = [r for r in name.names if r.nameID not in replaced]
    copyright_ = name.getDebugName(0) or ""
    records = {
        0: f"{copyright_}. Hanja {added}: Fusion Pixel Font, Copyright (c) 2022 TakWolf (https://takwolf.com)",
        1: family,
        3: f"{ps};Eiketsuden Reloaded",
        4: f"{family} Regular",
        5: f"{version}; Eiketsuden Reloaded modification: Hanja from Fusion Pixel Font {FUSION_VERSION}",
        6: ps,
        10: f"Galmuri with the Hanja {added} added from Fusion Pixel Font for Eiketsuden Reloaded.",
    }
    for name_id, text in records.items():
        name.removeNames(nameID=name_id)
        name.setName(text, name_id, 3, 1, 0x409)
        if name_id in (1, 3, 4, 5, 6):  # ASCII-only records also for the Mac platform
            name.setName(text, name_id, 1, 0, 0)


def merge(base_bytes: bytes, donor_bytes: bytes, family: str, needed: set[str]) -> tuple[bytes, str]:
    """The base font with the needed characters it lacks copied from the donor; also returns them."""
    base = TTFont(io.BytesIO(base_bytes), recalcTimestamp=False)
    donor = TTFont(io.BytesIO(donor_bytes))
    bmap, dmap = base.getBestCmap(), donor.getBestCmap()
    missing = "".join(sorted(c for c in needed if ord(c) not in bmap))
    unavailable = [c for c in missing if ord(c) not in dmap]
    if unavailable:
        raise SourceError(f"fonts: {''.join(unavailable)} missing from both Galmuri and Fusion Pixel Font")
    offset = _grid_offset(base, donor, needed)
    advance = base["hmtx"][bmap[ord("一")]][0]
    for c in missing:
        _copy_glyph(base, donor, c, offset, advance)
    _rename(base, family, missing)
    out = io.BytesIO()
    base.save(out)
    return out.getvalue(), missing


def build_fonts(src: Sources, pack: Path) -> list[str]:
    needed = pack_hanja()
    written = []
    for job in FONTS:
        data, added = merge(
            src.member("galmuri", job.member),
            src.member(job.fusion, job.fusion_member),
            job.family,
            needed,
        )
        path = pack / job.out
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        written.append(job.out)
        codes = " ".join(f"U+{ord(c):04X}" for c in added) or "nothing"
        print(f"  {job.out}: added {codes}")  # code points: the console may not print Hanja
    galmuri = src.member("galmuri", "LICENSE.txt").decode("utf-8")
    write_text(pack / "fonts" / "OFL.txt", galmuri.replace("\r\n", "\n"))
    written.append("fonts/OFL.txt")
    write_text(pack / "fonts" / "OFL-FusionPixel.txt", _fusion_licences(src))
    written.append("fonts/OFL-FusionPixel.txt")
    return written


def _fusion_licences(src: Sources) -> str:
    """Fusion Pixel Font's OFL and the licences of the fonts it is built from, in one file."""
    parts = [
        "The Hanja added to Galmuri11.ttf and Galmuri9.ttf (see CREDITS.md) come from Fusion Pixel\n"
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
