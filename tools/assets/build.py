#!/usr/bin/env python3
"""Build the base pack's media (tiles, units, FX, UI, SFX, fonts) from the cached sources.

Run tools/assets/fetch.py first. The build is deterministic: the same sources always give
byte-identical files, so `--check` can verify that the committed outputs are up to date.

Usage:
    python tools/assets/build.py                 # everything into data/base
    python tools/assets/build.py terrain units   # selected steps
    python tools/assets/build.py --check         # rebuild into a temp dir and compare
"""

from __future__ import annotations

import argparse
import sys
import tempfile
from collections.abc import Callable
from pathlib import Path

from assetlib import PACK_DIR, Sources, SourceError
from build_audio import build_fonts, build_sfx
from build_fx import build_fx

Step = Callable[[Sources, Path], list[str]]

STEPS: dict[str, Step] = {
    "fonts": build_fonts,
    "sfx": build_sfx,
    "fx": build_fx,
}


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
    steps = args.steps or list(STEPS)
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
