"""Unit sprite sheets: `gfx/units/<class>_<side>.png` + `gfx/units/units.toml`.

Every sheet uses the layout of docs/ASSETS.md: 4 columns (facing down, up, left, right) x 6 rows
(walk cycle 0-3, attack 4, hurt 5).

* Foot soldiers are Ninja Adventure (NA) characters holding a weapon (or load) chosen per class.
* Riders are the upper body of an NA character on a mount: an NA horse (front/side views from the
  pack, back view drawn here), the NA bear, or a horse-drawn war chariot.
* The catapult is the MiniWorld ballista mapped onto the NA palette.

The three sides differ by the colour of clothing/armour, saddle cloths, chariot panels, lance
pennants and ballista cords, remapped onto the team ramps of art.py; outlines and skin stay as
drawn. Frames are 24x24 (32x24 for the chariot) with the feet on the bottom row, so weapons and
riders overhang the tile upwards and sideways only.
"""

from __future__ import annotations

from collections.abc import Callable, Mapping
from dataclasses import dataclass, field
from pathlib import Path

from PIL import Image

import art
from assetlib import Sources, cell, flip_h, new, paste, recolor, rgba, save_png, sprite, toml_value, write_text

DIRS = ("down", "up", "left", "right")
WALK_ROWS = 4
ATTACK_ROW = 4
HURT_ROW = 5
ROWS = 6
BODY = 16  # NA character frame size
FRAME = 24  # unit frame size (chariot: 32 wide)
RIDER_ROWS = 11  # upper-body rows of a character used for riders
OUTLINE = "#141b1b"

# Direction vectors used to lunge on attack and recoil when hurt.
FORWARD = {"down": (0, 1), "up": (0, -1), "left": (-1, 0), "right": (1, 0)}

# ---------------------------------------------------------------------------------------------
# Colour helpers


def team_table(cloth: Mapping[str, int], side: str) -> dict[tuple[int, ...], tuple[int, ...]]:
    """Source colour -> entry `index` of the side's team ramp (0 = darkest ... 4 = lightest)."""
    ramp = art.TEAM_RAMPS[side]
    return {rgba(src): rgba(ramp[i]) for src, i in cloth.items()}


def swap(img: Image.Image, table: Mapping[str, str]) -> Image.Image:
    return recolor(img, {rgba(k): rgba(v) for k, v in table.items()}, strict=False)


def shift(img: Image.Image, dx: int, dy: int) -> Image.Image:
    out = new(img.width, img.height)
    paste(out, img, dx, dy)
    return out


# ---------------------------------------------------------------------------------------------
# Weapons and carried loads

UP, DOWN, LEFT, RIGHT = "up", "down", "left", "right"
ROTATE = {
    UP: None,
    DOWN: Image.Transpose.ROTATE_180,
    LEFT: Image.Transpose.ROTATE_90,
    RIGHT: Image.Transpose.ROTATE_270,
}


def orient(img: Image.Image, direction: str) -> tuple[Image.Image, tuple[int, int]]:
    """Rotate an item drawn pointing up (grip at the bottom centre) to point `direction`.

    Returns the rotated image and the position of the grip in it.
    """
    w, h = img.size
    gx, gy = w // 2, h - 1
    op = ROTATE[direction]
    if op is None:
        return img, (gx, gy)
    out = img.transpose(op)
    if direction == DOWN:
        return out, (w - 1 - gx, h - 1 - gy)
    if direction == LEFT:
        return out, (gy, w - 1 - gx)
    return out, (h - 1 - gy, gx)


@dataclass(frozen=True)
class Hold:
    """How an item is held in one direction and pose: where it points, where the hand is (in
    16x16 body coordinates) and whether it is drawn behind the body."""

    point: str
    hand: tuple[int, int]
    behind: bool = False


