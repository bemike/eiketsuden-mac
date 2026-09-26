"""Pixel art drawn for this project as text (one character per pixel), in the Ninja Adventure palette.

These sprites fill gaps the third-party packs do not cover at 16x16 (weather, stat symbols,
Chinese-themed items, small buildings, trees, ...). They are original work of the Eiketsuden
Reloaded project; see CREDITS.md.
"""

from __future__ import annotations

from PIL import Image

from assetlib import sprite

# Shared palette. Most entries are Ninja Adventure's own colours (Palette.png); the jade and the
# team ramps below were added for this project.
PAL: dict[str, str | None] = {
    ".": None,
    "k": "#141b1b",  # outline
    "w": "#ffffff",
    "W": "#f2eaf1",
    "l": "#b8dce5",
    "s": "#abc2bc",
    "S": "#8d977f",
    "d": "#5f7160",
    "y": "#ffe18d",
    "Y": "#f1c471",
    "o": "#ffad5d",
    "O": "#e46d3a",
    "r": "#e0394c",
    "R": "#8f3e56",
    "b": "#8feff1",
    "c": "#71ddee",
    "B": "#3da6bd",
    "D": "#2d697b",
    "u": "#4a5270",
    "g": "#adbc3a",
    "G": "#74a334",
    "h": "#56864c",
    "m": "#8bd6a8",
    "M": "#3f9e70",
    "q": "#23604a",
    "n": "#c8966b",
    "N": "#a3754e",
    "e": "#695953",
    "p": "#ef9597",
    "P": "#a5608b",
    "v": "#d3a2c0",
    "j": "#345a52",
    "x": "#3b3643",
    "z": "#4e484a",
    "a": "#816855",
    "t": "#d78b4a",
    "f": "#eecf9b",
    "F": "#d2b37d",
    "i": "#b3957f",
    "I": "#8e7c73",
    "E": "#90775e",
    "C": "#548789",
    "H": "#d3865f",
    "A": "#965340",
}

# Team colour ramps, dark -> light. Clothing/armour ramps of unit sprites and flags are remapped
# onto these so the three sides read the same on every unit.
TEAM_RAMPS: dict[str, list[str]] = {
    "player": ["#1b2754", "#27438f", "#3a6bd0", "#72a2ee", "#bcd7ff"],
    "ally": ["#163b22", "#23682c", "#3a9c38", "#7cc751", "#c9ec98"],
    "enemy": ["#4a1420", "#8a1d2a", "#d23140", "#ef7266", "#ffc2ad"],
}
SIDES = ("player", "ally", "enemy")


def draw(art: str) -> Image.Image:
    return sprite(art, PAL)


# --- icons ------------------------------------------------------------------------------------

SUN = """
.......OO.......
..O....OO....O..
...O........O...
......kkkk......
....kkyyyykk....
....kyywyyyk....
...kyywyyyyYk...
OO.kyyyyyyyYk.OO
OO.kyyyyyyYYk.OO
...kyyyyyYYYk...
....kyyyYYYk....
....kkYYYYkk....
......kkkk......
...O........O...
..O....OO....O..
.......OO.......
"""

CLOUD = """
....kkk.........
...kwwwk.kkkk...
..kwwwwwkwwwwk..
.kwwwwwwwwwwwwk.
kwwwwwwwwwwwwwwk
kwwwwwwwwwwwwwlk
klwwwwwwwwwwwllk
.kllllllllllllk.
..kkkkkkkkkkkk..
"""

RAIN = """
....kkk.........
...ksssk.kkkk...
..ksssssksssSk..
.kssssssssssSSk.
ksssssssssssSSSk
kssssssssssSSSSk
kSssssssssSSSSSk
.kSSSSSSSSSSSSk.
..kkkkkkkkkkkk..
................
..B....B....B...
.B....B....B....
................
....B....B....B.
...B....B....B..
"""

BOOT = """
.kkkkk......
.kNnnk......
.knnnk......
.knnnk......
.kOOOk......
.knnnk......
.knnnnkk....
.knnnnnnkk..
.knnnnnnnnk.
.kNNNNNNNNk.
.kkkkkkkkkk.
"""

STAR = """
.......k.......
......kyk......
......kyk......
.....kyyyk.....
kkkkkyyyyykkkkk
kyyyyyywyyyyyYk
.kyyyyyyyyyyYk.
..kyyyyyyyyYk..
...kyyyyyyYk...
...kyyyyyYYk...
..kyyyykyyYYk..
..kyyk...kYYk..
.kyk.......kYk.
.kk.........kk.
"""

