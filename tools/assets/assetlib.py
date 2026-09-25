"""Shared helpers of the asset pipeline: source registry, archive access, image and TOML utilities."""

from __future__ import annotations

import io
import tomllib
import zipfile
from dataclasses import dataclass
from pathlib import Path

from PIL import Image

TOOLS_DIR = Path(__file__).resolve().parent
REPO_DIR = TOOLS_DIR.parent.parent
CACHE_DIR = TOOLS_DIR / ".cache"
PACK_DIR = REPO_DIR / "data" / "base"


@dataclass(frozen=True)
class Source:
    id: str
    title: str
    author: str
    license: str
    license_url: str
    homepage: str
    file: str
    size: int
    sha256: str
    url: str = ""
    itch_page: str = ""
    itch_upload: int = 0

    @property
    def path(self) -> Path:
        return CACHE_DIR / self.file


def load_sources(path: Path = TOOLS_DIR / "sources.toml") -> dict[str, Source]:
    with path.open("rb") as f:
        doc = tomllib.load(f)
    out: dict[str, Source] = {}
    for sid, entry in doc["sources"].items():
        itch = entry.get("itch", {})
        out[sid] = Source(
            id=sid,
            title=entry["title"],
            author=entry["author"],
            license=entry["license"],
            license_url=entry["license_url"],
            homepage=entry["homepage"],
            file=entry["file"],
            size=int(entry["size"]),
            sha256=entry["sha256"].lower(),
            url=entry.get("url", ""),
            itch_page=itch.get("page", ""),
            itch_upload=int(itch.get("upload", 0)),
        )
    return out
