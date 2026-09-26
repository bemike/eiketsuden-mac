# Asset pipeline

This directory rebuilds every graphic, sound effect and font of the base pack (`data/base`) from
pinned, openly licensed sources. The generated files are committed (they are the game's runtime
data); the downloaded archives are not. Conventions for all outputs are in
[docs/ASSETS.md](../../docs/ASSETS.md); authors and licences are in [CREDITS.md](../../CREDITS.md).

Portraits (`gfx/portraits`), drama backgrounds (`gfx/bg`) and music (`bgm`) are not produced here.

## Rebuilding

Requirements: Python 3.12+ and [Pillow](https://pypi.org/project/pillow/) (`pip install pillow`).

```sh
python tools/assets/fetch.py            # download sources into tools/assets/.cache/ and verify SHA-256
python tools/assets/build.py            # regenerate everything under data/base
python tools/assets/build.py --check    # rebuild into a temp dir and compare with the committed files
python tools/assets/build.py units      # selected steps (steps they depend on run too)
python tools/assets/preview.py          # review sheets in tools/assets/.cache/preview/
```

The build is deterministic: the same sources give byte-identical outputs, so `--check` proves the
committed pack matches the pipeline. A full build fails if a source pinned in `sources.toml` is not
used by any step, which keeps `sources.toml` and `CREDITS.md` honest.

### Sources and the Ninja Adventure download

`sources.toml` pins each source's URL, size, SHA-256 and licence. `fetch.py` refuses a file whose
size or hash differs and removes partial downloads. The Ninja Adventure pack has no stable URL:
`fetch.py` follows the itch.io "name your own price" flow (read the page's `csrf_token`, POST it to
`<page>/file/16981275`, GET the short-lived signed URL from the JSON answer). If itch.io changes that
flow, download the zip by hand from <https://pixel-boy.itch.io/ninja-adventure-asset-pack> and save it
as `.cache/NinjaAdventure-AssetPack.zip`; `fetch.py` then only verifies it. The archive.org mirror
holds an older (2021) version that lacks files we use, so it cannot replace the pinned zip.

To add or update a source: pin it in `sources.toml` (size and SHA-256), use it from a build step,
credit it in `CREDITS.md` with the outputs that use it, and rebuild.

Lint and format the scripts with `uvx ruff check --no-cache tools/assets` and
`uvx ruff format --no-cache tools/assets` (settings in `ruff.toml`).

## Layout

| file | role |
|---|---|
| `sources.toml` | pinned third-party archives |
| `fetch.py` | download + verification |
| `build.py` | step runner (`terrain`, `units`, `fonts`, `sfx`, `fx`, `icons`, `flags`, `title`) and `--check` |
| `assetlib.py` | source access (zip members), recolouring, atlas and deterministic PNG/TOML writers |
| `art.py` | pixel art drawn for this project, as text (one character per pixel) in the NA palette; team ramps |
| `build_terrain.py` | `gfx/tiles/terrain.png` + `terrain.toml` |
| `build_units.py` | `gfx/units/<class>_<side>.png` + `units.toml` |
| `build_fx.py` | `gfx/fx/<key>.png` + `fx.toml` |
| `build_ui.py` | `gfx/ui/icons.png` + `icons.toml`, `gfx/ui/flags.png` |
| `build_title.py` | `gfx/ui/title.png` (composed from the generated tiles, units and flags) |
| `build_audio.py` | `sfx/<key>.wav`, `fonts/` |
| `tilemap.py` | reference renderer of `terrain.toml` (used by the title and the previews) |
| `preview.py` | review sheets: sample map with every unit, tile catalogue, unit sheets |

## Terrain (`gfx/tiles/terrain.toml`)

One tile key per canonical terrain id. Ground layers are 4-bit autotiles; objects are variant cells.

| key | look | autotile `connect` |
|---|---|---|
| `plain` | light grass with tufts | – |
| `grass` | dark meadow patches | grass, forest, mountain |
| `forest` | pine groves on meadow | ground: grass, forest, mountain |
| `mountain` | rocky, snow-capped peaks on meadow | ground: grass, forest, mountain |
| `road` | dirt road | road, bridge, gate, village, barracks, fort |
| `wasteland` | pale stony ground, stones and dry tufts | wasteland |
| `river` | water with foam and earthen banks | river, bridge |
| `bridge` | wooden deck over water, turned across the river | water: river, bridge; deck: river |
| `castle` | flagstone floor | – |
| `gate` | gate tower over an open arch on flagstones | – |
| `wall` | crenellated stone wall with a south face | wall, gate |
| `cliff` | raised rock with a front face | cliff |
| `village` | thatched farmhouses on a dirt yard | yard: village, barracks, fort, road |
| `barracks` | army tents and a pennant | yard: village, barracks, fort, road |
| `fort` | watchtower behind a palisade | yard: village, barracks, fort, road |
| `granary` | storehouse (brown tiles) with straw rice bales | – |
| `treasury` | storehouse (slate tiles) with a gold coin sign and a chest | – |
| `house` | town house with a grey tiled roof (impassable) | – |
| `fence` | wooden palisade | fence |

A map should keep rivers at least two tiles wide where they bend: the 16-cell autotiles have no inner
corners, so a one-tile diagonal step shows a notch.

## Units (`gfx/units`)

All sheets: 4 columns (down, up, left, right) x 6 rows (walk 0-3, attack 4, hurt 5); the hurt pose
is NA's crouch knocked back one pixel. Frames are 24x24 with `anchor = [12, 23]` (the chariot is
32x24, anchor `[16, 23]`), so units overhang the tile only upwards and sideways. Team colours:
`player` blue, `ally` green, `enemy` red (ramps in `art.py`).

