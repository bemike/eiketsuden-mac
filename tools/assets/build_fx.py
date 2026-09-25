"""Battle effects: `gfx/fx/<key>.png` horizontal frame strips + `gfx/fx/fx.toml`."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from PIL import Image

from assetlib import Sources, new, paste, recolor, rgba, save_png, toml_value, write_text


@dataclass(frozen=True)
class Strip:
    frames: list[Image.Image]
    fps: int

    @property
    def size(self) -> tuple[int, int]:
        return self.frames[0].size

    def image(self) -> Image.Image:
        w, h = self.size
        out = new(w * len(self.frames), h)
        for i, f in enumerate(self.frames):
            out.alpha_composite(f, (i * w, 0))
        return out


def split(sheet: Image.Image, count: int) -> list[Image.Image]:
    if sheet.width % count:
        raise ValueError(f"strip width {sheet.width} is not a multiple of {count} frames")
    fw = sheet.width // count
    return [sheet.crop((i * fw, 0, i * fw + fw, sheet.height)) for i in range(count)]


def fit(frames: list[Image.Image], w: int, h: int, valign: str = "center") -> list[Image.Image]:
    """Place every frame on a w x h canvas, centred horizontally (and vertically or at the bottom)."""
    out = []
    for f in frames:
        if f.width > w or f.height > h:
            raise ValueError(f"frame {f.size} does not fit {w}x{h}")
        c = new(w, h)
        x = (w - f.width) // 2
        y = (h - f.height) // 2 if valign == "center" else h - f.height
        paste(c, f, x, y)
        out.append(c)
    return out


def _slash(src: Sources) -> Strip:
    frames = split(src.na("FX/Attack/SlashCurved/SpriteSheet.png"), 4)
    return Strip(fit(frames, 32, 32), fps=14)


def _arrow(src: Sources) -> Strip:
    """A short volley: three arrows fall onto the target tile and stick (drawn for this pack)."""
    arrow = src.na("FX/Projectile/Arrow.png").transpose(Image.Transpose.ROTATE_270)  # tip down
    aw, ah = arrow.size
    volley = [(8, 22, 0), (17, 25, 1), (24, 20, 2)]  # (x of the shaft, landing y of the tip, delay)
    speed = 9
    frames = []
    for t in range(8):
        f = new(32, 32)
        for x, land, delay in volley:
            k = t - delay
            if k < 0:
                continue
            tip = min(land, land - speed * (3 - k))
            top = tip - ah + 1
            shaft = arrow if tip < land else arrow.crop((0, 0, aw, ah - 2))  # stuck: tip hidden
            paste(f, shaft, x - aw // 2, top)
            if tip == land and k <= 4:
                # small dust puff where the arrow hit
                dust = rgba("#c8b49a")
                for dx, dy in ((-2, 0), (2, 0), (-1, -1), (1, -1)) if k <= 3 else ((-3, 0), (3, 0)):
                    px, py = x + dx, land - 1 + dy
                    if 0 <= px < 32 and 0 <= py < 32:
                        f.putpixel((px, py), dust)
        frames.append(f)
    return Strip(frames, fps=16)


def _fire(src: Sources) -> Strip:
    frames = split(src.na("FX/Elemental/Flam/SpriteSheet.png"), 8)
    return Strip(fit(frames, 32, 32, valign="bottom"), fps=12)


def _water(src: Sources) -> Strip:
    frames = split(src.na("FX/Elemental/Water/SpriteSheet.png"), 11)
    return Strip(fit(frames, 40, 40), fps=14)


def _rock(src: Sources) -> Strip:
    frames = split(src.na("FX/Elemental/Rock/SpriteSheet.png"), 14)
    return Strip(fit(frames, 32, 32), fps=14)


def _heal(src: Sources) -> Strip:
    sheet = recolor(
        src.na("FX/Magic/Spark/SpriteSheet.png"),
        {rgba("#f1c471"): rgba("#7ed957"), rgba("#d3a2c0"): rgba("#c6f59a")},
    )
    return Strip(fit(split(sheet, 9), 32, 36), fps=12)


def _morale_up(src: Sources) -> Strip:
    frames = split(src.na("FX/Magic/Shield/SpriteSheetYellow.png"), 6)
    return Strip(fit(frames, 24, 32, valign="bottom"), fps=10)


def _morale_down(src: Sources) -> Strip:
    sheet = recolor(
        src.na("FX/Magic/Shield/SpriteSheetYellow.png"),
        {
            rgba("#ffffff"): rgba("#d6d9f2"),
            rgba("#ffe18d"): rgba("#8d93d6"),
            rgba("#ff9554"): rgba("#4a5270"),
            rgba("#e46d3a"): rgba("#2e2f52"),
        },
    )
    frames = [f.transpose(Image.Transpose.FLIP_TOP_BOTTOM) for f in split(sheet, 6)]
    return Strip(fit(frames, 24, 32, valign="center"), fps=10)


def _confuse(src: Sources) -> Strip:
    swirl = src.na("FX/Magic/Spirit/SpriteSheet.png")
    frames = []
    for f in split(swirl, 5):
        body = recolor(f, {rgba("#ffffff"): rgba("#f4cbe8")})
        edge = new(f.width, f.height)
        a = f.getchannel("A")
        for y in range(f.height):
            for x in range(f.width):
                if a.getpixel((x, y)):
                    continue
                if any(
                    0 <= x + dx < f.width and 0 <= y + dy < f.height and a.getpixel((x + dx, y + dy))
                    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))
                ):
                    edge.putpixel((x, y), rgba("#8f3e56"))
        edge.alpha_composite(body)
        frames.append(edge)
    return Strip(frames, fps=10)


def _levelup(src: Sources) -> Strip:
    frames = split(src.na("FX/Magic/Circle/SpriteSheetSpark.png"), 6)
    return Strip(fit(frames, 32, 32), fps=12)


FX: dict[str, Callable[[Sources], Strip]] = {
    "slash": _slash,
    "arrow": _arrow,
    "fire": _fire,
    "water": _water,
    "rock": _rock,
    "heal": _heal,
    "morale_up": _morale_up,
    "morale_down": _morale_down,
    "confuse": _confuse,
    "levelup": _levelup,
}

HEADER = """\
# Battle effects (generated by tools/assets/build.py - do not edit by hand).
# Each <key>.png is a horizontal strip of `frames` frames of `frame` = [w, h] pixels, played at
# `fps`. Frames are designed to be drawn centred on the centre of the target tile.
"""


def build_fx(src: Sources, pack: Path) -> list[str]:
    out_dir = pack / "gfx" / "fx"
    lines = [HEADER]
    written = []
    for key, make in FX.items():
        strip = make(src)
        save_png(strip.image(), out_dir / f"{key}.png")
        written.append(f"gfx/fx/{key}.png")
        w, h = strip.size
        lines.append(f"\n[fx.{key}]\nframe = {toml_value([w, h])}\nframes = {len(strip.frames)}\nfps = {strip.fps}\n")
    write_text(out_dir / "fx.toml", "".join(lines))
    written.append("gfx/fx/fx.toml")
    return written
