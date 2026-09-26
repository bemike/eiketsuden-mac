#!/usr/bin/env python3
"""Build the base pack's media from the cached sources.

Steps: tiles, units, FX, UI, title art, portraits, drama backgrounds, music, SFX and fonts.

Run tools/assets/fetch.py first. The build is deterministic: the same sources always give
byte-identical files, so `--check` can verify that the committed outputs are up to date.

Usage:
    python tools/assets/build.py                 # everything into data/base
    python tools/assets/build.py terrain units   # selected steps (plus the steps they depend on)
    python tools/assets/build.py --check         # rebuild into a temp dir and compare
"""

from __future__ import annotations

import argparse
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

from assetlib import PACK_DIR, SourceError, Sources
from build_audio import build_sfx
from build_backgrounds import build_backgrounds
from build_fonts import build_fonts
from build_fx import build_fx
from build_music import build_music
from build_portraits import build_portraits
from build_terrain import build_terrain
from build_title import build_title
from build_ui import build_flags, build_icons
from build_units import build_units

Step = Callable[[Sources, Path], list[str]]

STEPS: dict[str, Step] = {
    "terrain": build_terrain,
    "units": build_units,
    "fonts": build_fonts,
    "sfx": build_sfx,
    "fx": build_fx,
    "icons": build_icons,
    "flags": build_flags,
    "title": build_title,
    "portraits": build_portraits,
    "backgrounds": build_backgrounds,
    "music": build_music,
}

# Steps that read other steps' outputs from the pack directory.
DEPENDS: dict[str, list[str]] = {"title": ["terrain", "units", "flags"]}


def resolve(steps: list[str]) -> list[str]:
    """The requested steps and their dependencies, in the canonical STEPS order."""
    wanted = set(steps)
    for name in steps:
        wanted.update(DEPENDS.get(name, []))
    return [name for name in STEPS if name in wanted]


def run(steps: list[str], out: Path) -> list[str]:
    src = Sources()
    src.verify()
    written: list[str] = []
    try:
        for name in steps:
            files = STEPS[name](src, out)
            print(f"{name}: {len(files)} file(s)")
            written.extend(files)
    finally:
        src.close()
    if set(steps) == set(STEPS):
        # sources.toml, fetch.py and CREDITS.md must only list what the pack really uses
        unused = src.unused()
        if unused:
            raise SourceError(f"sources pinned in sources.toml but not used by any step: {', '.join(unused)}")
    return written


def check(steps: list[str]) -> int:
    with tempfile.TemporaryDirectory(prefix="hero-assets-") as tmp:
        written = run(steps, Path(tmp))
        stale = []
        for rel in written:
            new = (Path(tmp) / rel).read_bytes()
            old_path = PACK_DIR / rel
            if not old_path.exists() or old_path.read_bytes() != new:
                stale.append(rel)
    if stale:
        print("out of date (run tools/assets/build.py):", *stale, sep="\n  ", file=sys.stderr)
        return 1
    print(f"all {len(written)} generated files are up to date")
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("steps", nargs="*", help=f"steps to run (default: all): {', '.join(STEPS)}")
    parser.add_argument("--out", type=Path, default=PACK_DIR, help="pack directory to write (default: data/base)")
    parser.add_argument("--check", action="store_true", help="verify committed outputs instead of writing them")
    args = parser.parse_args(argv)
    unknown = [s for s in args.steps if s not in STEPS]
    if unknown:
        parser.error(f"unknown step(s) {', '.join(unknown)}; choose from {', '.join(STEPS)}")
    steps = resolve(args.steps) if args.steps else list(STEPS)
    try:
        if args.check:
            return check(steps)
        run(steps, args.out)
    except SourceError as e:
        print(f"ERROR {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
