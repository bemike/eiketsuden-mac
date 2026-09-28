# Media conventions

Every graphic, sound and font lives inside a data pack (`data/base/...`) and is referenced by **key**, never by
path, from rules and scripts. This file is the contract between the asset pipeline (`tools/assets/`), content
authors and the renderer. All paths below are relative to the pack directory. Only PNG images (no JPEG) and
OGG/WAV audio are supported by the engine.

## Screen model

The game draws in a **virtual coordinate space** whose size the pack declares (the *presentation
profile*, below; 480×270 for the base pack). The renderer uses a render target of `W·S × H·S` real
pixels, where `S` is the largest integer that fits the window (minimum 1, and no side of the target
larger than 4096 pixels), so:

* pixel art (tiles, units, UI skins, FX) is drawn with nearest filtering and scaled by exactly `S` — always crisp;
* text is rasterised at `S ×` its nominal size — crisp at every scale;
* high-resolution artwork (portraits, drama backgrounds, title art) keeps its detail up to `S` times the virtual size.

A window smaller than the canvas shrinks it (with linear filtering, so text stays readable).

### Presentation profile

Everything that fixes the *scale* of the art is data, so a pack made for another screen (for example
one sized for the original game's 640×480 VGA screen, with bigger unit sprites than the base pack) is
laid out without engine changes:

| what | where | default |
|---|---|---|
| virtual canvas `[width, height]` | `pack.toml`: `[presentation] canvas = [640, 480]` | `[480, 270]` |
| map tile size (virtual pixels) | `gfx/tiles/terrain.toml`: `tile_size` | `16` |
| map picture size | `gfx/maps/<key>.png`: map tiles × `tile_size` | — |
| unit frame size and anchor | `gfx/units/units.toml`: `frame`, `anchor` per sprite | 16×16, anchor `[8, 15]` |
| effect frame size | `gfx/fx/fx.toml`: `frame` per effect | — |

```toml
# pack.toml
[presentation]
canvas = [640, 480]   # virtual canvas in pixels, each side within 320×200 ..= 1280×800
```

* **Canvas.** A pack whose canvas lies outside 320×200 ..= 1280×800 (each side on its own) does not load.
  Before a pack is loaded (loading, error and gallery screens) and when no pack of the chain sets
  `canvas` the canvas is 480×270. Every screen is laid out relative to the canvas: windows
  are centred or anchored to its edges, lists and panels grow with it, fonts keep their pixel size
  (so a bigger canvas shows more, not bigger, text). The camp and battle screens are laid out for at
  least **480×270**; smaller canvases are accepted, but those screens overlap and `Pack::validate`
  warns about it. Procedural backdrops (title, credits, missing drama backgrounds) are designed for
  a 270-pixel-high canvas and stretch vertically with the canvas height.
* **Tile size.** The battle map, camera, cursor, range highlights, unit placement, floating numbers,
  effects and pointer hit-testing use the tileset's `tile_size`: one atlas pixel is one virtual
  pixel. Maps are drawn with flat colours at 16 pixels per tile when the tileset is missing. Motion
  of the battle animation (lunges, knock-back) is designed for 16-pixel tiles and scales with the
  tile size.
* **Units.** A unit frame's `anchor` pixel is placed on the tile's bottom-centre pixel —
  `(T/2, T−1)` for `T`-pixel tiles, `(8, 15)` for the base pack. The shadow and the HP bar follow the
  tile; the commander flag, lord crown and confusion stars sit above the top of the unit's frame.
  In the camp, unit icons taller than 32 pixels are drawn at 1/2 (1/3, ...) so the officer rows keep
  their spacing.
* **Portraits and backgrounds** are drawn into boxes in virtual pixels (4:5 portrait boxes whose size
  depends on the screen, see below; the whole canvas for backgrounds with `Cover` fitting),
  independent of their file resolution.
* **Fixed UI sizes (not data).** These are set by the engine, not by the pack:
  * icons (`icons.png`): drawn 16×16 whatever the atlas cell size in `icons.toml`;
  * banners (`flags.png`): cells and drawn size 16×16 (larger cells are cut off);
  * portrait boxes per screen: 64×80 in the dialogue and message boxes and the camp's deploy and
    equipment screens, 96×120 on the officer page, 44×55 in drama notices, 38×47 and 36×45 in the
    battle HUD; drama stage portraits are up to 104×130 and shrink (keeping 4:5) on low or narrow
    canvases;
  * the fonts' pixel sizes.

  A pack made for another resolution changes the canvas and the tile size; the UI keeps these
  sizes, so a bigger canvas shows more, not bigger, UI (like the fonts). Larger portrait files
  still help: they are scaled into their box, so they keep detail at window scales above 1. These
  sizes would become `[presentation]` fields (inherited field by field, `docs/DECISIONS.md` D8)
  only when a pack needs a different one; none does yet.

## Canonical terrain ids

Battle maps use these terrain ids (defined in `rules/terrain.toml`, glyph in brackets). The tileset must provide a
tile key for each.

| id | glyph | meaning | passable |
|---|---|---|---|
| `plain` | `.` | 평지 plain | yes |
| `grass` | `,` | 초원 grassland | yes |
| `road` | `_` | 길 road (plain rules, road look) | yes |
| `forest` | `T` | 숲 forest | yes (not cavalry) |
| `mountain` | `^` | 산지 mountain | `mountain` move type only (bandit line, martial artists, beast tamers, tribesmen) |
| `wasteland` | `:` | 황무지 wasteland | yes (slow for cavalry/supply) |
| `bridge` | `=` | 다리 bridge | yes |
| `castle` | `c` | 성내 castle floor | yes |
| `gate` | `G` | 성문 open city gate (castle rules, gate look) | yes |
| `village` | `v` | 마을 village (heals) | yes |
| `barracks` | `b` | 병영 barracks (heals) | yes |
| `fort` | `f` | 성채 fort (heals, defence 30) | yes |
| `granary` | `g` | 군량고 granary | yes |
| `treasury` | `$` | 보물고 treasury | yes |
| `river` | `~` | 강 river / lake water | no |
| `wall` | `#` | 성벽 castle wall | no |
| `cliff` | `X` | 절벽 cliff | no |
| `house` | `H` | 민가 house / building | no |
| `fence` | `\|` | 목책 palisade | no |

## Terrain tileset — `gfx/tiles/terrain.png` + `gfx/tiles/terrain.toml`

`terrain.png` is an atlas of `tile_size`×`tile_size` cells (16×16 in the base pack; the cell size is also the size
of a map tile on screen, see [Presentation profile](#presentation-profile)). `terrain.toml` maps a **tile key** (the terrain's `tile` field, default
its id) to a stack of layers drawn bottom to top:

```toml
tile_size = 16
image = "terrain.png"

[tiles.grass]
layers = [
  { cells = [[0, 0], [1, 0], [2, 0]] },          # variant picked by a hash of the map position
]

[tiles.forest]
layers = [
  { cells = [[0, 0]] },                           # grass underneath
  { cells = [[5, 3], [6, 3]] },                   # tree on top
]

[tiles.river]
layers = [
  { auto = [[0, 8], [1, 8], ...16 cells...], connect = ["river", "bridge"] },
]
```

* `cells` — list of `[column, row]` atlas cells; one is chosen per map position with a stable hash so maps look
  varied but never flicker.
* `auto` — exactly 16 cells indexed by a 4-bit mask of orthogonal neighbours whose terrain id is in `connect`
  (bit 1 = north, 2 = east, 4 = south, 8 = west). Out-of-map neighbours count as connected.
* A layer may also carry `offset = [dx, dy]` (pixels) for objects that overhang their tile, and `fps` + `frames`
  (list of cell lists) for animated water.

## Map pictures — `gfx/maps/<key>.png`

A map with `image = "<key>"` (in a battle's `[map]` or a map file entry, see
[MODDING.md](MODDING.md#map)) is drawn from this one picture instead of the terrain tileset: the
**picture layer**. The map's `rows` stay the rules layer, so the picture has to line up with them:

* Size: exactly `width × tile_size` by `height × tile_size` pixels, where `width`/`height` are the
  map's size in tiles and `tile_size` is the tileset's (`gfx/tiles/terrain.toml`, 16 without one).
  Tile `(x, y)` is the square at `(x·T, y·T)`. The tileset still sets the tile size, the camera and
  everything drawn on the map, so a pack with picture maps keeps a `terrain.toml`.
* One picture pixel is one virtual pixel, drawn with nearest filtering like the tiles.
* No animation: animated tileset layers (water) are not drawn on a picture map.
* A picture that is missing or of another size is not drawn; the game logs a warning and uses the
  tileset. `hero-tools validate` reports both as errors.

The original mode writes one per converted original battle map (`gfx/maps/hexz_NN.png`, 32-px tiles;
[ORIGINAL_DATA.md](ORIGINAL_DATA.md#45-원작-모드-팩을-파일로-만들기-개발검증용)).

## Unit sprites — `gfx/units/<sprite>_<side>.png` + `gfx/units/units.toml`

The sprite key of a class is its `sprite` field; the base pack uses the class id. Base pack class ids:
`short_infantry` 단병, `long_infantry` 장병, `chariot` 전차, `light_cavalry` 경기병, `heavy_cavalry` 중기병,
`guard_cavalry` 친위대, `archer` 궁병, `crossbow` 연노병, `catapult` 발석차, `bandit` 산적, `brigand` 흉적,
`outlaw` 의적, `martial` 무도가, `tribe` 이민족, `beast` 맹수사, `supply` 수송대, `band` 군악대, `sorcerer` 주술사,
`civilian` 백성.

One sheet per sprite key and side colour: `side` ∈ `player` (blue), `ally` (green), `enemy` (red).
Layout (same as the Ninja Adventure character sheets): **4 columns = facing down, up, left, right**; rows:

| row | content |
|---|---|
| 0–3 | walk cycle (idle uses row 0) |
| 4 | attack pose |
| 5 | hurt pose |

```toml
[sprites.short_infantry]
frame = [24, 24]      # frame size in pixels
anchor = [12, 23]     # frame pixel placed on the tile's bottom-centre pixel, (8, 15) on 16-pixel tiles
```

Frames larger than the tile overhang it upwards and sideways around the anchor. Every base pack sheet
uses 24×24 frames (the chariot 32×24, anchor `[16, 23]`); a 16×16 frame with anchor `[8, 15]` fits a 16-pixel
tile exactly. On `T`-pixel tiles the anchor lands on the tile pixel `(T/2, T−1)`.

## Portraits — `gfx/portraits/<key>.png`

Any resolution with a **4:5 aspect** (recommended 192×240), head-and-shoulders, drawn into a 64×80 virtual box.
`_unknown.png` is used for officers without a portrait. Officers default to their id as key.

## Drama backgrounds — `gfx/bg/<key>.png`

16:9 images (recommended 960×540) shown behind drama scenes, scaled to cover the canvas (on a 4:3 canvas such as
640×480 the sides are cropped; packs for such a canvas may ship 4:3 art). Keys used by the base pack:
`palace`, `town`, `village`, `camp`, `field`, `river`, `mountain`, `castle`, `night`, `black` (plain black is also
implied by `@bg none`).

## Effects — `gfx/fx/<key>.png` + `gfx/fx/fx.toml`

Horizontal frame strips. `fx.toml`: `[fx.fire] frame = [32, 32]  frames = 8  fps = 12`. Keys referenced by
strategies' `fx` and by the renderer: `slash`, `arrow`, `fire`, `water`, `rock`, `heal`, `morale_up`, `morale_down`,
`confuse`, `levelup`.

## UI — `gfx/ui/`

Windows, menus, the map cursor and range highlights are drawn procedurally by the engine (blue bevelled windows
in the spirit of the original PC version), so a pack only supplies:

| file | content |
|---|---|
| `icons.png` + `icons.toml` | 16×16 icons by key: `[icons] bean = [0, 0]` (column, row). Keys used by the engine: `gold`, `weather_clear`, `weather_cloudy`, `weather_rain`, `hp`, `mp`, `morale`, `atk`, `def`, `move`, `exp`, `weapon`, `armor`, `accessory`, `consumable`, `fire`, `water`, `earth`, `heal`, `morale_up`, `morale_down`, `confuse`, `lord`, `commander`; items and strategies may use any other key. |
| `title.png` | title screen artwork (16:9, recommended 960×540; covers the canvas like drama backgrounds) |
| `flags.png` | 16×16 animated banner, 4 frames horizontally, per side in rows: player, ally, enemy (drawn beside commanders) |

## Audio — `bgm/<key>.ogg`, `sfx/<key>.(ogg|wav)`

BGM keys used by the engine and base pack: `title`, `peace`, `tension`, `sad`, `camp`, `battle`, `enemy`,
`boss`, `victory` (jingle), `defeat` (jingle), `ending`.
SFX keys used by the engine: `cursor`, `confirm`, `cancel`, `error`, `step`, `hit`, `hit_heavy`, `arrow`, `fire`,
`water`, `rock`, `heal`, `morale_up`, `morale_down`, `confuse`, `levelup`, `retreat`, `treasure`, `phase`,
`victory`, `defeat`.

## Fonts — `fonts/`

`Galmuri11.ttf` (UI and dialogue, nominal 12 px), `Galmuri9.ttf` (small numbers, nominal 10 px), `OFL.txt`
(Galmuri's licence) and `OFL-FusionPixel.txt` (the licences of Fusion Pixel Font and the fonts it is built from).
The base pack's two fonts are OFL Modified Versions that contain Fusion Pixel glyphs, so both licence files must
travel with them: a pack that copies `fonts/` copies all four files.

## Licensing

Every third-party file must be listed in the repository's `CREDITS.md` with author, licence and source URL. Only
CC0, CC-BY, CC-BY-SA, OFL and public-domain material is allowed. `tools/assets/` records the source archive URL and
SHA-256 of everything it processes so the pack can be rebuilt.
