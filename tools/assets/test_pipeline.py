"""Unit tests of the pipeline's pure logic (no cached sources needed).

Run with `python -m unittest discover -s tools/assets -p "test_*.py"`.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from assetlib import SourceError, load_sources
from build_backgrounds import HIGHLIGHT_MAX, _tone_curve
from build_fonts import EXTRA_HANJA, outline, pack_hanja, pixel_glyph, pixels
from build_music import Track, _filters
from build_portraits import SIZE, Portrait, check_table

SOURCE_HEAD = """
[sources.book]
title = "t"
author = "a"
license = "public-domain"
license_url = "https://example.org/l"
homepage = "https://example.org/h"
"""


def write(dir_: Path, name: str, text: str) -> Path:
    path = dir_ / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


class SourcesTest(unittest.TestCase):
    def load(self, text: str):
        with tempfile.TemporaryDirectory() as tmp:
            return load_sources(write(Path(tmp), "sources.toml", SOURCE_HEAD + text))

    def test_parts_expand_templates_and_accept_own_urls(self) -> None:
        src = self.load(
            """
url = "https://example.org/page{part}.jpg"
file = "book/page{part}.jpg"
[sources.book.parts]
16 = { size = 1, sha256 = "AB" }
17 = { size = 2, sha256 = "cd", url = "https://mirror.example.org/x.jpg" }
"""
        )["book"]
        self.assertTrue(src.multipart)
        self.assertEqual([p.name for p in src.parts], ["16", "17"])
        self.assertEqual(src.part("16").url, "https://example.org/page16.jpg")
        self.assertEqual(src.part("16").file, "book/page16.jpg")
        self.assertEqual(src.part("16").sha256, "ab")
        self.assertEqual(src.part("17").url, "https://mirror.example.org/x.jpg")
        with self.assertRaises(SourceError):
            src.part("18")

    def test_part_without_any_url_is_refused(self) -> None:
        with self.assertRaises(SourceError):
            self.load('file = "book/{part}.jpg"\n[sources.book.parts]\n1 = { size = 1, sha256 = "00" }\n')

    def test_plain_source_has_one_unnamed_part(self) -> None:
        src = self.load('url = "https://example.org/f.zip"\nfile = "f.zip"\nsize = 3\nsha256 = "ff"\n')["book"]
        self.assertFalse(src.multipart)
        self.assertEqual(src.part().url, "https://example.org/f.zip")


def portrait(key: str, figure: str, quad: str = "TR", stand_in: bool = False, mirror: bool = False) -> Portrait:
    return Portrait(key, figure, 16, quad, (100, 100), 300, mirror, stand_in)


INDEX = {16: {"TR": "丁原", "TL": "丁管", "BR": "伍孚", "BL": "董卓"}}


class PortraitTableTest(unittest.TestCase):
    def check(self, entries: list[Portrait], keys: set[str], reserved: frozenset[str] = frozenset()) -> list[str]:
        return check_table(entries, INDEX, set(reserved), keys)

    def test_box_is_4_by_5_with_the_face_40_percent_down(self) -> None:
        box = portrait("a", "丁原").box
        w, h = box[2] - box[0], box[3] - box[1]
        self.assertEqual(w * SIZE[1], h * SIZE[0])
        self.assertEqual(box[0] + w // 2, 100)
        self.assertEqual(box[1] + h * 2 // 5, 100)

    def test_valid_table_passes(self) -> None:
        unused = self.check(
            [
                portrait("a", "丁原"),
                portrait("b", "伍孚", "BR", stand_in=True),
                portrait("c", "伍孚", "BR", True, True),
            ],
            {"a", "b", "c"},
        )
        self.assertEqual(unused, [])

    def test_entries_no_officer_uses_are_listed(self) -> None:
        # a key for `@show` or for an officer a later chapter adds: accepted, but reported
        self.assertEqual(self.check([portrait("a", "丁原"), portrait("x", "丁管", "TL")], {"a"}), ["x"])

    def test_rejections(self) -> None:
        cases = {
            "missing officer": ([portrait("a", "丁原")], {"a", "b"}),
            "wrong figure": ([portrait("a", "董卓")], {"a"}),
            "second use not mirrored": (
                [portrait("a", "伍孚", "BR", True), portrait("b", "伍孚", "BR", True)],
                {"a", "b"},
            ),
            "third use": (
                [
                    portrait("a", "伍孚", "BR", True),
                    portrait("b", "伍孚", "BR", True, True),
                    portrait("c", "伍孚", "BR", True, True),
                ],
                {"a", "b", "c"},
            ),
            "own figure reused": (
                [portrait("a", "丁原"), portrait("b", "丁原", stand_in=True, mirror=True)],
                {"a", "b"},
            ),
        }
        for name, (entries, keys) in cases.items():
            with self.subTest(name), self.assertRaises(SourceError):
                self.check(entries, keys)
        with self.assertRaises(SourceError):
            self.check([portrait("a", "董卓", "BL", stand_in=True)], {"a"}, frozenset({"董卓"}))


class GradeTest(unittest.TestCase):
    def test_tone_curve_is_monotonic_and_caps_highlights(self) -> None:
        for gamma in (0.5, 1.0, 2.0):
            curve = _tone_curve(gamma)
            self.assertEqual(len(curve), 256)
            self.assertEqual(curve[0], 0)
            self.assertTrue(all(a <= b for a, b in zip(curve, curve[1:], strict=False)))
            self.assertLessEqual(curve[255], round(255 * HIGHLIGHT_MAX))


class MusicTest(unittest.TestCase):
    def test_filters_trim_then_fade_relative_to_the_cut(self) -> None:
        f = _filters(Track("k", "s", 10.0, 40.0, fade_in=0.5, fade_out=2.0))
        self.assertEqual(
            f,
            "atrim=start=10.0:end=40.0,asetpts=PTS-STARTPTS,afade=t=in:st=0:d=0.5,afade=t=out:st=28.000000:d=2.0",
        )
        self.assertNotIn("afade=t=in", _filters(Track("k", "s", 0.0, 5.0)))


class FontTextTest(unittest.TestCase):
    def test_pack_hanja_reads_pack_text_only(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            write(root, "officers.toml", 'hanja = "劉備"\nname = "유비"\n')
            write(root, "dramas/a.drama", "@say 關羽: 兄長\n")
            write(root, "credits.txt", "「漢宮春曉圖」\n")
            write(root, "fonts/OFL-FusionPixel.txt", "此字型是免費的\n")  # licence text: ignored
            chars = pack_hanja(root)
        self.assertTrue(set("劉備關羽兄長漢宮春曉圖") <= chars)
        self.assertTrue(set(EXTRA_HANJA) <= chars)
        self.assertFalse(set("此字型是免費的") & (chars - set(EXTRA_HANJA)))
        self.assertNotIn("유", chars)


def glyph_pixels(filled: set[tuple[int, int]]) -> set[tuple[int, int]]:
    """Rasterise pixel_glyph(filled) back to pixels."""
    coords, ends, _ = pixel_glyph(filled).getCoordinates(None)
    contours, start = [], 0
    for end in ends:
        contours.append([tuple(p) for p in coords[start : end + 1]])
        start = end + 1
    return pixels(contours)


class PixelGlyphTest(unittest.TestCase):
    def test_round_trip_keeps_holes_and_diagonal_contacts(self) -> None:
        ring = {(c, r) for c in range(3) for r in range(3)} - {(1, 1)}  # a hole in the middle
        diagonal = {(5, 0), (6, 1), (7, 0)}  # pixels touching only at corners
        for filled in (ring, diagonal, ring | diagonal, {(0, 0)}):
            with self.subTest(sorted(filled)):
                self.assertEqual(glyph_pixels(filled), filled)

    def test_outline_is_the_union_boundary(self) -> None:
        loops = outline({(0, 0), (1, 0), (0, 1), (1, 1)})
        self.assertEqual(loops, [[(0, 0), (0, 2), (2, 2), (2, 0)]])  # one clockwise square
        ring = outline({(c, r) for c in range(3) for r in range(3)} - {(1, 1)})
        self.assertEqual(len(ring), 2)  # the outer edge and the hole
        self.assertEqual(len(outline({(0, 0), (1, 1)})), 2)  # corner contact: two separate loops


if __name__ == "__main__":
    unittest.main()
