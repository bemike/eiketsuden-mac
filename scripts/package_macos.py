#!/usr/bin/env python3
"""Package the native engine plus an explicitly supplied converted game data pack.

Run on macOS. No original game installation, saves or private settings are read implicitly.
The full-game result contains proprietary original assets; keep it in the private release.
"""
from pathlib import Path
import argparse
import hashlib
import json
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile


REPO = Path(__file__).resolve().parents[1]


def run(*args):
    subprocess.run([str(arg) for arg in args], check=True)


def package(binary, original, output):
    version = (REPO / "VERSION").read_text().strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError("VERSION must be a semantic version")
    if sys.platform != "darwin":
        raise RuntimeError("Packaging requires macOS: sips, iconutil and codesign")
    if "arm64" not in subprocess.check_output(["file", str(binary)], text=True):
        raise ValueError("The executable must contain Apple Silicon arm64 code")
    output.mkdir(parents=True, exist_ok=True)
    app_name = f"三国志英杰传-{version}.app"
    archive = output / f"eiketsuden-mac-{version}-arm64.zip"
    if archive.exists():
        raise FileExistsError("Output already exists; choose a new output directory")
    with tempfile.TemporaryDirectory(prefix="eiketsuden-package-") as temporary:
        stage = Path(temporary)
        app = stage / app_name
        contents = app / "Contents"
        resources = contents / "Resources"
        macos = contents / "MacOS"
        macos.mkdir(parents=True)
        resources.mkdir()
        shutil.copyfile(binary, macos / "eiketsuden")
        (macos / "eiketsuden").chmod(0o755)
        shutil.copytree(REPO / "data/base", resources / "data/base",
                        ignore=shutil.ignore_patterns(".DS_Store", "* 2.*"))
        index = json.loads((original / "original-pack.json").read_text())
        destination = resources / "data/original"
        for name in [*index["files"], "original-pack.json"]:
            relative = Path(name)
            if relative.is_absolute() or ".." in relative.parts:
                raise ValueError(f"Unsafe manifest path: {name}")
            source = (original / relative).resolve()
            if not source.is_relative_to(original.resolve()):
                raise ValueError(f"Manifest path escapes pack: {name}")
            target = destination / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
        phrases = {
            "유비전": "刘备传", "출진 준비": "出战准备",
            "영걸전 원작 모드": "三国志英杰传 · 中文原版数据",
            "사수관 전투": "汜水关之战", "낙양 동쪽 사수관": "洛阳东侧汜水关",
            "적장 화웅을 물리쳐라": "击败华雄", "게임 오버": "游戏结束",
            "장 완료": "章完成", "엔딩": "结局", "전투": "之战",
            "에게 간다": "（前往）", "의 뜻을 따른다": "（听从建议）",
            "아니오": "否", "예": "是", "년": "年",
        }
        for path in destination.rglob("*"):
            if path.suffix not in {".toml", ".drama"}:
                continue
            text = path.read_text()
            for old, new in phrases.items():
                text = text.replace(old, new)
            if path.name == "pack.toml":
                text = re.sub(r'^extends = .*$', 'extends = "../base"', text, flags=re.M)
                text = re.sub(r'^description = .*$',
                              'description = "中文 DOS 原版资源与剧情的原生引擎转换版本。"',
                              text, flags=re.M)
            path.write_text(text.replace("제 ", "第 "))
        index["extends"] = "../base"
        (destination / "original-pack.json").write_text(
            json.dumps(index, ensure_ascii=False, indent=2))
        iconset = stage / "AppIcon.iconset"
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                suffix = "@2x" if scale == 2 else ""
                target = iconset / f"icon_{size}x{size}{suffix}.png"
                run("sips", "-z", size * scale, size * scale,
                    REPO / "crates/hero-game/icon/original_icon_rounded.png", "--out", target)
        run("iconutil", "-c", "icns", iconset, "-o", resources / "AppIcon.icns")
        info = {
            "CFBundleName": "三国志英杰传", "CFBundleDisplayName": "三国志英杰传",
            "CFBundleIdentifier": "local.bemike.eiketsuden-native.classic-rounded",
            "CFBundleExecutable": "eiketsuden", "CFBundlePackageType": "APPL",
            "CFBundleShortVersionString": version, "CFBundleVersion": "10",
            "LSMinimumSystemVersion": "12.0", "LSArchitecturePriority": ["arm64"],
            "NSHighResolutionCapable": True, "CFBundleIconFile": "AppIcon.icns",
            "NSHumanReadableCopyright": "Non-official GPL engine adaptation. Original assets belong to their rights holders.",
        }
        (contents / "Info.plist").write_bytes(plistlib.dumps(info))
        for name in ("LICENSE", "CREDITS.md"):
            shutil.copyfile(REPO / name, resources / name)
        for name in ("ORIGINAL_TITLE_SOURCES.md", "HAN_CAMP_AND_CHINESE_FONTS.md"):
            shutil.copyfile(REPO / "docs" / name, resources / name)
        run(sys.executable, REPO / "tools/assets/verify_chinese_fonts.py",
            "--data-root", resources / "data")
        run("xattr", "-cr", app)
        run("codesign", "--force", "--sign", "-", app)
        run("codesign", "--verify", "--strict", app)
        # Archive the verified staging bundle directly. Copying a loose .app into an
        # iCloud-synced output folder can immediately attach Finder metadata that
        # fails strict signature checks; ZIP never serializes those attributes.
        with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as packed:
            for path in sorted(app.rglob("*")):
                if path.is_file():
                    packed.write(path, path.relative_to(stage))
        with zipfile.ZipFile(archive) as packed:
            if packed.testzip() is not None:
                raise RuntimeError("ZIP CRC verification failed")
        with archive.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
        archive.with_suffix(".zip.sha256").write_text(f"{digest}  {archive.name}\n")
        print(archive)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--original-pack", type=Path, required=True,
                        help="Explicit converted pack, e.g. the existing app's Resources/data/original")
    parser.add_argument("--output", type=Path, default=REPO / "dist")
    args = parser.parse_args()
    package(args.binary.resolve(), args.original_pack.resolve(), args.output.resolve())