ARMOR = """
..kkk....kkk..
.kSssk..kssSk.
kSsssskkssssSk
kSsWssssssssSk
kSssssssssssSk
.kSsSsSsSsSSk.
.kssssssssssk.
.kSsSsSsSsSSk.
.kssssssssssk.
.kOOOOOOOOOOk.
.kSsSsSsSsSSk.
..kssssssssk..
...kkkkkkkk...
"""

PENDANT = """
.....k.....
....kOk....
.....O.....
...kkkkk...
..kmmmmMk..
.kmmkkkmMk.
.kmk...kMk.
.kmk...kMk.
.kmMkkkMMk.
..kMMMMMk..
...kkkkk...
.....O.....
....kOk....
....rOr....
....rrr....
"""

FLAME = """
....k......
...kOk.....
...kOOk..k.
..kOOOk.kOk
..kOoOOkkOk
.kOooOOOOOk
.kOoooOOOOk
kOooyyooOOk
kOoyyyyooOk
kOoyywyyoOk
kOoyywwyoOk
.kOoyyyoOk.
..kOOOOOk..
...kkkkk...
"""

DROP = """
....kk....
....kk....
...kcBk...
...kcBk...
..kccBBk..
..kcBBBk..
.kccBBBBk.
.kcwBBBBk.
kcwcBBBBDk
kcwBBBBBDk
kccBBBBBDk
.kcBBBBDk.
..kBBDDk..
...kkkk...
"""

ORB = """
....kkkk....
..kkbccBkk..
.kbwbcccBBk.
.kwbccccBBk.
kbbcccccBBDk
kbccccccBBDk
kccccccBBBDk
kcccccBBBDDk
.kccBBBBDDk.
.kBBBBBDDDk.
..kkDDDDkk..
....kkkk....
"""

CROWN = """
.k.....k.....k.
kyk...kyk...kyk
kyyk.kyyyk.kyyk
kyyykyyryyykyyk
kyyyyyrrryyyyyk
kyyyyyyryyyyyYk
kyyyyyyyyyyyYYk
kkkkkkkkkkkkkkk
kYrYYYbYYYrYYOk
kOOOOOOOOOOOOOk
.kkkkkkkkkkkkk.
"""

HELMET = """
.....krk.....
....krrrk....
.....krk.....
....kYYYk....
...kyyyyYk...
..kyywyyyYk..
.kyyyyyyyyYk.
kOOOOOOOOOOOk
kOkkkkkkkkkOk
kOk.......kOk
kOk.......kOk
kOOk.....kOOk
.kkk.....kkk.
"""

CROSSBOW = """
......kk......
.....kwwk.....
.....kssk.....
kk...kNNk...kk
kNkkkkNNkkkkNk
.kNNNNNNNNNNk.
..kkwkNNkwkk..
....wkNNkw....
.....kNNk.....
.....kNNk.....
....kkNNk.....
...kekNNk.....
.....kNNk.....
.....kkkk.....
"""

JADE_SEAL = """
....kkkk....
...kmmMMk...
...kmMMMk...
....kMMk....
.kkkkkkkkkk.
kmmmmmmmmMMk
kmwmmmmmmMMk
kmmmmmmmMMMk
kMMMMMMMMMqk
krrrrrrrrrRk
.kkkkkkkkkk.
"""

WHEAT = """
......kkk......
.....kyyYk.....
.....kYyyk.....
.kkk.kyyYk.kkk.
kyyYkkYyykkyyYk
kYyykkyyYkkYyyk
kyyYk.kyk.kyyYk
kYyyk..G..kYyyk
kyyYk..G..kyyYk
.kyk...G...kyk.
..G....G....G..
...G...G...G...
....GkOOOkG....
......GGG......
.....G.G.G.....
"""

QUESTION = """
.PPP.
P...P
....P
..PP.
..P..
.....
..P..
"""


# --- terrain objects (drawn onto 16x16 map cells by build_terrain.py) -------------------------

# Thatched farmhouse of the village tile.
HUT = """
.....kk.....
....kYYk....
...kYYYtk...
..kYYYYYtk..
.kYYYYYYttk.
kYYYYYYYtttk
kttttttttttk
.kffffNNffk.
.kfFffNNfFk.
.kkkkkkkkkk.
"""

# Wasteland decoration: loose stones and a dry tuft.
STONES = """
..kk.....
.kiIk....
.kIIk.kk.
..kk.kiIk
.....kkk.
"""

DRY_TUFT = """
.F..F..
..FiF.F
F.iFi..
.iIiIi.
"""

# Town house with a grey tiled roof (impassable `house`); fills the whole cell.
HOUSE = """
kk............kk
.kkkkkkkkkkkkkk.
kssSsSsSsSsSsSsk
kCuCuCuCuCuCuCuk
kCuCuCuCuCuCuCuk
kCuCuCuCuCuCuCuk
kuuuuuuuuuuuuuuk
kkkkkkkkkkkkkkkk
.kWWWWWNNWWWWWk.
.kWRRRWNNWRRRWk.
.kWRRRWNNWRRRWk.
.kWRRRWNNWRRRWk.
.kWWWWWNNWWWWWk.
.kWWWWWaaWWWWWk.
.kSSSSSaaSSSSSk.
.kkkkkkkkkkkkkk.
"""