| class | look (Ninja Adventure unless noted) | weapon / load |
|---|---|---|
| `short_infantry` | straw-hat foot soldier (Samurai) | sword |
| `long_infantry` | armoured soldier (GladiatorBlue), armour in team colour | spear |
| `chariot` | topknot warrior in a two-wheeled chariot drawn by a bay horse | spear with pennant |
| `light_cavalry` | topknot rider (SamuraiBlue) on a bay horse, saddle cloth | spear with pennant |
| `heavy_cavalry` | helmeted rider (Knight) on a black horse | lance with pennant |
| `guard_cavalry` | gold-armoured rider (KnightGold) on a white horse | halberd with pennant |
| `archer` | hunter | bow |
| `crossbow` | officer in a lacquered hat (SamuraiRed) | crossbow (drawn) |
| `catapult` | ballista (MiniWorld), cords in team colour | – |
| `bandit` | racoon-masked thief | club |
| `brigand` | masked ruffian in a straw hat | axe |
| `outlaw` | hooded swordsman (NinjaDark) | katana |
| `martial` | martial artist with headband | fists |
| `tribe` | tribesman in a beast-hood (CaveLion) | bone club |
| `beast` | beast tamer (LionBoy) riding a bear | whip |
| `supply` | official in a white hat (Inspector) | rice bale |
| `band` | official in a black hat (Noble) | war drum |
| `sorcerer` | white-haired sage (Master) | wand |
| `civilian` | villager | – |

## Icons (`gfx/ui/icons.toml`)

Engine keys (docs/ASSETS.md): `gold`, `weather_clear`, `weather_cloudy`, `weather_rain`, `hp`, `mp`,
`morale`, `atk`, `def`, `move`, `exp`, `weapon`, `armor`, `accessory`, `consumable`, `fire`, `water`,
`earth`, `heal`, `morale_up`, `morale_down`, `confuse`, `lord`, `commander`.

Item icons for content authors (use the key as an item's icon):

| key | picture | typical items |
|---|---|---|
| `item_sword` | sword | swords |
| `item_blade` | broad blade | great swords, glaives |
| `item_spear` | spear | spears, long spears (class-up to long infantry) |
| `item_halberd` | halberd / trident | halberds (戟) |
| `item_axe` | axe | axes |
| `item_bow` | bow | bows |
| `item_crossbow` | crossbow | crossbows (class-up to crossbowmen) |
| `item_armor` | armour | armour, horse armour |
| `item_book` | book | manuals, strategy books, class-change books (指南書) |
| `item_horse` | horse | horses |
| `item_bean` | bean | beans (small heal) |
| `item_wheat` | wheat sheaf | wheat |
| `item_rice` | rice ball | rice |
| `item_wine` | gourd | wine |
| `item_peach` | peach | peaches |
| `item_medicine` | medicine box | medicines |
| `item_scroll` | plain scroll | generic strategy scrolls |
| `item_scroll_fire` | fire scroll | fire strategy books |
| `item_scroll_water` | water scroll | water strategy books |
| `item_scroll_earth` | earth scroll | earth strategy books |
| `item_scroll_confuse` | scroll with a question mark | confusion strategy books |
| `item_classup` | golden cup | class-up items |
| `item_seal` | jade seal | the imperial seal (玉璽) and other seals |
| `item_edict` | scroll | imperial edicts |
| `item_report` | letter | reports, letters |
| `item_gold` | gold ingot | gold treasures |
| `item_silver` | silver ingot | silver treasures |
| `item_gem` | gem | jewels |
| `item_chest` | chest | treasure chests |
| `item_bag` | money bag | money |

## Sound effects (`sfx`)

All from Ninja Adventure, converted to 16-bit mono WAV and trimmed (`build_audio.py` lists the
source file of each key): `cursor`, `confirm`, `cancel`, `error`, `step`, `hit`, `hit_heavy`,
`arrow`, `fire`, `water`, `rock`, `heal`, `morale_up`, `morale_down`, `confuse`, `levelup`,
`retreat`, `treasure`, `phase`, `victory`, `defeat`.
