"""Officer portraits (`gfx/portraits/<key>.png`) from the 繡像 pages of 增像全圖三國演義.

Every portrait is a head-and-shoulders crop of one figure, chosen and framed by hand in
`portraits.toml`: a full-length figure of that book, or (where the book's drawing does not read
as a face at portrait size) a figure from another pinned woodblock book, whose printed panel on
the scanned page is given so the levels ignore the rest of the scan. All of them get the same
treatment so they read as one set next to the blue UI:

1. the page scan (or the figure's panel) is levelled (1st percentile -> ink, 60th percentile ->
   paper), which restores the faded lithograph lines and clears the paper grain;
2. the crop is lightly denoised (3x3 median) and scaled to 192x240 (4:5, docs/ASSETS.md);
3. brightness is mapped through one ramp from indigo ink to warm paper, with a soft vignette and
   an inset double rule echoing the frames printed around each figure in the book.

The ramp has at most 256 colours, so the PNGs are stored indexed (small downloads for the web
build). `_unknown.png`, the fallback for officers without a portrait, is an ink-wash silhouette
with the same paper, vignette and frame.
"""

from __future__ import annotations

import math
import tomllib
from dataclasses import dataclass
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

from assetlib import PACK_DIR, TOOLS_DIR, SourceError, Sources, save_png

SIZE = (192, 240)
SOURCE = "xiuxiang"
QUADS = ("TR", "TL", "BR", "BL")  # the book fills a page right to left, top to bottom

# Levels, as percentiles of the page's grey histogram.
INK_PERCENTILE = 0.01
PAPER_PERCENTILE = 0.60
GAMMA = 1.15  # > 1 darkens the mid greys so thin strokes survive downscaling

INK = (0x1B, 0x1A, 0x2C)  # indigo-black
PAPER = (0xEF, 0xE4, 0xCB)  # warm paper
VIGNETTE = 0.22  # paper brightness lost at the corners
FRAME_INSET = 5  # px from the image edge to the outer rule; survives the UI's cover-fit crop
FRAME_TONE = 0.30  # brightness of the rules (0 = ink, 1 = paper)


Rect = tuple[int, int, int, int]


@dataclass(frozen=True)
class Portrait:
    key: str
    figure: str
    page: int
    quad: str
    face: tuple[int, int]
    width: int
    mirror: bool
    stand_in: bool
    erase: tuple[Rect, ...] = ()
    # Another pinned source than the 繡像 pages: its id, the part (page) and the printed panel of
    # the figure on it. `page` and `quad` are unused then.
    source: str = SOURCE
    part: str = ""
    panel: Rect | None = None
    # Size of a minimum filter run over the levelled crop (odd, 0 = none): thickens thin lines
    # to the weight of the set before the crop is scaled down.
    bold: int = 0

    @property
    def box(self) -> Rect:
        """Crop box in page pixels: `face` sits at the horizontal centre, 40 % from the top."""
        w = self.width
        h = w * SIZE[1] // SIZE[0]
        left = self.face[0] - w // 2
        top = self.face[1] - h * 2 // 5
        return (left, top, left + w, top + h)

    @property
    def figure_id(self) -> tuple[str, str, str]:
        """What identifies the drawn figure (for the reuse rules)."""
        if self.source == SOURCE:
            return (SOURCE, str(self.page), self.quad)
        return (self.source, self.part, self.figure)


def _rect(value: list[int]) -> Rect:
    return (int(value[0]), int(value[1]), int(value[2]), int(value[3]))


def load_table(path: Path = TOOLS_DIR / "portraits.toml") -> tuple[list[Portrait], dict[int, dict[str, str]], set[str]]:
    """Portrait entries, the page index (page -> quadrant -> printed name) and the reserved names."""
    with path.open("rb") as f:
        doc = tomllib.load(f)
    index = {int(p): dict(zip(QUADS, names, strict=True)) for p, names in doc["pages"].items()}
    reserved = set(doc["reserved"]["figures"])
    entries = []
    for key, e in doc["portraits"].items():
        source = e.get("source", SOURCE)
        entries.append(
            Portrait(
                key=key,
                figure=e["figure"],
                page=int(e["page"]) if source == SOURCE else 0,
                quad=e["quad"] if source == SOURCE else "",
                face=(int(e["face"][0]), int(e["face"][1])),
                width=int(e.get("width", 300)),
                mirror=bool(e.get("mirror", False)),
                stand_in=bool(e.get("stand_in", False)),
                erase=tuple(_rect(r) for r in e.get("erase", [])),
                source=source,
                part=str(e.get("part", "")),
                panel=_rect(e["panel"]) if "panel" in e else None,
                bold=int(e.get("bold", 0)),
            )
        )
    return entries, index, reserved


