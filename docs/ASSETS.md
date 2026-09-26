# Media conventions

Every graphic, sound and font lives inside a data pack (`data/base/...`) and is referenced by **key**, never by
path, from rules and scripts. This file is the contract between the asset pipeline (`tools/assets/`), content
authors and the renderer. All paths below are relative to the pack directory. Only PNG images (no JPEG) and
OGG/WAV audio are supported by the engine.

## Screen model

The game draws in a **480×270 virtual coordinate space**. The renderer uses a render target of
`480·S × 270·S` real pixels, where `S` is the largest integer that fits the window (minimum 1), so:

* pixel art (tiles, units, UI skins, FX) is drawn with nearest filtering and scaled by exactly `S` — always crisp;
* text is rasterised at `S ×` its nominal size — crisp at every scale;
* high-resolution artwork (portraits, drama backgrounds, title art) keeps its detail up to `S` times the virtual size.

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

`terrain.png` is an atlas of 16×16 cells. `terrain.toml` maps a **tile key** (the terrain's `tile` field, default
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
anchor = [12, 23]     # frame pixel placed on the tile's bottom-centre pixel (8, 15)
```

Frames larger than the 16×16 tile overhang it upwards and sideways around the anchor. Every base pack sheet
uses 24×24 frames (the chariot 32×24, anchor `[16, 23]`); a 16×16 frame with anchor `[8, 15]` fits the tile
exactly.

## Portraits — `gfx/portraits/<key>.png`

Any resolution with a **4:5 aspect** (recommended 192×240), head-and-shoulders, drawn into a 64×80 virtual box.
`_unknown.png` is used for officers without a portrait. Officers default to their id as key.

## Drama backgrounds — `gfx/bg/<key>.png`

16:9 images (recommended 960×540) shown behind drama scenes. Keys used by the base pack:
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
| `title.png` | title screen artwork (16:9, recommended 960×540) |
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
