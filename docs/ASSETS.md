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
| `mountain` | `^` | 산지 mountain | special classes only |
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

One sheet per class sprite key and side colour: `side` ∈ `player` (blue), `ally` (green), `enemy` (red).
Layout (same as the Ninja Adventure character sheets): **4 columns = facing down, up, left, right**; rows:

| row | content |
|---|---|
| 0–3 | walk cycle (idle uses row 0) |
| 4 | attack pose |
| 5 | hurt pose |

```toml
[sprites.infantry]
frame = [16, 16]      # frame size in pixels
anchor = [8, 15]      # pixel of the frame that sits on the tile's bottom-centre (x = 8, y = 15 of the tile)
```

Frames larger than 16×16 (cavalry 24×24) overhang the tile upwards/sideways around the anchor.

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

| file | content |
|---|---|
| `window.png` + `ui.toml [window]` | nine-slice window skin (`border = 6` etc.) |
| `cursor.png` | 16×16 map cursor, 2 frames horizontally |
| `highlight.png` | 16×16 range overlays in a row: move (blue), attack (red), strategy (purple), heal (green), danger (orange) |
| `icons.png` + `ui.toml [icons]` | 16×16 icons by key (items, weather `clear`/`cloudy`/`rain`, stats, classes) |
| `pointer.png` | menu selection arrow |
| `title.png` | title screen artwork (16:9) |

## Audio — `bgm/<key>.ogg`, `sfx/<key>.(ogg|wav)`

BGM keys used by the engine and base pack: `title`, `peace`, `tension`, `sad`, `camp`, `battle`, `enemy`,
`boss`, `victory` (jingle), `defeat` (jingle), `ending`.
SFX keys used by the engine: `cursor`, `confirm`, `cancel`, `error`, `step`, `hit`, `hit_heavy`, `arrow`, `fire`,
`water`, `rock`, `heal`, `morale_up`, `morale_down`, `confuse`, `levelup`, `retreat`, `treasure`, `phase`,
`victory`, `defeat`.

## Fonts — `fonts/`

`Galmuri11.ttf` (UI and dialogue, nominal 12 px), `Galmuri9.ttf` (small numbers, nominal 10 px), `OFL.txt`.

## Licensing

Every third-party file must be listed in the repository's `CREDITS.md` with author, licence and source URL. Only
CC0, CC-BY, CC-BY-SA, OFL and public-domain material is allowed. `tools/assets/` records the source archive URL and
SHA-256 of everything it processes so the pack can be rebuilt.