def officers(pack: Path = PACK_DIR) -> dict[str, str]:
    """Officer id -> portrait key (the `portrait` field, default the id) of the pack."""
    with (pack / "officers.toml").open("rb") as f:
        doc = tomllib.load(f)
    return {o["id"]: o.get("portrait", o["id"]) for o in doc["officer"]}


def check_table(
    entries: list[Portrait], index: dict[int, dict[str, str]], reserved: set[str], keys: set[str]
) -> list[str]:
    """Refuse inconsistent mappings before anything is drawn; returns the keys no officer uses.

    Such entries are legitimate (a drama can `@show` any portrait key, and a chapter in progress
    may add its officers later) but are listed so a misspelt key does not go unnoticed.
    """
    problems = []
    by_key = {p.key: p for p in entries}
    for key in sorted(keys - set(by_key)):
        problems.append(f"officer portrait {key!r} has no entry in portraits.toml")
    own = {p.figure_id: p.key for p in entries if not p.stand_in}
    users: dict[tuple[str, str, str], list[Portrait]] = {}
    for p in entries:
        users.setdefault(p.figure_id, []).append(p)
        if p.stand_in and p.figure_id in own:
            problems.append(f"{p.key}: {p.figure} is the own figure of {own[p.figure_id]}")
        if p.stand_in and p.figure in reserved:
            problems.append(f"{p.key}: {p.figure} is reserved for its own person and cannot stand in")
        if p.source != SOURCE:
            # another book: no page index; the panel bounds the levels and the crop
            if p.panel is None:
                problems.append(f"{p.key}: a figure from {p.source} needs its `panel`")
            elif not _inside(p.box, p.panel):
                problems.append(f"{p.key}: crop box {p.box} leaves the panel {p.panel}")
            continue
        printed = index.get(p.page, {}).get(p.quad)
        if printed is None:
            problems.append(f"{p.key}: page {p.page} {p.quad} is not in the page index")
            continue
        if printed != p.figure:
            problems.append(f"{p.key}: page {p.page} {p.quad} shows {printed}, not {p.figure}")
    for group in users.values():
        if len(group) > 2 or (len(group) == 2 and [p.mirror for p in group].count(True) != 1):
            keys = ", ".join(p.key for p in group)
            problems.append(f"{keys}: a figure may serve at most twice, the second use mirrored")
    if problems:
        raise SourceError("portraits.toml:\n  " + "\n  ".join(problems))
    return sorted(set(by_key) - keys)


def _inside(inner: Rect, outer: Rect) -> bool:
    return outer[0] <= inner[0] and outer[1] <= inner[1] and inner[2] <= outer[2] and inner[3] <= outer[3]


# ---------------------------------------------------------------------------------------------
# Treatment


def _percentile(img: Image.Image, q: float) -> int:
    hist = img.histogram()
    target = q * sum(hist)
    acc = 0
    for value, count in enumerate(hist):
        acc += count
        if acc >= target:
            return value
    return 255


def _levels(page: Image.Image, region: Rect | None = None) -> Image.Image:
    """Stretch ink to black and paper to white, measured on the whole page or on `region`."""
    sample = page if region is None else page.crop(region)
    lo = _percentile(sample, INK_PERCENTILE)
    hi = max(lo + 1, _percentile(sample, PAPER_PERCENTILE))
    lut = [round(255 * min(1.0, max(0.0, (v - lo) / (hi - lo))) ** GAMMA) for v in range(256)]
    return page.point(lut)