# Idle weapons are held upright just outside the body so the silhouette shows the weapon type.
MELEE_IDLE = {
    "down": Hold(UP, (0, 14)),
    "up": Hold(UP, (15, 14)),
    "left": Hold(UP, (0, 14)),
    "right": Hold(UP, (15, 14)),
}
# Facing down there is no room below the feet, so that blow is a sideways sweep across the body.
MELEE_ATTACK = {
    "down": Hold(LEFT, (13, 13)),
    "up": Hold(UP, (11, 4), behind=True),
    "left": Hold(LEFT, (2, 11)),
    "right": Hold(RIGHT, (13, 11)),
}
BOW_IDLE = {
    "down": Hold(LEFT, (3, 11)),
    "up": Hold(RIGHT, (12, 10), behind=True),
    "left": Hold(LEFT, (5, 11)),
    "right": Hold(RIGHT, (10, 11)),
}
BOW_ATTACK = {
    "down": Hold(DOWN, (8, 11)),
    "up": Hold(UP, (8, 5), behind=True),
    "left": Hold(LEFT, (3, 10)),
    "right": Hold(RIGHT, (12, 10)),
}
# Carried loads (drum, rice bale) are not rotated; `hand` is where the load's centre goes.
CARRY = {
    "down": Hold(UP, (8, 12)),
    "up": Hold(UP, (8, 10)),
    "left": Hold(UP, (4, 12)),
    "right": Hold(UP, (11, 12)),
}


@dataclass(frozen=True)
class Weapon:
    load: Callable[[Sources], Image.Image]  # drawn pointing up, grip at the bottom centre
    idle: Mapping[str, Hold] = field(default_factory=lambda: MELEE_IDLE)
    attack: Mapping[str, Hold] = field(default_factory=lambda: MELEE_ATTACK)
    carried: bool = False
    pennant: bool = False  # pole weapon: riders fly a team pennant from it


def na_in_hand(name: str) -> Callable[[Sources], Image.Image]:
    """An NA `SpriteInHand` weapon (drawn pointing down) turned to point up."""
    return lambda src: src.na(f"Items/Weapons/{name}/SpriteInHand.png").transpose(Image.Transpose.ROTATE_180)


def na_item(name: str) -> Callable[[Sources], Image.Image]:
    """An NA weapon `Sprite` (inventory picture, already pointing up)."""
    return lambda src: src.na(f"Items/Weapons/{name}/Sprite.png")


def na_bow(src: Sources) -> Image.Image:
    """The NA bow (drawn with its arc down, string up) turned to shoot upwards."""
    return src.na("Items/Weapons/Bow/Sprite.png").transpose(Image.Transpose.ROTATE_180)


WEAPONS: dict[str, Weapon] = {
    "sword": Weapon(na_in_hand("Sword2")),
    "spear": Weapon(na_in_hand("Lance"), pennant=True),
    "lance": Weapon(na_in_hand("Lance2"), pennant=True),
    "halberd": Weapon(na_in_hand("Fork"), pennant=True),
    "katana": Weapon(na_in_hand("Katana")),
    "club": Weapon(na_in_hand("Club")),
    "axe": Weapon(na_item("Axe")),
    "bone": Weapon(na_in_hand("Bone")),
    "whip": Weapon(na_in_hand("Whip")),
    "wand": Weapon(na_in_hand("MagicWand")),
    "bow": Weapon(na_bow, idle=BOW_IDLE, attack=BOW_ATTACK),
    "crossbow": Weapon(lambda s: art.unit_part("crossbow")),
    "drum": Weapon(lambda s: art.unit_part("drum"), idle=CARRY, attack=CARRY, carried=True),
    "bale": Weapon(lambda s: art.unit_part("bale"), idle=CARRY, attack=CARRY, carried=True),
}


