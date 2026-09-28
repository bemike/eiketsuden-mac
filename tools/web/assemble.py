#!/usr/bin/env python3
"""Assemble the web site from a built eiketsuden.wasm: the one place that knows the layout.

Used by tools/web/build.sh, tools/web/build.ps1 and the GitHub Pages workflow, so the three cannot
drift apart. The site holds index.html, mq_js_bundle.js, hero_web.js, eiketsuden.wasm, .nojekyll
and the data pack in data/base/ -- the browser always loads the top pack from there -- plus every
pack it extends, at the path its child's `extends` names (a sibling of data/base/, see
docs/MODDING.md "Layered packs in the web build").

usage: assemble.py --wasm <eiketsuden.wasm> --out <dir> [--data <pack dir>]

The output directory is only ever cleared if an earlier run created it (it holds a marker file).
Needs Python 3.11+ (tomllib).
"""

from __future__ import annotations

import argparse
import posixpath
import shutil
import sys
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:  # Python < 3.11
    sys.exit("tools/web/assemble.py needs Python 3.11 or newer (tomllib)")

ROOT = Path(__file__).resolve().parents[2]
WEB_FILES = ("index.html", "mq_js_bundle.js", "hero_web.js")
MARKER = ".eiketsuden-web-dist"
# The same limit as hero_core::pack::MAX_CHAIN_DEPTH: a pack and the packs it extends.
MAX_CHAIN = 4


class AssembleError(Exception):
    pass


def extends_of(pack: Path) -> str | None:
    """The `extends` of the pack.toml in `pack`, or None."""
    manifest = pack / "pack.toml"
    if not manifest.is_file():
        return None
    try:
        with manifest.open("rb") as f:
            value = tomllib.load(f).get("extends")
    except tomllib.TOMLDecodeError as e:
        raise AssembleError(f"{manifest}: {e}") from e
    if value is None:
        return None
    if not isinstance(value, str) or not value or value.startswith("/") or "\\" in value:
        raise AssembleError(f"{manifest}: extends must be a relative path with '/', got {value!r}")
    return value


def chain(top: Path) -> list[tuple[Path, str]]:
    """The packs to copy: `(source dir, site path below data/)`, the top pack first.

    A parent's site path is its child's site path joined with the child's `extends`, resolved
    lexically like a URL (the browser resolves it that way). It must stay inside data/ and must
    not land on a pack already placed (a parent at data/base would be the child itself).
    """
    packs = [(top, "base")]
    while True:
        source, site = packs[-1]
        rel = extends_of(source)
        if rel is None:
            return packs
        if len(packs) == MAX_CHAIN:
            raise AssembleError(f"{top}: a chain holds at most {MAX_CHAIN} packs")
        parent_source = (source / rel).resolve()
        parent_site = posixpath.normpath(posixpath.join(site, rel))
        if parent_site.startswith("..") or parent_site == ".":
            raise AssembleError(
                f"{source / 'pack.toml'}: extends = {rel!r} leaves the site's data/ folder (from data/{site}/)"
            )
        if any(parent_site == placed for _, placed in packs):
            raise AssembleError(
                f"{source / 'pack.toml'}: extends = {rel!r} points back at data/{parent_site}/ "
                "on the web, where the top pack always sits in data/base/; keep the parent in a "
                "sibling directory with another name (docs/MODDING.md)"
            )
        if not (parent_source / "pack.toml").is_file():
            raise AssembleError(f"{source / 'pack.toml'}: extends {parent_source}, which has no pack.toml")
        packs.append((parent_source, parent_site))


def assemble(wasm: Path, out: Path, data: Path) -> list[str]:
    """Build the site in `out`; returns warnings."""
    warnings = []
    if not wasm.is_file():
        raise AssembleError(f"{wasm} not found: build hero-game for wasm32-unknown-unknown first")
    if out.exists():
        if not (out / MARKER).exists():
            raise AssembleError(f"{out} exists but was not created by this script; refusing to overwrite it")
        shutil.rmtree(out)
    (out / "data").mkdir(parents=True)
    (out / MARKER).touch()
    (out / ".nojekyll").touch()
    for name in WEB_FILES:
        shutil.copy2(ROOT / "web" / name, out / name)
    shutil.copy2(wasm, out / "eiketsuden.wasm")

    if not data.is_dir():
        warnings.append(f"data pack {data} not found: the page will show the 'pack not found' error screen")
        return warnings
    if not (data / "pack.toml").is_file():
        warnings.append(f"{data} has no pack.toml: only the UI gallery (#gallery) will work")
    for source, site in chain(data):
        shutil.copytree(source, out / "data" / site)
    return warnings


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--wasm", type=Path, required=True, help="the built eiketsuden.wasm")
    parser.add_argument("--out", type=Path, required=True, help="output directory")
    parser.add_argument(
        "--data",
        type=Path,
        default=ROOT / "data" / "base",
        help="data pack copied to <out>/data/base (default: data/base of this repository)",
    )
    args = parser.parse_args(argv)
    try:
        warnings = assemble(args.wasm.resolve(), args.out.resolve(), args.data.resolve())
    except AssembleError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    for w in warnings:
        print(f"warning: {w}", file=sys.stderr)
    size = (args.out / "eiketsuden.wasm").stat().st_size
    print(f"site ready in {args.out.resolve()} ({size:,} bytes of wasm)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