def _vignette_frame(tone: Image.Image) -> Image.Image:
    """Mat the picture inside an inset double rule and darken the paper towards the edges.

    Works on tone (0 ink .. 255 paper): the picture shows only inside the inner rule, the band
    outside it is blank paper like the margin around a printed frame.
    """
    w, h = tone.size
    i = FRAME_INSET
    matted = Image.new("L", (w, h), 255)
    inner = (i + 5, i + 5, w - i - 5, h - i - 5)
    matted.paste(tone.crop(inner), inner[:2])
    tone = matted
    mask = Image.new("L", (w, h))
    px = mask.load()
    cx, cy = (w - 1) / 2, (h - 1) / 2
    for y in range(h):
        for x in range(w):
            # 0 at the centre, 1 at the corners of the inscribed ellipse's bounding box
            r = math.hypot((x - cx) / cx, (y - cy) / cy) / math.sqrt(2)
            px[x, y] = round(255 * (1 - VIGNETTE * r * r))
    out = Image.new("L", (w, h))
    # tone * mask / 255, so ink stays ink and only the paper darkens
    out.putdata([t * m // 255 for t, m in zip(tone.getdata(), mask.getdata(), strict=True)])
    draw = ImageDraw.Draw(out)
    draw.rectangle((i, i, w - 1 - i, h - 1 - i), outline=round(255 * FRAME_TONE), width=2)
    draw.rectangle((i + 4, i + 4, w - 1 - i - 4, h - 1 - i - 4), outline=round(255 * FRAME_TONE * 1.6), width=1)
    return out


def _colorize(tone: Image.Image) -> Image.Image:
    """Map tone (0 ink .. 255 paper) through the ink -> paper ramp."""
    channels = [tone.point([round(INK[c] + (PAPER[c] - INK[c]) * v / 255) for v in range(256)]) for c in range(3)]
    return Image.merge("RGB", channels)


def render(page: Image.Image, p: Portrait) -> Image.Image:
    left, top, right, bottom = p.box
    if left < 0 or top < 0 or right > page.width or bottom > page.height:
        raise SourceError(f"{p.key}: crop box {p.box} leaves the {page.width}x{page.height} page")
    levelled = _levels(page, p.panel)
    if p.erase:
        draw = ImageDraw.Draw(levelled)
        for rect in p.erase:
            draw.rectangle(rect, fill=255)
    crop = levelled.crop(p.box)
    if p.bold:
        crop = crop.filter(ImageFilter.MinFilter(p.bold))
    crop = crop.filter(ImageFilter.MedianFilter(3))
    tone = crop.resize(SIZE, Image.Resampling.LANCZOS)
    if p.mirror:
        tone = tone.transpose(Image.Transpose.FLIP_LEFT_RIGHT)
    return _colorize(_vignette_frame(tone))


def unknown_portrait() -> Image.Image:
    """Ink-wash head-and-shoulders silhouette (a scholar's cap) on the portrait paper."""
    s = 4  # draw at 4x and scale down for soft edges
    w, h = SIZE[0] * s, SIZE[1] * s
    img = Image.new("L", (w, h), 255)
    d = ImageDraw.Draw(img)
    ink = 70  # a lighter ink than the drawings: nobody in particular
    cx = w // 2
    head_w, head_h = 64 * s, 78 * s
    head_top = 62 * s
    d.ellipse((cx - head_w // 2, head_top, cx + head_w // 2, head_top + head_h), fill=ink)
    d.rounded_rectangle((cx - 30 * s, head_top - 22 * s, cx + 30 * s, head_top + 16 * s), radius=10 * s, fill=ink)
    d.rectangle((cx - 48 * s, head_top + 2 * s, cx + 48 * s, head_top + 8 * s), fill=ink)  # cap wings
    neck_top = head_top + head_h - 10 * s
    d.rectangle((cx - 16 * s, neck_top, cx + 16 * s, neck_top + 26 * s), fill=ink)
    d.pieslice((cx - 120 * s, neck_top + 12 * s, cx + 120 * s, neck_top + 12 * s + 200 * s), 180, 360, fill=ink)
    img = img.filter(ImageFilter.GaussianBlur(1.5 * s)).resize(SIZE, Image.Resampling.LANCZOS)
    return _colorize(_vignette_frame(img))


def build_portraits(src: Sources, pack: Path) -> list[str]:
    entries, index, reserved = load_table()
    unused = check_table(entries, index, reserved, set(officers().values()))
    if unused:
        print(f"  portraits no officer uses (drawn anyway): {', '.join(unused)}")
    out_dir = pack / "gfx" / "portraits"
    written = []
    pages: dict[tuple[str, str], Image.Image] = {}
    for p in sorted(entries, key=lambda e: (e.source, e.page, e.part, e.key)):
        page_id = (SOURCE, str(p.page)) if p.source == SOURCE else (p.source, p.part)
        if page_id not in pages:
            with Image.open(src.path(*page_id)) as im:
                pages[page_id] = im.convert("L")
        save_png(render(pages[page_id], p), out_dir / f"{p.key}.png")
        written.append(f"gfx/portraits/{p.key}.png")
    save_png(unknown_portrait(), out_dir / "_unknown.png")
    written.append("gfx/portraits/_unknown.png")
    return sorted(written)
