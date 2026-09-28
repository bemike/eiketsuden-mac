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
import os
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
    # The same rule as hero_core's pack.toml check: a relative path with '/'.
    if not isinstance(value, str) or not value or value.startswith("/") or "\\" in value or ":" in value:
        raise AssembleError(f"{manifest}: extends must be a relative path with '/', got {value!r}")
    return value


def chain(top: Path) -> list[tuple[Path, str]]:
    """The packs of the chain: `(source dir, site path)`, the top pack first.

    Sources are resolved lexically from the top pack, like hero_core does. Site paths are
    relative to the site root: the top pack is `data/base`, and a parent is its child's site path
    joined with the child's `extends`, resolved lexically like the browser resolves the URL. A
    parent must stay below `data/` and must not be a pack already placed (a parent at data/base
    would be the child itself).
    """
    packs = [(Path(os.path.normpath(top)), "data/base")]
    while True:
        source, site = packs[-1]
        rel = extends_of(source)
        if rel is None:
            return packs
        if len(packs) == MAX_CHAIN:
            raise AssembleError(f"{top}: a chain holds at most {MAX_CHAIN} packs")
        parent_source = Path(os.path.normpath(source / rel))
        parent_site = posixpath.normpath(posixpath.join(site, rel))
        manifest = source / "pack.toml"
        if parent_site != "data" and not parent_site.startswith("data/"):
            raise AssembleError(
                f"{manifest}: extends = {rel!r} leaves the site's data/ folder (from {site}/): "
                "the web build loads every pack below data/"
            )
        if any(parent_site == placed for _, placed in packs):
            raise AssembleError(
                f"{manifest}: extends = {rel!r} points back at {parent_site}/ on the web, where the "
                "top pack always sits in data/base/; keep the parent in a sibling directory with "
                "another name (docs/MODDING.md)"
            )
        if not (parent_source / "pack.toml").is_file():
            raise AssembleError(f"{manifest}: extends {parent_source}, which has no pack.toml")
        packs.append((parent_source, parent_site))


def inside(path: str, parent: str) -> bool:
    """`path` is `parent` or below it (site paths, '/'-separated)."""
    return path == parent or path.startswith(parent + "/")


def copies(packs: list[tuple[Path, str]]) -> list[tuple[Path, str]]:
    """The directory copies that put every pack in place.

    A pack whose site path lies inside another pack's (a parent in a subfolder of its child, or a
    child inside its parent) is already copied with it when the two sit the same way on disk;
    otherwise the layout cannot be built.
    """
    result = []
    for source, site in sorted(packs, key=lambda p: p[1].count("/")):
        outer = next(((s, t) for s, t in result if inside(site, t)), None)
        if outer is None:
            result.append((source, site))
            continue
        outer_source, outer_site = outer
        want = Path(os.path.normpath(outer_source / posixpath.relpath(site, outer_site)))
        if want != source:
            raise AssembleError(
                f"{source} would go to {site}/, inside {outer_site}/ ({outer_source}), where "
                f"{want} is: the web build cannot lay these packs out"
            )
    return result


def assemble(wasm: Path, out: Path, data: Path) -> list[str]:
    """Build the site in `out`; returns warnings."""
    warnings = []
    if not wasm.is_file():
        raise AssembleError(f"{wasm} not found: build hero-game for wasm32-unknown-unknown first")
    plan: list[tuple[Path, str]] = []
    if data.is_dir():
        if not (data / "pack.toml").is_file():
            warnings.append(f"{data} has no pack.toml: only the UI gallery (#gallery) will work")
        plan = copies(chain(data))
        for source, _ in plan:
            if out == source or source in out.parents:
                raise AssembleError(f"the output {out} lies inside the pack {source}")
    else:
        warnings.append(f"data pack {data} not found: the page will show the 'pack not found' error screen")
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
    for source, site in plan:
        shutil.copytree(source, out / site, dirs_exist_ok=site == "data")
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
        warnings = assemble(
            Path(os.path.abspath(args.wasm)), Path(os.path.abspath(args.out)), Path(os.path.abspath(args.data))
        )
    except (AssembleError, OSError) as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    for w in warnings:
        print(f"warning: {w}", file=sys.stderr)
    size = (args.out / "eiketsuden.wasm").stat().st_size
    print(f"site ready in {args.out.resolve()} ({size:,} bytes of wasm)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
