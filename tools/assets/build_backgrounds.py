"""Drama backgrounds (`gfx/bg/<key>.png`, 960x540) cropped from public-domain Chinese paintings.

Every background gets the same grade so that scenes from different paintings sit together and the
white text of title cards stays readable on them:

1. a 16:9 region of the painting (chosen by eye, see BACKGROUNDS) is scaled to 960x540;
2. saturation is reduced and a gamma curve brings the mean brightness to one target value, then
   the highlights are rolled off so no area gets close to white;
3. a soft vignette darkens the corners;
4. the result is reduced to 256 colours with Floyd-Steinberg dithering (indexed PNG, about 40 %
   of the size of a truecolour PNG and indistinguishable at 1x-2x).

`night` is the same treatment with most colour removed and a blue moonlight tint; `black` is plain
black (for fades and `@bg black`).
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageEnhance

from assetlib import SourceError, Sources, save_png

SIZE = (960, 540)


@dataclass(frozen=True)
class Background:
    key: str
    source: str
    part: str
    # left, top and width of the crop in source pixels; the height is width * 9 / 16
    crop: tuple[int, int, int]
    night: bool = False


BACKGROUNDS: list[Background] = [
    # court ladies, pillars and balustrades of a palace hall
    Background("palace", "han_palace", "", (150, 40, 2750)),
    # shops, courtyards and a busy street
    Background("town", "qingming_qing", "14", (300, 300, 2400)),
    # willows, a farmstead and travellers on the village green
    Background("village", "qingming_qing", "03", (300, 680, 2400)),
    # the yurt camp and ranks of officials at Chengde
    Background("camp", "wanshuyuan", "", (1700, 150, 2900)),
    # open country under the hills, a road and a column of riders
    Background("field", "kangxi_tour", "", (400, 40, 2400)),
    # the broad river below the Red Cliff
    Background("river", "red_cliff", "", (0, 245, 820)),
    # blue-green peaks and a mountain road with riders
    Background("mountain", "minghuang", "", (0, 0, 4000)),
    # a city gate tower on the wall
    Background("castle", "qingming_qing", "11", (700, 350, 2300)),
    # lake and mountains, turned to moonlight
    Background("night", "thousand_li", "", (2400, 80, 2700), night=True),
]

SATURATION = 0.70  # colour kept (1 = unchanged)
MEAN_LUMA = 0.40  # target mean brightness (0..1) after the gamma curve
HIGHLIGHT_KNEE = 0.62  # brightness above which highlights are compressed ...
HIGHLIGHT_MAX = 0.84  # ... so that pure white maps to this
VIGNETTE = 0.30  # brightness lost in the corners

NIGHT_SATURATION = 0.25
NIGHT_MEAN_LUMA = 0.22
NIGHT_TINT = (0.55, 0.72, 1.05)  # channel gains of the moonlight grade
NIGHT_LIFT = (0, 0, 10)  # added to the channels after the gains
NIGHT_VIGNETTE = 0.45


def _mean_luma(img: Image.Image) -> float:
    hist = img.convert("L").histogram()
    return sum(i * c for i, c in enumerate(hist)) / sum(hist) / 255


def _tone_curve(gamma: float) -> list[int]:
    """Gamma, then a smooth highlight roll-off above HIGHLIGHT_KNEE (continuous slope at the knee)."""
    out = []
    span = HIGHLIGHT_MAX - HIGHLIGHT_KNEE
    for v in range(256):
        x = (v / 255) ** gamma
        if x > HIGHLIGHT_KNEE:
            # exponential approach to HIGHLIGHT_MAX with slope 1 at the knee
            x = HIGHLIGHT_KNEE + span * (1 - math.exp(-(x - HIGHLIGHT_KNEE) / span))
        out.append(round(255 * x))
    return out


def _vignette(size: tuple[int, int], strength: float) -> Image.Image:
    """Multiplier mask: 255 in the centre, 255 * (1 - strength) in the corners (quadratic)."""
    w, h = size
    small = Image.new("L", (w // 8, h // 8))
    px = small.load()
    for y in range(small.height):
        for x in range(small.width):
            dx = (x + 0.5) / small.width * 2 - 1
            dy = (y + 0.5) / small.height * 2 - 1
            r = min(1.0, math.hypot(dx, dy) / math.sqrt(2))
            px[x, y] = round(255 * (1 - strength * r * r))
    return small.resize(size, Image.Resampling.BICUBIC)


def grade(img: Image.Image, night: bool = False) -> Image.Image:
    """Apply the common background grade to a 960x540 RGB image."""
    img = ImageEnhance.Color(img).enhance(NIGHT_SATURATION if night else SATURATION)
    target = NIGHT_MEAN_LUMA if night else MEAN_LUMA
    gamma = math.log(target) / math.log(min(0.99, max(0.01, _mean_luma(img))))
    img = img.point(_tone_curve(gamma) * 3)
    if night:
        img = Image.merge(
            "RGB",
            [
                band.point([min(255, round(v * gain) + lift) for v in range(256)])
                for band, gain, lift in zip(img.split(), NIGHT_TINT, NIGHT_LIFT, strict=True)
            ],
        )
    mask = _vignette(img.size, NIGHT_VIGNETTE if night else VIGNETTE)
    return Image.composite(img, Image.new("RGB", img.size), mask)


def render(src: Sources, bg: Background) -> Image.Image:
    with Image.open(src.path(bg.source, bg.part)) as im:
        painting = im.convert("RGB")
    x, y, w = bg.crop
    h = w * SIZE[1] // SIZE[0]
    if x < 0 or y < 0 or x + w > painting.width or y + h > painting.height:
        raise SourceError(f"bg {bg.key}: crop {bg.crop} leaves the {painting.width}x{painting.height} source")
    img = painting.crop((x, y, x + w, y + h)).resize(SIZE, Image.Resampling.LANCZOS)
    graded = grade(img, bg.night)
    return graded.quantize(colors=256, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.FLOYDSTEINBERG)


def build_backgrounds(src: Sources, pack: Path) -> list[str]:
    out_dir = pack / "gfx" / "bg"
    written = []
    for bg in BACKGROUNDS:
        save_png(render(src, bg), out_dir / f"{bg.key}.png")
        written.append(f"gfx/bg/{bg.key}.png")
    save_png(Image.new("RGB", SIZE), out_dir / "black.png")
    written.append("gfx/bg/black.png")
    return written
