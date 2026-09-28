"""Tests of tools/web/assemble.py: the site layout and layered packs."""

from __future__ import annotations

import importlib.util
import shutil
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))

import assemble

HAS_FONTTOOLS = importlib.util.find_spec("fontTools") is not None


def pack(root: Path, name: str, extends: str | None = None) -> Path:
    d = root / name
    d.mkdir(parents=True)
    text = f'id = "{name}"\n'
    if extends is not None:
        text += f'extends = "{extends}"\n'
    (d / "pack.toml").write_text(text, encoding="utf-8")
    return d


class AssembleTest(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.wasm = self.root / "eiketsuden.wasm"
        self.wasm.write_bytes(b"\0asm")

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def test_site_layout_and_parents(self) -> None:
        packs = self.root / "packs"
        pack(packs, "vanilla")
        child = pack(packs, "balance", "../vanilla")
        out = self.root / "site"
        self.assertEqual(assemble.assemble(self.wasm, out, child), [])
        for name in (*assemble.WEB_FILES, "eiketsuden.wasm", ".nojekyll", assemble.MARKER):
            self.assertTrue((out / name).is_file(), name)
        self.assertIn("balance", (out / "data/base/pack.toml").read_text(encoding="utf-8"))
        self.assertIn("vanilla", (out / "data/vanilla/pack.toml").read_text(encoding="utf-8"))
        # A second run replaces its own output.
        assemble.assemble(self.wasm, out, child)

    def test_refuses_a_folder_it_did_not_create(self) -> None:
        out = self.root / "mine"
        out.mkdir()
        (out / "keep.txt").write_text("x", encoding="utf-8")
        with self.assertRaises(assemble.AssembleError):
            assemble.assemble(self.wasm, out, pack(self.root, "base"))
        self.assertTrue((out / "keep.txt").exists())

    def test_a_parent_named_base_is_the_child_itself_on_the_web(self) -> None:
        packs = self.root / "data"
        pack(packs, "base")
        child = pack(packs, "original", "../base")
        with self.assertRaisesRegex(assemble.AssembleError, "points back at data/base/"):
            assemble.chain(child)

    def test_parents_must_stay_in_data_and_exist(self) -> None:
        with self.assertRaisesRegex(assemble.AssembleError, "leaves the site's data/"):
            assemble.chain(pack(self.root / "a", "child", "../../elsewhere"))
        with self.assertRaisesRegex(assemble.AssembleError, "has no pack.toml"):
            assemble.chain(pack(self.root / "b", "child", "../missing"))

    def test_a_parent_inside_its_child_is_copied_with_it(self) -> None:
        child = pack(self.root, "mod", "core")
        pack(child, "core")
        out = self.root / "site"
        assemble.assemble(self.wasm, out, child)
        self.assertIn("core", (out / "data/base/core/pack.toml").read_text(encoding="utf-8"))

    def test_extends_resolved_from_the_site_root(self) -> None:
        # mods/balance extends ../../data/vanilla: on the web data/base + ../../data/vanilla is
        # data/vanilla, where the parent is copied.
        pack(self.root / "data", "vanilla")
        child = pack(self.root / "mods", "balance", "../../data/vanilla")
        self.assertEqual(
            [site for _, site in assemble.chain(child)],
            ["data/base", "data/vanilla"],
        )
        with self.assertRaisesRegex(assemble.AssembleError, "extends must be a relative path"):
            assemble.chain(pack(self.root / "c", "child", "C:/elsewhere"))

    def test_packs_that_cannot_be_laid_out(self) -> None:
        # The parent's site path lies inside the child's, but on disk it is somewhere else.
        pack(self.root / "p", "sub")
        child = pack(self.root / "x", "child", "sub")
        with self.assertRaisesRegex(assemble.AssembleError, "cannot lay these packs out"):
            assemble.copies([(child, "data/base"), (self.root / "p/sub", "data/base/sub")])
        # The output inside the pack it copies.
        top = pack(self.root, "top")
        with self.assertRaisesRegex(assemble.AssembleError, "lies inside the pack"):
            assemble.assemble(self.wasm, top / "site", top)

    def test_chain_depth(self) -> None:
        packs = self.root / "deep"
        pack(packs, "p4")
        pack(packs, "p3", "../p4")
        pack(packs, "p2", "../p3")
        pack(packs, "p1", "../p2")
        top = pack(packs, "p0", "../p1")
        with self.assertRaisesRegex(assemble.AssembleError, "at most 4 packs"):
            assemble.chain(top)

    def test_missing_pack_and_wasm(self) -> None:
        warnings = assemble.assemble(self.wasm, self.root / "s1", self.root / "nothing")
        self.assertIn("not found", warnings[0])
        with self.assertRaises(assemble.AssembleError):
            assemble.assemble(self.root / "no.wasm", self.root / "s2", self.root / "nothing")

    @unittest.skipUnless(HAS_FONTTOOLS, "needs fontTools")
    def test_fonts_keep_only_the_hanja_the_game_shows(self) -> None:
        from fontTools.ttLib import TTFont

        base = pack(self.root, "base")
        (base / "fonts").mkdir()
        for font in assemble.FONT_FILES:
            shutil.copy2(assemble.ROOT / "data/base" / font, base / font)
        cmap = TTFont(base / assemble.FONT_FILES[0]).getBestCmap()
        liu, bei, guan, zhang = (ord(c) for c in "劉備關張")
        self.assertTrue(all(c in cmap for c in (liu, bei, guan, zhang)))
        (base / "officers.toml").write_text('name = "유비"\nhanja = "劉備"\n', encoding="utf-8")
        (base / "credits.txt").write_text("關\n", encoding="utf-8")
        # The game's own strings.
        self.wasm.write_bytes(b"\0asm\x01" + "영걸전 張".encode() + b"\xff")
        out = self.root / "site"
        self.assertEqual(assemble.assemble(self.wasm, out, base), [])
        for font in assemble.FONT_FILES:
            kept = TTFont(out / "data/base" / font).getBestCmap()
            hanja = {c for c in kept if assemble.is_hanja(c)}
            self.assertEqual(hanja, {liu, bei, guan, zhang}, font)
            # Every other character stays.
            original = TTFont(base / font).getBestCmap()
            self.assertEqual(
                {c for c in kept if not assemble.is_hanja(c)},
                {c for c in original if not assemble.is_hanja(c)},
            )
            self.assertLess((out / "data/base" / font).stat().st_size, (base / font).stat().st_size)
            self.assertIn("ER", TTFont(out / "data/base" / font)["name"].getBestFamilyName())

    def test_without_fonttools_the_fonts_are_copied_whole(self) -> None:
        base = pack(self.root, "base")
        (base / "fonts").mkdir()
        (base / assemble.FONT_FILES[0]).write_bytes(b"font")
        with mock.patch.dict(sys.modules, {"fontTools": None}):
            warnings = assemble.assemble(self.wasm, self.root / "s1", base)
            self.assertIn("pip install fonttools", warnings[0])
            self.assertEqual((self.root / "s1/data/base" / assemble.FONT_FILES[0]).read_bytes(), b"font")
            with self.assertRaisesRegex(assemble.AssembleError, "pip install fonttools"):
                assemble.assemble(self.wasm, self.root / "s2", base, require_font_subset=True)


if __name__ == "__main__":
    unittest.main()
