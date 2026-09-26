# Asset pipeline

This directory rebuilds every graphic, portrait, background, piece of music, sound effect and font
of the base pack (`data/base`) from pinned, openly licensed sources. The generated files are committed (they are the game's runtime
data); the downloaded archives are not. Conventions for all outputs are in
[docs/ASSETS.md](../../docs/ASSETS.md); authors and licences are in [CREDITS.md](../../CREDITS.md).

## Rebuilding

Requirements: Python 3.12+, [Pillow](https://pypi.org/project/pillow/) and
[fontTools](https://pypi.org/project/fonttools/) with WOFF2 support
(`pip install pillow "fonttools[woff]"`), and [ffmpeg](https://ffmpeg.org) with libvorbis on `PATH`
(or in the `FFMPEG` environment variable) for the music.

```sh
python tools/assets/fetch.py            # download sources into tools/assets/.cache/ and verify SHA-256
python tools/assets/build.py            # regenerate everything under data/base
python tools/assets/build.py --check    # rebuild into a temp dir and compare with the committed files
python tools/assets/build.py units      # selected steps (steps they depend on run too)
python tools/assets/preview.py          # review sheets in tools/assets/.cache/preview/
```

The build is deterministic: the same sources give byte-identical outputs, so `--check` proves the
committed pack matches the pipeline. Two steps also depend on tool versions: `music` on the ffmpeg /
libvorbis build (the committed files were made with ffmpeg 8.1.2, gyan.dev full build) and
`portraits` / `backgrounds` / every image step on Pillow's resampling and quantisation; a different
version may give slightly different bytes, so compare with `--check` on the same tools. A full build fails if a source pinned in `sources.toml` is not
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
credit it in `CREDITS.md` with the outputs that use it, and rebuild. Several files of one work (the
pages of a scanned book, the sections of a scroll) are one source with `[sources.<id>.parts]`; a
full build also fails on a pinned part that no step reads. Wikimedia serves bursts of downloads with
HTTP 429; `fetch.py` waits and retries. The portrait pages are Wikimedia's renders of a djvu file:
should they ever be re-rendered, `fetch.py` reports the new hashes and the parts must be re-pinned.

Lint and format the scripts with `uvx ruff check --no-cache tools/assets` and
`uvx ruff format --no-cache tools/assets` (settings in `ruff.toml`).
Unit tests of the pure logic (source table parsing, the portrait table checks, grading and filter
helpers) run without the cache: `python -m unittest discover -s tools/assets -p "test_*.py"`.

## Layout

| file | role |
|---|---|
| `sources.toml` | pinned third-party archives |
| `fetch.py` | download + verification |
| `build.py` | step runner (`terrain`, `units`, `fonts`, `sfx`, `fx`, `icons`, `flags`, `title`, `portraits`, `backgrounds`, `music`) and `--check` |
| `assetlib.py` | source access (zip members), recolouring, atlas and deterministic PNG/TOML writers |
| `art.py` | pixel art drawn for this project, as text (one character per pixel) in the NA palette; team ramps |
| `build_terrain.py` | `gfx/tiles/terrain.png` + `terrain.toml` |
| `build_units.py` | `gfx/units/<class>_<side>.png` + `units.toml` |
| `build_fx.py` | `gfx/fx/<key>.png` + `fx.toml` |
| `build_ui.py` | `gfx/ui/icons.png` + `icons.toml`, `gfx/ui/flags.png` |
| `build_title.py` | `gfx/ui/title.png` (composed from the generated tiles, units and flags) |
| `build_audio.py` | `sfx/<key>.wav` |
| `build_fonts.py` | `fonts/`: Galmuri completed with the Hanja of the pack's text |
| `build_portraits.py` + `portraits.toml` | `gfx/portraits/<officer>.png`, `_unknown.png` |
| `build_backgrounds.py` | `gfx/bg/<key>.png` |
| `build_music.py` | `bgm/<key>.ogg` |
| `tilemap.py` | reference renderer of `terrain.toml` (used by the title and the previews) |
| `preview.py` | review sheets: sample map with every unit, tile catalogue, unit sheets |
| `test_pipeline.py` | unit tests (see above) |

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
| `item_bomb` | powder bomb | bombs, fire pots |

## Sound effects (`sfx`)

All from Ninja Adventure, converted to 16-bit mono WAV and trimmed (`build_audio.py` lists the
source file of each key): `cursor`, `confirm`, `cancel`, `error`, `step`, `hit`, `hit_heavy`,
`arrow`, `fire`, `water`, `rock`, `heal`, `morale_up`, `morale_down`, `confuse`, `levelup`,
`retreat`, `treasure`, `phase`, `victory`, `defeat`.

## Portraits (`gfx/portraits`)

192x240 head-and-shoulders crops of the full-length figures in the portrait section of
增像全圖三國演義 (a late-Qing illustrated edition of the novel, public domain), levelled, lightly
denoised, recoloured to indigo ink on warm paper and framed. `portraits.toml` maps every officer of
`data/base/officers.toml` to a figure (page, quarter, face point, crop width, optional `erase`
rectangles for printed text); the build refuses a table that misses an officer, names the wrong
figure, uses a reserved figure as a stand-in or uses a figure more than twice. To add an officer:
look for their figure in `[pages]`, else pick an unused figure of the same type, then check the crop
on a review sheet (the portraits are small; judge them at 1x and 2x).

## Drama backgrounds (`gfx/bg`)

| key | painting (region) |
|---|---|
| `palace` | 漢宮春曉圖, Qiu Ying — halls and balustrades |
| `town` | 清院本清明上河圖, section 14 — streets and shops |
| `village` | 清院本清明上河圖, section 03 — willows, a farmstead, travellers |
| `camp` | 萬樹園賜宴圖 — the yurt camp at Chengde |
| `field` | 康熙南巡圖 卷三 (Met) — open country, a road and riders |
| `river` | 武元直 赤壁圖, detail 5 — the river below the Red Cliff |
| `mountain` | 明皇幸蜀圖 — blue-green peaks and a mountain road |
| `castle` | 清院本清明上河圖, section 11 — city gate tower on the wall |
| `night` | 千里江山圖, section 1 — lake and mountains, graded to moonlight |
| `black` | plain black |

All are graded alike (see `build_backgrounds.py`) so white text stays readable on them.

## Music (`bgm`)

| key | track | use |
|---|---|---|
| `title` | Hitctrl — Views From Atop the Jade Kings Throne | title screen |
| `peace` | Kevin MacLeod — Shenyang | calm drama, towns |
| `tension` | Kevin MacLeod — Asian Drums | tense drama |
| `sad` | Kevin MacLeod — Nu Flute | sad scenes |
| `camp` | Kevin MacLeod — Ishikari Lore | camp / preparation |
| `battle` | Kevin MacLeod — Mountain Emperor | player phase |
| `enemy` | Majadroid — Samurai Nights (loop section) | enemy phase |
| `boss` | Kevin MacLeod — Five Armies | decisive battles |
| `victory` | Spring Spring — 10 fanfares (no. 2) | jingle |
| `defeat` | Joth — Death of a Ninja (Game Over) | jingle |
| `ending` | Spring Spring — Generic 2 minute Asian Arrangement | ending, "to be continued" |

The picks were made from the tracks' descriptions, instrumentation and a loudness/structure analysis,
not by listening; the cuts are listed in `build_music.py`.

## Fonts (`fonts`)

`Galmuri11.ttf` and `Galmuri9.ttf` are Galmuri with the CJK ideographs of the pack's text that Galmuri
lacks copied from Fusion Pixel Font (12 px and 10 px Korean builds, same pixel grid). The step scans
the pack's `*.toml`, `*.drama` and `credits.txt`, so rerun it when new Hanja appear in the text. A
character the donor lacks too stops the build unless its `FontJob` composes it from the pixel columns
of two glyphs Galmuri has (so far only 豨 in Galmuri9: 豕 of 豬 beside 希 of 稀). The
fonts are renamed "Galmuri11 ER" / "Galmuri9 ER" inside (OFL Modified Versions); the file names
stay because the game loads them by name.
