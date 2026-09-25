"""Sound effects (`sfx/<key>.wav`) and fonts (`fonts/`) of the base pack."""

from __future__ import annotations

import io
import struct
import wave
from dataclasses import dataclass
from pathlib import Path

from assetlib import Sources, write_text

SAMPLE_RATE = 44100


@dataclass(frozen=True)
class Sfx:
    key: str
    source: str  # path inside the Ninja Adventure pack
    max_seconds: float = 1.2


# Engine SFX keys (docs/ASSETS.md) -> Ninja Adventure sound. Chosen by name, length and pitch
# contour (e.g. the descending Menu11/Menu12 blips for losses); every file is CC0.
SFX: list[Sfx] = [
    Sfx("cursor", "Audio/Sounds/Menu/Move1.wav"),
    Sfx("confirm", "Audio/Sounds/Menu/Accept4.wav"),
    Sfx("cancel", "Audio/Sounds/Menu/Cancel.wav"),
    Sfx("error", "Audio/Sounds/Alert/Alert2.wav"),
    Sfx("step", "Audio/Sounds/Elemental/Grass.wav", max_seconds=0.2),
    Sfx("hit", "Audio/Sounds/Hit & Impact/Hit1.wav"),
    Sfx("hit_heavy", "Audio/Sounds/Hit & Impact/Hit2.wav"),
    Sfx("arrow", "Audio/Sounds/Whoosh & Slash/Whoosh2.wav"),
    Sfx("fire", "Audio/Sounds/Elemental/Fire.wav"),
    Sfx("water", "Audio/Sounds/Elemental/Water1.wav"),
    Sfx("rock", "Audio/Sounds/Elemental/Explosion2.wav"),
    Sfx("heal", "Audio/Sounds/Magic & Skill/Heal.wav"),
    Sfx("morale_up", "Audio/Sounds/Bonus/PowerUp1.wav"),
    Sfx("morale_down", "Audio/Sounds/Menu/Menu11.wav"),
    Sfx("confuse", "Audio/Sounds/Magic & Skill/Strange.wav"),
    Sfx("levelup", "Audio/Jingles/LevelUp1.wav", max_seconds=2.0),
    Sfx("retreat", "Audio/Sounds/Menu/Menu12.wav"),
    Sfx("treasure", "Audio/Sounds/Bonus/Gold1.wav"),
    Sfx("phase", "Audio/Sounds/Alert/Alert.wav"),
    Sfx("victory", "Audio/Jingles/Success2.wav", max_seconds=2.0),
    Sfx("defeat", "Audio/Jingles/GameOver2.wav", max_seconds=2.0),
]

TAIL_SECONDS = 0.04  # kept after the last audible sample
FADE_SECONDS = 0.015  # linear fade at the very end, avoids clicks when trimming
SILENCE_RATIO = 0.02  # "audible" = above 2 % of the peak


def _read_mono(data: bytes, name: str) -> list[int]:
    with wave.open(io.BytesIO(data)) as w:
        if w.getsampwidth() != 2 or w.getframerate() != SAMPLE_RATE or w.getcomptype() != "NONE":
            raise ValueError(f"{name}: expected 16-bit PCM at {SAMPLE_RATE} Hz")
        channels = w.getnchannels()
        raw = w.readframes(w.getnframes())
    samples = struct.unpack(f"<{len(raw) // 2}h", raw)
    if channels == 1:
        return list(samples)
    return [sum(samples[i : i + channels]) // channels for i in range(0, len(samples), channels)]


def _trim(samples: list[int], max_seconds: float) -> list[int]:
    peak = max((abs(s) for s in samples), default=0)
    if peak == 0:
        raise ValueError("silent sound")
    limit = peak * SILENCE_RATIO
    first = next(i for i, s in enumerate(samples) if abs(s) > limit)
    last = max(i for i, s in enumerate(samples) if abs(s) > limit)
    end = min(len(samples), last + int(TAIL_SECONDS * SAMPLE_RATE), first + int(max_seconds * SAMPLE_RATE))
    out = samples[first:end]
    fade = min(len(out), int(FADE_SECONDS * SAMPLE_RATE))
    for i in range(fade):
        j = len(out) - fade + i
        out[j] = out[j] * (fade - i) // fade
    return out


def _wav_bytes(samples: list[int]) -> bytes:
    buf = io.BytesIO()
    with wave.open(buf, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        w.writeframes(struct.pack(f"<{len(samples)}h", *samples))
    return buf.getvalue()


def build_sfx(src: Sources, pack: Path) -> list[str]:
    out_dir = pack / "sfx"
    written = []
    for sfx in SFX:
        mono = _read_mono(src.na_bytes(sfx.source), sfx.source)
        data = _wav_bytes(_trim(mono, sfx.max_seconds))
        path = out_dir / f"{sfx.key}.wav"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        written.append(f"sfx/{sfx.key}.wav")
    return written


FONTS = {
    "fonts/Galmuri11.ttf": "Galmuri11.ttf",
    "fonts/Galmuri9.ttf": "Galmuri9.ttf",
}


def build_fonts(src: Sources, pack: Path) -> list[str]:
    written = []
    for out, member in FONTS.items():
        path = pack / out
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(src.member("galmuri", member))
        written.append(out)
    licence = src.member("galmuri", "LICENSE.txt").decode("utf-8")
    write_text(pack / "fonts" / "OFL.txt", licence.replace("\r\n", "\n"))
    written.append("fonts/OFL.txt")
    return written
