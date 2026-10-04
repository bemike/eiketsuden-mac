"""Check both game fonts against current UI code and the actual packaged Chinese data.

Usage: python tools/assets/verify_chinese_fonts.py --data-root /path/to/Resources/data
Requires fontTools. The check includes source comments, a conservative superset of UI text.
"""
from pathlib import Path
import argparse
import json
import re
from fontTools.ttLib import TTFont


def audit(data_root: Path) -> dict:
    repo = Path(__file__).resolve().parents[2]
    required = set("傕彧昱汜蟬褚郃龐豨")
    cjk = re.compile(r"[\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff]")
    for root in (data_root, repo / "crates/hero-game/src", repo / "crates/hero-core/src"):
        for path in root.rglob("*"):
            if path.is_file() and (path.suffix in {".toml", ".drama", ".rs"} or path.name == "credits.txt"):
                required.update(cjk.findall(path.read_text(encoding="utf-8")))
    result = {"required_cjk": len(required), "fonts": {}}
    for name in ("Galmuri11.ttf", "Galmuri9.ttf"):
        with TTFont(data_root / "base/fonts" / name) as font:
            cmap = font.getBestCmap()
            missing = "".join(sorted(c for c in required if ord(c) not in cmap))
            empty = "".join(sorted(c for c in required if ord(c) in cmap and font["glyf"][cmap[ord(c)]].numberOfContours == 0))
            result["fonts"][name] = {"missing": missing, "empty_outlines": empty}
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-root", type=Path, required=True)
    args = parser.parse_args()
    report = audit(args.data_root)
    print(json.dumps(report, ensure_ascii=False, indent=2))
    if any(f["missing"] or f["empty_outlines"] for f in report["fonts"].values()):
        raise SystemExit(1)
