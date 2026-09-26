"""UI graphics: `gfx/ui/icons.png` + `icons.toml` and the animated side banners `gfx/ui/flags.png`."""

from __future__ import annotations

from collections.abc import Callable
from pathlib import Path

from PIL import Image

import art
from assetlib import (
    Atlas,
    Sources,
    cell,
    new,
    paste,
    ramp_mapping,
    recolor,
    rgba,
    save_png,
    toml_value,
    trim,
    write_text,
)

ICON = 16


def centred(img: Image.Image) -> Image.Image:
    """Trim transparent borders and centre the sprite on a 16x16 cell (bottom-heavy rounding)."""
    img = trim(img)
    if img.width > ICON or img.height > ICON:
        raise ValueError(f"icon {img.size} larger than {ICON}x{ICON}")
    out = new(ICON, ICON)
    paste(out, img, (ICON - img.width) // 2, (ICON - img.height + 1) // 2)
    return out


# Colour ramps (dark -> light) of the MiniWorld arrow and question-mark icons.
MW_ARROW = ["#50391c", "#835835", "#b38047", "#e2ad6e"]
MW_QUESTION = ["#02808a", "#349aa4", "#4ebcb9", "#66d2cf"]


def _mw_ui(src: Sources, col: int, row: int) -> Image.Image:
    sheet = src.image("miniworld", "MiniWorldSprites/User Interface/UiIcons.png")
    return cell(sheet, col, row)


def _chinese(src: Sources, col: int, row: int) -> Image.Image:
    return cell(src.image("chinese_icons"), col, row)


def _flag_frame(src: Sources, color: str) -> Image.Image:
    return cell(src.na(f"Backgrounds/Animated/Flag/Flag{color}16x16.png"), 0, 0)


def _confuse_scroll(src: Sources) -> Image.Image:
    scroll = src.na("Items/Scroll/ScrollEmpty.png")
    paste(scroll, art.icon("question"), 6, 5)
    return scroll


# Each icon key -> function producing the (untrimmed) sprite. Engine keys come first (see
# docs/ASSETS.md), then item icons for content authors (listed in tools/assets/README.md).
ICONS: dict[str, Callable[[Sources], Image.Image]] = {
    # engine keys
    "gold": lambda s: _chinese(s, 0, 0),
    "weather_clear": lambda s: art.icon("sun"),
    "weather_cloudy": lambda s: art.icon("cloud"),
    "weather_rain": lambda s: art.icon("rain"),
    "hp": lambda s: s.na("Ui/Receptacle/IconHeart.png"),
    "mp": lambda s: art.icon("orb"),
    "morale": lambda s: _flag_frame(s, "Yellow"),
    "atk": lambda s: _mw_ui(s, 1, 0),
    "def": lambda s: _mw_ui(s, 2, 0),
    "move": lambda s: art.icon("boot"),
    "exp": lambda s: art.icon("star"),
    "weapon": lambda s: s.na("Items/Weapons/Sword2/Sprite.png"),
    "armor": lambda s: art.icon("armor"),
    "accessory": lambda s: art.icon("pendant"),
    "consumable": lambda s: s.na("Items/Potion/LifePot.png"),
    "fire": lambda s: art.icon("flame"),
    "water": lambda s: art.icon("drop"),
    "earth": lambda s: s.na("Items/Resource/Rock.png"),
    "heal": lambda s: _mw_ui(s, 2, 8),
    "morale_up": lambda s: recolor(
        _mw_ui(s, 2, 4), ramp_mapping(MW_ARROW, ["#1f4a22", "#2f7a36", "#4fa843", "#8fd65e"])
    ),
    "morale_down": lambda s: recolor(
        _mw_ui(s, 3, 4), ramp_mapping(MW_ARROW, ["#2e2f52", "#4a5270", "#6f76b8", "#a9aee8"])
    ),
    "confuse": lambda s: recolor(
        _mw_ui(s, 1, 7), ramp_mapping(MW_QUESTION, ["#6b2c4c", "#a5608b", "#c784ae", "#efb8dc"])
    ),
    "lord": lambda s: art.icon("crown"),
    "commander": lambda s: art.icon("helmet"),
    # item icons
    "item_sword": lambda s: s.na("Items/Weapons/Sword2/Sprite.png"),
    "item_blade": lambda s: s.na("Items/Weapons/BigSword/Sprite.png"),
    "item_spear": lambda s: s.na("Items/Weapons/Lance/SpriteInHand.png").transpose(Image.Transpose.FLIP_TOP_BOTTOM),
    "item_halberd": lambda s: s.na("Items/Weapons/Fork/Sprite.png"),
    "item_axe": lambda s: s.na("Items/Weapons/Axe/Sprite.png"),
    "item_bow": lambda s: s.na("Items/Weapons/Bow/Sprite.png").transpose(Image.Transpose.ROTATE_90),
    "item_crossbow": lambda s: art.icon("crossbow"),
    "item_armor": lambda s: art.icon("armor"),
    "item_book": lambda s: s.na("Items/Object/Book.png"),
    "item_horse": lambda s: cell(s.na("Actor/Animal/Horse/SpriteSheetBrown.png"), 0, 0),
    "item_bean": lambda s: s.na("Items/Food/SeedLarge.png"),
    "item_wheat": lambda s: art.icon("wheat"),
    "item_rice": lambda s: s.na("Items/Food/Onigiri.png"),
    "item_wine": lambda s: s.na("Items/Object/Gourd.png"),
    "item_peach": lambda s: _chinese(s, 2, 1),
    "item_medicine": lambda s: s.na("Items/Potion/Medipack.png"),
    "item_scroll": lambda s: s.na("Items/Scroll/Scroll.png"),
    "item_scroll_fire": lambda s: s.na("Items/Scroll/ScrollFire.png"),
    "item_scroll_water": lambda s: s.na("Items/Scroll/ScrollIce.png"),
    "item_scroll_earth": lambda s: s.na("Items/Scroll/ScrollRock.png"),
    "item_scroll_confuse": _confuse_scroll,
    "item_classup": lambda s: s.na("Items/Treasure/GoldCup.png"),
    "item_seal": lambda s: art.icon("jade_seal"),
    "item_edict": lambda s: _chinese(s, 2, 2),
    "item_report": lambda s: s.na("Items/Other/Letter.png"),
    "item_gold": lambda s: _chinese(s, 0, 1),
    "item_silver": lambda s: _chinese(s, 0, 2),
    "item_gem": lambda s: _chinese(s, 1, 0),
    "item_chest": lambda s: cell(s.na("Items/Treasure/LittleTreasureChest.png"), 0, 0),
    "item_bag": lambda s: s.na("Items/Object/MoneyBag.png"),
}

ICONS_HEADER = """\
# UI icons (generated by tools/assets/build.py - do not edit by hand).
# key = [column, row] of a 16x16 cell in icons.png. Item icon keys (item_*) are listed with
# their meaning in tools/assets/README.md.
"""


def build_icons(src: Sources, pack: Path) -> list[str]:
    atlas = Atlas(columns=8)
    entries = []
    for key, make in ICONS.items():
        entries.append((key, atlas.add(centred(make(src)))))
    out_dir = pack / "gfx" / "ui"
    save_png(atlas.image(), out_dir / "icons.png")
    lines = [ICONS_HEADER, "\n[icons]\n"]
    lines += [f"{key} = {toml_value(pos)}\n" for key, pos in entries]
    write_text(out_dir / "icons.toml", "".join(lines))
    return ["gfx/ui/icons.png", "gfx/ui/icons.toml"]


def team_flag(src: Sources, side: str) -> Image.Image:
    """The Ninja Adventure red flag (4 frames) with its cloth remapped to a team ramp."""
    ramp = art.TEAM_RAMPS[side]
    return recolor(
        src.na("Backgrounds/Animated/Flag/FlagRed16x16.png"),
        {rgba("#d14b34"): rgba(ramp[2]), rgba("#e46d3a"): rgba(ramp[3])},
    )


def build_flags(src: Sources, pack: Path) -> list[str]:
    sheet = new(64, 16 * len(art.SIDES))
    for row, side in enumerate(art.SIDES):
        sheet.alpha_composite(team_flag(src, side), (0, row * 16))
    save_png(sheet, pack / "gfx" / "ui" / "flags.png")
    return ["gfx/ui/flags.png"]
