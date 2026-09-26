#!/usr/bin/env python3
"""Download the third-party archives listed in sources.toml into tools/assets/.cache/.

Every file is checked against the size and SHA-256 pinned in sources.toml; a mismatch aborts
with a non-zero exit status and the partial download is removed. Files already present in the
cache with the right hash are not downloaded again.

The Ninja Adventure pack has no stable download URL: itch.io hands out a short-lived signed URL
after a POST from the game page. If that flow ever breaks, download the zip manually from the
itch.io page and save it as .cache/<file> (see sources.toml); fetch.py then only verifies it.

Wikimedia's upload servers answer bursts of requests with HTTP 429; fetch.py waits as long as the
`Retry-After` header asks and retries, and pauses briefly between the files of a multi-part source.

Usage:
    python tools/assets/fetch.py            # every source
    python tools/assets/fetch.py toen       # only the named sources
"""

from __future__ import annotations

import argparse
import hashlib
import http.cookiejar
import json
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

from assetlib import Part, Source, load_sources

# Wikimedia asks automated clients to identify themselves with a contact URL.
USER_AGENT = "Mozilla/5.0 (compatible; eiketsuden-asset-fetch/1.1; +https://github.com/jeiel85/eiketsuden-reloaded)"
CHUNK = 1 << 20
RETRIES = 8  # attempts per file on HTTP 429/503 (rate limiting)
PART_PAUSE = 2.0  # seconds between the files of a multi-part source


class FetchError(Exception):
    """A source could not be downloaded or failed verification."""


def sha256_of(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(CHUNK), b""):
            digest.update(block)
    return digest.hexdigest()


def _opener() -> urllib.request.OpenerDirector:
    jar = http.cookiejar.CookieJar()
    return urllib.request.build_opener(urllib.request.HTTPCookieProcessor(jar))


def _request(url: str, data: bytes | None = None) -> urllib.request.Request:
    return urllib.request.Request(url, data=data, headers={"User-Agent": USER_AGENT})


def _itch_signed_url(opener: urllib.request.OpenerDirector, page: str, upload: int) -> str:
    """Resolve the temporary download URL of an itch.io upload ("name your own price", $0)."""
    with opener.open(_request(page), timeout=60) as resp:
        html = resp.read().decode("utf-8", "replace")
    m = re.search(r'<meta name="csrf_token" value="([^"]+)"', html)
    if not m:
        raise FetchError(f"{page}: csrf_token not found (itch.io page layout changed?)")
    form = urllib.parse.urlencode({"csrf_token": m.group(1)}).encode()
    endpoint = f"{page}/file/{upload}?source=view_game&as_props=1&after_download_lightbox=true"
    with opener.open(_request(endpoint, data=form), timeout=60) as resp:
        answer = json.loads(resp.read().decode("utf-8"))
    url = answer.get("url")
    if not isinstance(url, str) or not url.startswith("https://"):
        raise FetchError(f"{endpoint}: unexpected answer {answer!r}")
    return url


def _download(opener: urllib.request.OpenerDirector, url: str, dest: Path, size: int) -> None:
    for attempt in range(1, RETRIES + 1):
        try:
            _download_once(opener, url, dest, size)
            return
        except urllib.error.HTTPError as e:
            if e.code not in (429, 503) or attempt == RETRIES:
                raise
            wait = _retry_after(e.headers.get("Retry-After"), attempt)
            print(f"  HTTP {e.code}, retrying in {wait:.0f} s", flush=True)
            time.sleep(wait)


def _retry_after(header: str | None, attempt: int) -> float:
    """Seconds to wait before retrying: the server's Retry-After (in seconds) or a backoff."""
    try:
        return max(1.0, float(header)) + 1.0 if header else 5.0 * attempt
    except ValueError:  # an HTTP date instead of seconds
        return 5.0 * attempt


def _download_once(opener: urllib.request.OpenerDirector, url: str, dest: Path, size: int) -> None:
    progress = sys.stdout.isatty() and size >= 10 * CHUNK
    done = 0
    with opener.open(_request(url), timeout=120) as resp, dest.open("wb") as out:
        for block in iter(lambda: resp.read(CHUNK), b""):
            out.write(block)
            done += len(block)
            if progress:
                print(f"\r  {done / CHUNK:7.1f} / {size / CHUNK:.1f} MiB", end="", flush=True)
    if progress:
        print()


def verify(label: str, part: Part, path: Path) -> None:
    actual_size = path.stat().st_size
    if actual_size != part.size:
        raise FetchError(f"{label}: size {actual_size} != pinned {part.size}")
    actual = sha256_of(path)
    if not part.sha256:
        raise FetchError(f"{label}: no sha256 pinned in sources.toml; the downloaded file hashes to {actual}")
    if actual != part.sha256:
        raise FetchError(f"{label}: sha256 {actual} != pinned {part.sha256}")


def fetch(src: Source) -> None:
    opener = _opener()
    downloaded = 0
    for part in src.parts:
        label = f"{src.id}[{part.name}]" if part.name else src.id
        dest = part.path
        if dest.exists():
            verify(label, part, dest)
            continue
        dest.parent.mkdir(parents=True, exist_ok=True)
        if downloaded and src.multipart:
            time.sleep(PART_PAUSE)
        tmp = dest.with_name(dest.name + ".part")
        if src.itch_page:
            print(f"{label}: resolving itch.io download for upload {src.itch_upload}")
            url = _itch_signed_url(opener, src.itch_page, src.itch_upload)
        elif part.url:
            url = part.url
        else:
            raise FetchError(f"{label}: sources.toml gives neither url nor itch")
        print(f"{label}: downloading {part.size} bytes", flush=True)
        try:
            _download(opener, url, tmp, part.size)
            verify(label, part, tmp)
        except BaseException:
            tmp.unlink(missing_ok=True)
            raise
        tmp.replace(dest)
        downloaded += 1
    print(f"{src.id}: {downloaded} downloaded, {len(src.parts) - downloaded} cached; all verified")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("ids", nargs="*", help="source ids from sources.toml (default: all)")
    args = parser.parse_args(argv)
    sources = load_sources()
    wanted = args.ids or list(sources)
    unknown = [i for i in wanted if i not in sources]
    if unknown:
        print(f"unknown source id(s): {', '.join(unknown)}", file=sys.stderr)
        return 2
    failed = 0
    for sid in wanted:
        try:
            fetch(sources[sid])
        except (FetchError, OSError, ValueError) as e:  # URLError and HTTPError are OSErrors
            print(f"ERROR {e}", file=sys.stderr)
            failed += 1
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