# Army tent of the barracks tile.
TENT = """
.....kk.....
....kYWk....
...kYYWWk...
..kYYYWWWk..
.kYYYYWWWWk.
kYYYYYWWWWWk
kYYYYkkWWWWk
kYYYkxxkWWWk
kYYkxxxxkWWk
kkkkkkkkkkkk
"""

# Pennant on a pole (barracks and fort).
PENNANT = """
kk....
kOrrk.
kOrrrk
kOrrk.
kk....
k.....
k.....
k.....
k.....
k.....
"""

# Watchtower of the fort tile (the palisade in front of it is drawn procedurally).
WATCHTOWER = """
....kkkkkk....
...kOOOOOOk...
..kOOOOOOOOk..
.kOOOOOOOOOOk.
kttttttttttttk
kkkkkkkkkkkkkk
.kNkkxxxxkkNk.
.kNkxxxxxxkNk.
.kNkkkkkkkkNk.
.kNaNNNNNNaNk.
"""

# Storehouses of the granary (brown tiles) and treasury (slate tiles, golden coin sign).
GRANARY = """
k..............k
kk............kk
.kkkkkkkkkkkkkk.
kotototototototk
kAtAtAtAtAtAtAtk
kAAAAAAAAAAAAAAk
kkkkkkkkkkkkkkkk
.kffffkNNkffffk.
.kfFFfkNNkfFFfk.
.kfFFfkaakfFFfk.
.kffffkaakffffk.
.kkkkkkkkkkkkkk.
"""

TREASURY = """
k..............k
kk............kk
.kkkkkkkkkkkkkk.
kIiIiIiIiIiIiIik
kzxzxzxzxzxzxzxk
kxxxxxxxxxxxxxxk
kkkkkkkkkkkkkkkk
.kSsSkkkkkkSsSk.
.ksSkyyyyYOksSk.
.kSskyyxxYOkSsk.
.ksSkyyxxOOksSk.
.kSskYYOOOOkSsk.
.ksSskkkkkksSsk.
.kSsSkddddkSsSk.
.ksSskdzzdksSsk.
.kkkkkkkkkkkkkk.
"""

# Straw rice bale (granary) and a small treasure chest (treasury).
BALE = """
.kkkkk.
kyYtYyk
kYYtYYk
kYYtYYk
.kkkkk.
"""

CHEST = """
.kkkkkk.
kNnnnnNk
kkkkkkkk
kNnyynNk
kNNyyNNk
kkkkkkkk
"""

SACK = """
..kk..
.kwWk.
kwwWWk
kwWWsk
kWWssk
.kkkk.
"""

# Open city gate: roofed gate tower over an arch, the castle floor showing through.
GATE = """
k..............k
kk............kk
.kkkkkkkkkkkkkk.
kssSsSsSsSsSsSsk
kCuCuCuCuCuCuCuk
kuuuuuuuuuuuuuuk
kkkkkkkkkkkkkkkk
kRrRRrRRRRrRRrRk
kSdSdkkkkkkdSdSk
kdSdkxxxxxxkdSdk
kSdkxxxxxxxxkdSk
kdSkNxxxxxxNkSdk
kSdkNxxxxxxNkdSk
kdSkNxxxxxxNkSdk
kSdkNffffffNkdSk
kkkkkffffffkkkkk
"""

TERRAIN_ART = {
    "hut": HUT,
    "stones": STONES,
    "dry_tuft": DRY_TUFT,
    "house": HOUSE,
    "tent": TENT,
    "pennant": PENNANT,
    "watchtower": WATCHTOWER,
    "granary": GRANARY,
    "sack": SACK,
    "bale": BALE,
    "chest": CHEST,
    "treasury": TREASURY,
    "gate": GATE,
}


def terrain(name: str) -> Image.Image:
    return draw(TERRAIN_ART[name])


def icon(name: str) -> Image.Image:
    arts = {
        "sun": SUN,
        "cloud": CLOUD,
        "rain": RAIN,
        "boot": BOOT,
        "star": STAR,
        "armor": ARMOR,
        "pendant": PENDANT,
        "flame": FLAME,
        "drop": DROP,
        "orb": ORB,
        "crown": CROWN,
        "helmet": HELMET,
        "crossbow": CROSSBOW,
        "jade_seal": JADE_SEAL,
        "wheat": WHEAT,
        "question": QUESTION,
    }
    return draw(arts[name])