@dataclass(frozen=True)
class Armed:
    """A weapon ready to draw for one side (pennant applied)."""

    weapon: Weapon
    image: Image.Image

    @classmethod
    def of(cls, src: Sources, name: str | None, side: str, riding: bool) -> Armed | None:
        if name is None:
            return None
        weapon = WEAPONS[name]
        image = weapon.load(src)
        if riding and weapon.pennant:
            image = art.pennant_on(image, side)
        return cls(weapon, image)

    def hold(self, d: str, row: int) -> Hold:
        return (self.weapon.attack if row == ATTACK_ROW else self.weapon.idle)[d]

    def draw(self, canvas: Image.Image, hold: Hold, origin: tuple[int, int]) -> None:
        ox, oy = origin
        hx, hy = hold.hand
        if self.weapon.carried:
            paste(canvas, self.image, ox + hx - self.image.width // 2, oy + hy - self.image.height // 2)
            return
        img, (gx, gy) = orient(self.image, hold.point)
        paste(canvas, img, ox + hx - gx, oy + hy - gy)


# ---------------------------------------------------------------------------------------------
# Characters


class Character:
    """An NA character sheet (4 columns x 7 rows of 16x16) recoloured for a side."""

    # Characters whose sheet does not use the pack's usual file name.
    SHEETS = {"SamuraiRed": "Actor/Character/SamuraiRed/redsamurai.png"}

    def __init__(self, src: Sources, name: str, cloth: Mapping[str, int]) -> None:
        self.sheet = src.na(self.SHEETS.get(name, f"Actor/Character/{name}/SpriteSheet.png"))
        self.cloth = cloth

    def poses(self, side: str) -> list[list[Image.Image]]:
        """[row][direction]: rows 0-3 walk, 4 attack, 5 hurt."""
        sheet = recolor(self.sheet, team_table(self.cloth, side), strict=True)
        rows = [[cell(sheet, c, r) for c in range(4)] for r in range(5)]
        # NA has no hurt row: its crouched jump pose knocked back one pixel reads as a flinch.
        rows.append([shift(cell(sheet, c, 5), -FORWARD[d][0], -FORWARD[d][1]) for c, d in enumerate(DIRS)])
        return rows


# ---------------------------------------------------------------------------------------------
# Foot units


@dataclass(frozen=True)
class Foot:
    character: str
    cloth: Mapping[str, int]
    weapon: str | None = None


def foot_sheet(src: Sources, unit: Foot, side: str) -> Image.Image:
    poses = Character(src, unit.character, unit.cloth).poses(side)
    armed = Armed.of(src, unit.weapon, side, riding=False)
    sheet = new(FRAME * 4, FRAME * ROWS)
    origin = ((FRAME - BODY) // 2, FRAME - BODY)
    for col, d in enumerate(DIRS):
        for row in range(ROWS):
            frame = new(FRAME, FRAME)
            hold = armed.hold(d, row) if armed else None
            if armed and hold and hold.behind:
                armed.draw(frame, hold, origin)
            paste(frame, poses[row][col], *origin)
            if armed and hold and not hold.behind:
                armed.draw(frame, hold, origin)
            paste(sheet, frame, col * FRAME, row * FRAME)
    return sheet


# ---------------------------------------------------------------------------------------------
# Mounts

HORSE_RED = {"a": "#d14b34", "b": "#5f7160", "d": "#8f3e56", "k": OUTLINE}

# The NA horse has front and side views only; its back view is drawn here in the same colours.
HORSE_BACK = """
......kkkk......
....kkaaaakk....
...kaaabbaaak...
..kkaaabbaaakk..
.kaaaaabbaaaaak.
.kaaaaaaaaaaaak.
kaaaaaaaaaaaaaak
kaaaaaaaaaaaaaak
kaaaaaaaaaaaaaak
kaaaaaakkaaaaaak
kdaaaakbbkaaaadk
.kdaaakbbkaaadk.
.kddakbbbbkaddk.
..kddkbbbbkddk..
..kbk.kbbk.kbk..
..kbk..kk..kbk..
"""

# NA red horse -> coat colours.
HORSE_COATS: dict[str, dict[str, str]] = {
    "bay": {"#d14b34": "#a3754e", "#8f3e56": "#695953", "#f2ad7d": "#d2b37d", "#cf736d": "#c8966b", "#5f7160": "#3b3643"},
    "black": {"#d14b34": "#4e484a", "#8f3e56": "#3b3643", "#f2ad7d": "#8d977f", "#cf736d": "#5f7160", "#5f7160": OUTLINE},
    "white": {"#d14b34": "#f2eaf1", "#8f3e56": "#abc2bc", "#f2ad7d": "#ffcba9", "#cf736d": "#d3a2c0", "#5f7160": "#8d977f"},
}


@dataclass
class Mount:
    frames: dict[str, list[Image.Image]]  # walk frames per direction, cycled over the walk rows
    at: dict[str, tuple[int, int]]  # top-left of the mount in the unit frame
    rider_at: dict[str, tuple[int, int]]  # top-left of the rider's 16x16 body frame
    rider_behind: set[str]  # directions where the mount is drawn over the rider
    size: tuple[int, int] = (FRAME, FRAME)
    cloth: bool = True  # team saddle cloth
    extra: Callable[[Image.Image, str, int, str], None] | None = None  # (frame, dir, phase, side), drawn last


def _strip(sheet: Image.Image, w: int) -> list[Image.Image]:
    return [sheet.crop((i * w, 0, i * w + w, sheet.height)) for i in range(sheet.width // w)]


def horse(src: Sources, coat: str) -> Mount:
    table = HORSE_COATS[coat]
    front = _strip(swap(src.na("Actor/Animal/Horse/SpriteSheetBrown.png"), table), 16)
    side = _strip(swap(src.na("Actor/Animal/Horse/SpriteSheetBrownSide.png"), table), 23)
    back0 = swap(sprite(HORSE_BACK, HORSE_RED), table)
    return Mount(
        frames={"down": front, "up": [back0, shift(back0, 0, 1)], "right": side, "left": [flip_h(f) for f in side]},
        at={"down": (4, 8), "up": (4, 8), "right": (1, 8), "left": (0, 8)},
        rider_at={"down": (4, 0), "up": (4, 1), "right": (1, 1), "left": (7, 1)},
        rider_behind={"down"},
    )


def bear(src: Sources) -> Mount:
    sheet = swap(
        src.na("Actor/Monster/Bear/SpriteSheet.png"),
        {"#d14b34": "#a3754e", "#8f3e56": "#695953", "#f2ad7d": "#d2b37d", "#fce2ca": "#eecf9b"},
    )
    # NA monster sheets: columns down, left, up, right; rows are the walk frames.
    cols = {"down": 0, "left": 1, "up": 2, "right": 3}
    return Mount(
        frames={d: [cell(sheet, c, r) for r in range(4)] for d, c in cols.items()},
        at={d: (4, 8) for d in DIRS},
        rider_at={"down": (4, 0), "up": (4, 1), "right": (3, 1), "left": (5, 1)},
        rider_behind={"down"},
    )


def chariot(src: Sources) -> Mount:
    """A bay horse drawing a two-wheeled war chariot; the rider stands in the car."""
    team_horse = horse(src, "bay")
    w, h = 32, FRAME
    placed = {"down": (8, 8), "up": (8, 0), "right": (9, 8), "left": (0, 8)}
    frames: dict[str, list[Image.Image]] = {}
    for d in DIRS:
        frames[d] = []
        for f in team_horse.frames[d]:
            img = new(w, h)
            # seen from behind, only the horse's head shows above the car and rider
            paste(img, f.crop((0, 0, 16, 8)) if d == "up" else f, *placed[d])
            frames[d].append(img)
    return Mount(
        frames=frames,
        at={d: (0, 0) for d in DIRS},
        rider_at={"down": (8, 0), "up": (8, 3), "right": (-1, 1), "left": (17, 1)},
        rider_behind={"down"},
        size=(w, h),
        cloth=False,
        extra=_chariot_car,
    )


def _chariot_car(frame: Image.Image, d: str, phase: int, side: str) -> None:
    """The car (team-coloured panels, spoked wheels), drawn over the rider's legs."""
    wheel_side = art.unit_part("wheel", side)
    wheel_edge = art.unit_part("wheel_edge", side)
    if d == "down":
        paste(frame, wheel_edge, 4, 11 + phase % 2)
        paste(frame, wheel_edge, 25, 11 + phase % 2)
    elif d == "up":
        paste(frame, art.unit_part("car_back", side), 8, 12)
        paste(frame, wheel_edge, 4, 14)
        paste(frame, wheel_edge, 25, 14)
    else:
        car = art.unit_part("car_side", side)
        x_car, x_wheel = (0, 2) if d == "right" else (32 - car.width, 32 - 2 - wheel_side.width)
        paste(frame, car, x_car, 11)
        paste(frame, wheel_side, x_wheel, 15)


MOUNTS: dict[str, Callable[[Sources], Mount]] = {
    "bay": lambda s: horse(s, "bay"),
    "black": lambda s: horse(s, "black"),
    "white": lambda s: horse(s, "white"),
    "bear": bear,
    "chariot": chariot,
}


def saddle_cloth(frame: Image.Image, d: str, mount_img: Image.Image, at: tuple[int, int], side: str) -> None:
    """A team-coloured caparison over the mount's back, centred on it."""
    x, y = at
    if d in ("left", "right"):
        cloth = art.unit_part("cloth_side", side)
        if d == "left":
            cloth = flip_h(cloth)
        # the saddle sits a little behind the middle of the side view
        cx = x + mount_img.width // 2 + (-3 if d == "right" else 3)
        paste(frame, cloth, cx - cloth.width // 2, y + 5)
    elif d == "up":
        cloth = art.unit_part("cloth_back", side)
        paste(frame, cloth, x + (mount_img.width - cloth.width) // 2, y + 5)
    else:
        cloth = art.unit_part("cloth_front", side)
        paste(frame, cloth, x + (mount_img.width - cloth.width) // 2, y + 6)


@dataclass(frozen=True)
class Rider:
    character: str
    cloth: Mapping[str, int]
    mount: str
    weapon: str | None = None


def upper_body(body: Image.Image) -> Image.Image:
    out = new(BODY, BODY)
    out.paste(body.crop((0, 0, BODY, RIDER_ROWS)), (0, 0))
    return out


def rider_sheet(src: Sources, unit: Rider, side: str) -> tuple[Image.Image, tuple[int, int]]:
    poses = Character(src, unit.character, unit.cloth).poses(side)
    mount = MOUNTS[unit.mount](src)
    armed = Armed.of(src, unit.weapon, side, riding=True)
    fw, fh = mount.size
    sheet = new(fw * 4, fh * ROWS)
    for col, d in enumerate(DIRS):
        walk = mount.frames[d]
        fx, fy = FORWARD[d]
        for row in range(ROWS):
            if row < WALK_ROWS:
                phase, dx, dy = row % len(walk), 0, 0
                bob = 1 if len(walk) == 2 and phase == 1 else 0  # NA horses dip on their second frame
            elif row == ATTACK_ROW:
                phase, dx, dy, bob = 0, fx, fy, 0
            else:
                phase, dx, dy, bob = len(walk) - 1, -fx, -fy, 0
            mount_img = walk[phase]
            mx, my = mount.at[d]
            mx, my = mx + dx, my + dy
            rx, ry = mount.rider_at[d]
            origin = (rx + dx, ry + dy + bob)
            rider = upper_body(poses[row][col])
            hold = armed.hold(d, row) if armed else None
            frame = new(fw, fh)
            if armed and hold and hold.behind:
                armed.draw(frame, hold, origin)
            if d in mount.rider_behind:
                paste(frame, rider, *origin)
                paste(frame, mount_img, mx, my)
                if mount.cloth:
                    saddle_cloth(frame, d, mount_img, (mx, my), side)
            else:
                paste(frame, mount_img, mx, my)
                if mount.cloth:
                    saddle_cloth(frame, d, mount_img, (mx, my), side)
                paste(frame, rider, *origin)
            if mount.extra is not None:
                mount.extra(frame, d, phase, side)
            if armed and hold and not hold.behind:
                armed.draw(frame, hold, origin)
            paste(sheet, frame, col * fw, row * fh)
    return sheet, (fw // 2, fh - 1)


# ---------------------------------------------------------------------------------------------
# Siege engine

BALLISTA = "MiniWorldSprites/Characters/Soldiers/Ranged/Ballista.png"
# MiniWorld colours -> NA palette; the white cords and bow arms take the team colour.
BALLISTA_NA = {
    "#171717": OUTLINE,
    "#000000": OUTLINE,
    "#50391c": "#3b3643",
    "#93674d": "#816855",
    "#99703b": "#a3754e",
    "#d3a061": "#c8966b",
    "#e2ad6e": "#d2b37d",
    "#b1b1b1": "#abc2bc",
}
BALLISTA_CORD = "#f4f4f4"


@dataclass(frozen=True)
class Machine:
    sheet: str  # MiniWorld sheet: rows up, down, right, left; columns 0-2 loaded, 3-6 shot/reload


def machine_sheet(src: Sources, unit: Machine, side: str) -> Image.Image:
    table = {**BALLISTA_NA, BALLISTA_CORD: art.TEAM_RAMPS[side][3]}
    source = swap(src.image("miniworld", unit.sheet), table)
    rows = {"up": 0, "down": 1, "right": 2, "left": 3}
    sheet = new(FRAME * 4, FRAME * ROWS)
    origin = ((FRAME - BODY) // 2, FRAME - BODY)
    for col, d in enumerate(DIRS):
        r = rows[d]
        fx, fy = FORWARD[d]
        frames = [cell(source, c, r) for c in (0, 1, 2, 1)]  # rolling
        frames.append(shift(cell(source, 4, r), fx, fy))  # the shot, recoiling forwards
        frames.append(shift(cell(source, 0, r), -fx, -fy))  # knocked back
        for row, img in enumerate(frames):
            paste(sheet, img, col * FRAME + origin[0], row * FRAME + origin[1])
    return sheet


# ---------------------------------------------------------------------------------------------
# Class table (docs/ASSETS.md)

# Clothing/armour colours of the NA characters used, mapped to team ramp entries (0 dark .. 4 light).
SAMURAI = {"#8f3e56": 1, "#e0394c": 2}
SAMURAI_BLUE = {"#4a5270": 0, "#548789": 1, "#79b8ce": 2, "#9ba7aa": 3}
GLADIATOR = {"#4e484a": 0, "#5f7160": 1, "#8d977f": 2, "#4a5270": 0, "#548789": 1, "#79b8ce": 3}
KNIGHT = {"#4e484a": 0, "#5f7160": 1, "#8d977f": 2}
KNIGHT_GOLD = {"#8f3e56": 1, "#d14b34": 2}
HUNTER = {"#4a5270": 0, "#548789": 1, "#8f3e56": 1, "#d14b34": 2, "#79b8ce": 3}
SAMURAI_RED = {"#8f3e56": 1, "#d14b34": 2, "#e0394c": 3}
RACOON = {"#965340": 1, "#e46d3a": 2, "#ffad5d": 3}
CAMOUFLAGE = {"#8f3e56": 1, "#e0394c": 2}
NINJA_DARK = {"#3b3643": 0, "#4e484a": 1, "#5f7160": 2}
FIGHTER = {"#8f3e56": 1, "#e0394c": 2, "#d3a2c0": 3}
CAVE_LION = {"#8f3e56": 1, "#e0394c": 2}
LION_BOY = {"#56864c": 1, "#a8a129": 3}
MASTER = {"#8f3e56": 1, "#e0394c": 2}
NOBLE = {"#3b3643": 0, "#4e484a": 1, "#965340": 3}
INSPECTOR = {"#91522c": 1, "#d14b34": 2}
VILLAGER = {"#4e484a": 0, "#548789": 2, "#79b8ce": 3}

Unit = Foot | Rider | Machine

UNITS: dict[str, Unit] = {
    "short_infantry": Foot("Samurai", SAMURAI, "sword"),
    "long_infantry": Foot("GladiatorBlue", GLADIATOR, "spear"),
    "chariot": Rider("SamuraiBlue", SAMURAI_BLUE, "chariot", "spear"),
    "light_cavalry": Rider("SamuraiBlue", SAMURAI_BLUE, "bay", "spear"),
    "heavy_cavalry": Rider("Knight", KNIGHT, "black", "lance"),
    "guard_cavalry": Rider("KnightGold", KNIGHT_GOLD, "white", "halberd"),
    "archer": Foot("Hunter", HUNTER, "bow"),
    "crossbow": Foot("SamuraiRed", SAMURAI_RED, "crossbow"),
    "catapult": Machine(BALLISTA),
    "bandit": Foot("MaskRacoon", RACOON, "club"),
    "brigand": Foot("CamouflageRed", CAMOUFLAGE, "axe"),
    "outlaw": Foot("NinjaDark", NINJA_DARK, "katana"),
    "martial": Foot("FighterWhite", FIGHTER),
    "tribe": Foot("CaveLion", CAVE_LION, "bone"),
    "beast": Rider("LionBoy", LION_BOY, "bear", "whip"),
    "supply": Foot("Inspector", INSPECTOR, "bale"),
    "band": Foot("Noble", NOBLE, "drum"),
    "sorcerer": Foot("Master", MASTER, "wand"),
    "civilian": Foot("Villager", VILLAGER),
}

HEADER = """\
# Unit sprites (generated by tools/assets/build.py - do not edit by hand).
# One sheet per class and side: <key>_player.png (blue), <key>_ally.png (green), <key>_enemy.png (red).
# Layout (docs/ASSETS.md): 4 columns = facing down, up, left, right; rows 0-3 walk (idle = row 0),
# row 4 attack, row 5 hurt. `frame` = frame size in pixels, `anchor` = frame pixel placed on the
# tile's bottom-centre pixel (8, 15); larger frames overhang the tile upwards and sideways.
"""


def unit_sheet(src: Sources, unit: Unit, side: str) -> tuple[Image.Image, tuple[int, int]]:
    if isinstance(unit, Foot):
        return foot_sheet(src, unit, side), (FRAME // 2, FRAME - 1)
    if isinstance(unit, Machine):
        return machine_sheet(src, unit, side), (FRAME // 2, FRAME - 1)
    return rider_sheet(src, unit, side)


def build_units(src: Sources, pack: Path) -> list[str]:
    out_dir = pack / "gfx" / "units"
    written = []
    lines = [HEADER]
    for key, unit in UNITS.items():
        sizes = set()
        for side in art.SIDES:
            sheet, anchor = unit_sheet(src, unit, side)
            sizes.add((sheet.size, anchor))
            save_png(sheet, out_dir / f"{key}_{side}.png")
            written.append(f"gfx/units/{key}_{side}.png")
        if len(sizes) != 1:
            raise ValueError(f"{key}: the side sheets differ in size or anchor")
        (w, h), anchor = sizes.pop()
        frame = [w // 4, h // ROWS]
        lines.append(f"\n[sprites.{key}]\nframe = {toml_value(frame)}\nanchor = {toml_value(list(anchor))}\n")
    write_text(out_dir / "units.toml", "".join(lines))
    written.append("gfx/units/units.toml")
    return written
