"""Add explicitly requested glyphs to the existing game fonts, preserving existing artwork.

Supply local Fusion Pixel v2026.09.01 donor ZIPs (OFL), whose download URLs are in
HAN_CAMP_AND_CHINESE_FONTS.md. This command never downloads files or rebuilds other assets.
"""
from pathlib import Path
from dataclasses import replace
import argparse
import hashlib
import zipfile
import build_fonts

ROOT = Path(__file__).resolve().parents[2]
EXPECTED = {
    12: "c3e2c4cadac184a5b8e3e2e48e428c5740397db76f80b5879ae8720ad6db6b0a",
    10: "8cc700b05fcf9a8639a60e6287db973e4d1d48864f2cc3cbc77d0542419ee269",
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--characters", required=True)
    parser.add_argument("--donor-12", type=Path, required=True)
    parser.add_argument("--donor-10", type=Path, required=True)
    args = parser.parse_args()
    for job, size, archive in zip(build_fonts.FONTS, (12, 10), (args.donor_12, args.donor_10)):
        if hashlib.sha256(archive.read_bytes()).hexdigest() != EXPECTED[size]:
            raise ValueError(f"Unexpected donor archive: {archive}")
        with zipfile.ZipFile(archive) as source:
            donor = source.read(f"fusion-pixel-{size}px-proportional-zh_hant.ttf.woff2")
        path = ROOT / "data/base" / job.out
        merged = build_fonts.merge(path.read_bytes(), donor, replace(job, family=job.family + " Chinese"), set(args.characters) | set("中文漢字天地日月山水火風林雨"))
        path.write_bytes(merged.data)
        print(f"{path.name}: added {len(merged.copied)} glyphs")


if __name__ == "__main__":
    main()
