"""Music (`bgm/<key>.ogg`): openly licensed tracks, trimmed, loudness-normalised and re-encoded.

Every track goes through ffmpeg (https://ffmpeg.org, must be on PATH or named by the FFMPEG
environment variable):

1. the part of the source listed in TRACKS is cut out (`start`/`end` in seconds of the decoded
   source, found by measuring silence and, for `enemy`, the bar grid of the loop section) with
   short fades against clicks;
2. two-pass EBU R128 loudness normalisation (`loudnorm`, linear mode) to LOUDNESS LUFS with a
   true-peak ceiling, so every track plays at the same perceived level;
3. Ogg Vorbis at 44.1 kHz stereo, VBR quality QUALITY (about 96 kbit/s). 44.1 kHz matters: the
   native audio backend resamples anything else with nearest-neighbour.

The engine loops BGM from the file's start to its end, so loop tracks start on the first note and
end where the music ends; `victory` and `defeat` are jingles played once.

Encoding with `-fflags +bitexact` makes the output reproducible for a given ffmpeg/libvorbis
build; `build.py --check` therefore needs the same ffmpeg release (see README.md).
"""

from __future__ import annotations

import json
import os
import re
import shutil
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path

from assetlib import SourceError, Sources

LOUDNESS = -16.0  # integrated loudness target, LUFS
TRUE_PEAK = -2.0  # dBTP ceiling before encoding (Vorbis adds up to ~1 dB of overshoot)
LRA = 11.0  # loudness range target (only used by loudnorm's fallback dynamic mode)
QUALITY = 2  # libvorbis -q:a (VBR, ~96 kbit/s stereo)
RATE = 44100


@dataclass(frozen=True)
class Track:
    key: str
    source: str
    start: float
    end: float
    fade_in: float = 0.0
    fade_out: float = 0.3
    member: str = ""  # path inside a zip source
    note: str = ""


TRACKS: list[Track] = [
    Track("title", "hitctrl_jade_throne", 0.0, 236.5, fade_out=0.5, note="whole piece"),
    Track("peace", "macleod_shenyang", 0.0, 149.2, fade_out=0.5, note="whole piece"),
    Track("tension", "macleod_asian_drums", 0.20, 128.8, fade_in=0.02, fade_out=0.5, note="whole piece"),
    Track("sad", "macleod_nu_flute", 1.0, 80.4, fade_in=0.02, fade_out=0.5, note="whole piece"),
    Track("camp", "macleod_ishikari_lore", 0.0, 164.0, fade_out=1.0, note="whole piece"),
    Track("battle", "macleod_mountain_emperor", 0.0, 197.3, fade_out=0.5, note="whole piece"),
    # The full-band section of the demo mix: six 16-beat bars at 140 BPM (41.143 s) starting on
    # the downbeat at 20.636 s, so the file loops on the bar grid; the quiet intro and the outro
    # hits are left out.
    Track(
        "enemy",
        "majadroid_samurai_nights",
        20.620,
        61.763,
        fade_in=0.005,
        fade_out=0.02,
        member="Samurai-Nights-Demo.mp3",
        note="loop section of the demo mix",
    ),
    Track("boss", "macleod_five_armies", 0.0, 152.4, fade_out=0.3, note="whole piece"),
    # The second of the ten fanfares (C major, the fullest brass).
    Track("victory", "springspring_fanfares", 6.84, 13.62, fade_in=0.005, fade_out=0.3, note="fanfare 2 of 10"),
    Track("defeat", "joth_death_of_a_ninja", 0.0, 16.07, fade_out=0.2, note="whole piece"),
    # The piece stops abruptly (as its author notes); a fade gives the ending roll a close.
    Track("ending", "springspring_asian_arrangement", 0.0, 115.2, fade_out=3.0, note="whole piece, faded out"),
]


def ffmpeg() -> str:
    exe = os.environ.get("FFMPEG") or shutil.which("ffmpeg")
    if not exe:
        raise SourceError("ffmpeg not found: install it (https://ffmpeg.org) or set FFMPEG to its path")
    return exe


def _run(args: list[str]) -> subprocess.CompletedProcess[str]:
    proc = subprocess.run(args, capture_output=True, text=True, encoding="utf-8", errors="replace")
    if proc.returncode != 0:
        tail = "\n".join(proc.stderr.strip().splitlines()[-15:])
        raise SourceError(f"ffmpeg failed ({' '.join(args[:3])} ...):\n{tail}")
    return proc


def _filters(t: Track) -> str:
    length = t.end - t.start
    chain = [f"atrim=start={t.start}:end={t.end}", "asetpts=PTS-STARTPTS"]
    if t.fade_in:
        chain.append(f"afade=t=in:st=0:d={t.fade_in}")
    if t.fade_out:
        chain.append(f"afade=t=out:st={length - t.fade_out:.6f}:d={t.fade_out}")
    return ",".join(chain)


def _measure(exe: str, source: Path, t: Track) -> dict[str, str]:
    """First loudnorm pass: the input's loudness statistics."""
    af = f"{_filters(t)},loudnorm=I={LOUDNESS}:TP={TRUE_PEAK}:LRA={LRA}:print_format=json"
    proc = _run([exe, "-hide_banner", "-nostdin", "-i", str(source), "-af", af, "-f", "null", "-"])
    m = re.search(r"\{[^{}]*\"input_i\"[^{}]*\}", proc.stderr)
    if not m:
        raise SourceError(f"bgm {t.key}: no loudnorm statistics in ffmpeg's output")
    return json.loads(m.group(0))


def _encode(exe: str, source: Path, t: Track, stats: dict[str, str], dest: Path) -> None:
    loudnorm = (
        f"loudnorm=I={LOUDNESS}:TP={TRUE_PEAK}:LRA={LRA}"
        f":measured_I={stats['input_i']}:measured_TP={stats['input_tp']}"
        f":measured_LRA={stats['input_lra']}:measured_thresh={stats['input_thresh']}"
        f":offset={stats['target_offset']}:linear=true"
    )
    af = f"{_filters(t)},{loudnorm},aresample={RATE}"
    _run(
        [
            exe,
            "-hide_banner",
            "-nostdin",
            "-y",
            "-i",
            str(source),
            "-af",
            af,
            "-map",
            "0:a:0",  # audio only: some MP3s carry cover art that would become a video stream
            "-map_metadata",
            "-1",
            "-ac",
            "2",
            "-ar",
            str(RATE),
            "-c:a",
            "libvorbis",
            "-q:a",
            str(QUALITY),
            "-fflags",
            "+bitexact",
            "-flags:a",
            "+bitexact",
            str(dest),
        ]
    )


def build_music(src: Sources, pack: Path) -> list[str]:
    exe = ffmpeg()
    out_dir = pack / "bgm"
    out_dir.mkdir(parents=True, exist_ok=True)
    written = []
    with tempfile.TemporaryDirectory(prefix="hero-bgm-") as tmp:
        for t in TRACKS:
            if t.member:
                source = Path(tmp) / f"{t.key}-source{Path(t.member).suffix}"
                source.write_bytes(src.member(t.source, t.member))
            else:
                source = src.path(t.source)
            if not t.end > t.start:
                raise SourceError(f"bgm {t.key}: end {t.end} is not after start {t.start}")
            dest = Path(tmp) / f"{t.key}.ogg"
            _encode(exe, source, t, _measure(exe, source, t), dest)
            shutil.copyfile(dest, out_dir / f"{t.key}.ogg")
            written.append(f"bgm/{t.key}.ogg")
    return written
