# Modding guide: the data pack format

Everything the player sees in Eiketsuden Reloaded comes from a **data pack**: rules, classes, items,
officers, battle maps, the campaign, the story scenes, graphics and music. The engine contains no
content. This guide is the complete reference for writing or changing a pack.

* The **formulas** that use the numbers below are in [RULES.md](RULES.md).
* The **media conventions** (image sizes, sprite sheet layouts, atlas files) are in [ASSETS.md](ASSETS.md).
* The small test pack in [`crates/hero-core/tests/fixtures/mini/`](../crates/hero-core/tests/fixtures/mini/)
  uses almost every feature and passes `Pack::validate` (stages 1 and 2 of [Validation](#validation))
  without a single warning; it is a good place to copy the data files from. It ships no media, so
  `hero-tools validate` also reports its missing media files (stage 3) and fails on them; copy `gfx/`,
  `bgm/`, `sfx/` and `fonts/` from `data/base` (or make your own) for a pack that passes the tool too.
  [`mini_ext`](../crates/hero-core/tests/fixtures/mini_ext/) next to it is a [layered pack](#layered-packs-extends)
  on top of `mini`: it replaces `rules/game.toml` and the campaign, adds a battle and replaces a scene.

Contents: [Quick start](#quick-start) · [Conventions](#conventions) · [pack.toml](#packtoml) ·
[Layered packs](#layered-packs-extends) ·
[rules/game.toml](#rulesgametoml) · [rules/terrain.toml](#rulesterraintoml) · [Ranges](#ranges) ·
[rules/classes.toml](#rulesclassestoml) · [Effects](#effects) · [rules/strategies.toml](#rulesstrategiestoml) ·
[rules/items.toml](#rulesitemstoml) · [officers.toml](#officerstoml) · [Battles](#battles) ·
[campaign.toml](#campaigntoml) · [Drama scripts](#drama-scripts-drama) · [Flags](#flags) · [Media](#media) ·
[Validation](#validation) · [hero-tools](#hero-tools)

## Quick start

A pack is a directory:

```
my_pack/
├── pack.toml              manifest: names every text file below
├── rules/
│   ├── game.toml          global numbers (level cap, EXP tables, weather ...)
│   ├── terrain.toml       [[terrain]] tiles and movement costs
│   ├── classes.toml       [[class]] unit classes
│   ├── strategies.toml    [[strategy]] spells (책략)
│   └── items.toml         [[item]] equipment and consumables
├── officers.toml          [[officer]] named characters
├── campaign.toml          the story graph: [[node]] ...
├── battles/*.toml         one battle per file
├── dramas/*.drama         dialogue scenes
├── gfx/ bgm/ sfx/ fonts/  media, referenced by key (see ASSETS.md)
```

The file names and folders are free: `pack.toml` lists every text file, so only `pack.toml` itself has a
fixed name. Media files follow the fixed layout of [ASSETS.md](ASSETS.md).

A pack does not have to be complete: with `extends = "../base"` it is built on another pack and lists
only what it adds or replaces (see [Layered packs](#layered-packs-extends)). That is the easiest way to
make a balance mod, add a battle or restyle the media of an existing pack.

Typical workflow:

```sh
hero-tools validate my_pack          # load + cross-check everything, list errors and warnings
hero-tools info my_pack              # content summary
hero-tools simulate my_pack --seeds 8   # let the AI fight every battle, find unwinnable maps
eiketsuden --data my_pack            # play it (or set EIKETSUDEN_DATA=my_pack)
```

## Conventions

* **Encoding**: all text files are UTF-8. A byte order mark is accepted and ignored.
* **TOML**: every data file except the `.drama` scripts is [TOML 1.0](https://toml.io). Missing optional
  fields take the default given in the tables below (`false`, `0`, empty, or "none").
* **Ids** are the strings that other files use to refer to a thing: `id = "guan_yu"`. Use lowercase ASCII,
  digits and `_` (`short_infantry`, `b03`, `ch2_camp`). Ids must be unique within their kind (two classes
  cannot share an id; a class and an item can, but avoid it). Display names (`name = "관우"`) are free text.
* **Positions** are `[x, y]` arrays: `x` is the column and `y` the row, counted from `[0, 0]` at the top-left
  tile of the map.
* **Unknown keys**: the list files reject a misspelt table name (`[[classes]]` instead of `[[class]]`) with an
  error. A misspelt field inside a table (`can_couter = true`) would silently fall back to its default, so
  `hero-tools validate` reports every key it does not know as a warning.
* **Saves** store the pack `id`, officer ids, item ids, class ids and campaign node ids. Renaming any of
  them in a released pack breaks existing save games; change the pack `id` if you must.

## pack.toml

```toml
id = "base"                       # machine id; save games remember it
name = "영걸전 Reloaded 기본 팩"
version = "1.0.0"
authors = ["Eiketsuden Reloaded contributors"]
license = "CC-BY-SA-4.0 (text) / see CREDITS.md (media)"
description = "The Romance of the Three Kingdoms campaign, told again from Liu Bei's side."
officers = "officers.toml"
campaign = "campaign.toml"
battles = ["battles/b01.toml", "battles/b02.toml"]
dramas = ["dramas/ch1.drama", "dramas/battles.drama"]

[rules]
game = "rules/game.toml"
terrain = "rules/terrain.toml"
classes = "rules/classes.toml"
strategies = "rules/strategies.toml"
items = "rules/items.toml"
```

| field | type | required | meaning |
|---|---|---|---|
| `id` | string | yes | Machine id of the pack. Every pack of a [chain](#layered-packs-extends) needs its own id. |
| `name` | string | yes | Display name. |
| `version` | string | yes | Pack version (free form, e.g. `1.2.0`). |
| `authors` | list of strings | no | Credits. |
| `license` | string | no | Licence summary of the content (every media file also goes into `CREDITS.md`). |
| `description` | string | no | One or two sentences for the pack list. |
| `extends` | directory | no | The pack this one is built on, relative to this pack's directory (`../base`). See [Layered packs](#layered-packs-extends). |
| `rules.game` / `.terrain` / `.classes` / `.strategies` / `.items` | path | yes, unless `extends` | The five rules files. |
| `officers` | path | yes, unless `extends` | Officer list. |
| `campaign` | path | yes, unless `extends` | Campaign graph. |
| `battles` | list of paths | no (`[]`) | Battle files, one battle each. |
| `dramas` | list of paths | no (`[]`) | Drama scripts, any number of scenes each. |
| `maps` | list of paths | no (`[]`) | [Map files](#map-files): maps that battles `use` by id. |
| `presentation.canvas` | `[width, height]` | no (`[480, 270]`) | Size in pixels of the virtual canvas the game draws on, from `[320, 200]` to `[1280, 800]`. Media is laid out for this size (the base pack: 16 px tiles, 30 × 17 visible). |

```toml
[presentation]
canvas = [640, 480]      # e.g. a pack made from the 640x480 original game
```

Paths are relative to the pack directory, use `/`, must not contain `..`, `\` or `:`, and no file may be
listed twice. The web build fetches exactly these files (and those of the packs it extends), so a file
that is not listed is never loaded.

## Layered packs (`extends`)

A pack that says `extends = "<directory>"` is a **child** built on top of its **parent**, the pack in
that directory; the parent may extend another pack in turn. The packs involved form a **chain**, from
the pack that is loaded (the **top** pack) down to the pack without `extends`.

```toml
# mods/balance/pack.toml: other item prices and rewritten scenes, everything else from the base pack
id = "balance"
name = "Balance mod"
version = "0.1.0"
extends = "../../data/base"          # mods/balance/../../data/base = data/base
dramas = ["dramas/rewrites.drama"]   # scenes with the ids of base scenes replace them

[rules]
items = "rules/items.toml"           # replaces the base items.toml as a whole
```

What the chain provides:

| part | rule |
|---|---|
| `rules.game`, `.terrain`, `.classes`, `.strategies`, `.items`, `officers`, `campaign` | Each comes from the **nearest** pack that lists it, starting with the top pack: a child's file **replaces** its parent's as a whole (there is no merging inside a file, so a child `items.toml` must hold every item the pack needs). Some pack of the chain must list each of them. |
| `battles` | The **union** of every pack's battle files. A battle whose `id` a nearer pack defines again **overrides** the farther pack's battle with that id. Within one pack a battle id must still be unique. |
| `dramas` | The same for scene ids: every pack's scenes, a nearer pack's scene replacing a farther pack's scene with the same id (`== b01_outro` in a child replaces the parent's `b01_outro`). |
| `maps` | The same for map ids. A battle's `use` is resolved after the whole chain is merged, so a child's map with the id of a parent's map also replaces it in the parent's battles (a mod can redraw a map without copying the battles). |
| `[presentation]` | Inherited: the nearest pack that has a `[presentation]` table decides; without any, `[480, 270]`. |
| media (`gfx/`, `bgm/`, `sfx/`, `fonts/`, `credits.txt`) | Every media file is looked up in the top pack first, then in each parent in chain order; the first pack that has the file wins. This includes the media index files `gfx/units/units.toml`, `gfx/tiles/terrain.toml`, `gfx/fx/fx.toml` and `gfx/ui/icons.toml`: a child's index **replaces** its parent's as a whole, so copy the entries you keep. |
| `id`, `name`, `version`, `authors`, `license`, `description` | The top pack's. Save games remember the top pack's `id`, so saves of the parent pack do not load in the child and vice versa; each pack also has its own save slots (autosave included), so playing one pack never overwrites another's saves. |

Rules of the chain (all errors when loading):

* `extends` is a directory relative to the pack that names it, written with `/` (no `\`, no `:`, not
  absolute). The chain's paths are computed **lexically**, like URLs: `..` removes the previous path
  segment, so `extends = "../base"` in `mods/balance` means `mods/base`. Natively the operating system
  then opens those paths, which gives the same files unless a directory on the way is a symbolic link
  (Unix systems resolve `..` after a link from the link's target); keep links out of pack paths.
* A chain holds at most **4 packs** (a pack and three it builds on, directly or indirectly).
* A pack cannot extend itself, directly or through other packs, and two packs of a chain cannot share
  an `id`.
* A parent directory without a readable `pack.toml` is an error that names the `extends` line.
* Error messages and validation contexts name files by their path relative to the top pack:
  `../base/rules/classes.toml` is the parent's classes file.

The game reads `pack.toml` of the top pack, then of each parent, then every other text file of the
chain; natively and in the browser alike.

**Layered packs in the web build.** The browser always loads the top pack from the fixed URL
`data/base/` next to `index.html`, and resolves `extends` from there like any relative URL. So on the
web:

* The child pack sits in `<site>/data/base/`, whatever its directory is called natively.
* Its parent must sit in a **sibling** directory with another name, for example `<site>/data/vanilla/`
  with `extends = "../vanilla"` in the child. `extends = "../base"` (or the `"../../data/base"` of the
  example above) points back at `data/base/`, the child itself, and the pack fails to load.
* `tools/web/build.sh --data <pack>` and `build.ps1 -Data <pack>` copy **only** that one directory,
  to `<out>/data/base/`; they do not copy the packs it extends. Copy each parent into the output
  folder yourself (after the script, before serving or uploading), at the path its child's `extends`
  names.

Natively the same child works from any directory whose `extends` path reaches the parent, so a mod
meant for both keeps its parent at a name other than `base` in both layouts (for example
`data/vanilla/` next to `data/balance/`). One further limit of the web build: the battle screen's media index files
(`units.toml`, `terrain.toml`, `fx.toml`) and `credits.txt` are read from the top pack only, because the
browser cannot check whether a file exists without fetching it; natively they come from the first
pack that has them. A child pack meant for the web ships its own copies of those files.

`hero-tools validate`, `info` and `simulate` take the top pack directory and work on the whole chain.
`validate` checks unknown keys in every `pack.toml`, in the rules, officer and campaign files in use (a
parent's file that the child replaces is not checked) and in every battle and map file of the chain,
and it looks for media in every pack of the chain.

## rules/game.toml

Global numbers. Every field is required except `affinity`.

```toml
level_cap = 50
exp_per_level = 100
gold_cap = 99999
mp_cap = 200
morale_start = 100
morale_loss_pct = 50
confuse_morale = 30
exp_attack = [[-99, 1], [-5, 4], [0, 8], [5, 12]]
exp_kill = [[-99, 4], [-5, 16], [0, 32], [5, 48]]
exp_commander = 20
exp_support = 8
counter_divisor = 150
counter_damage_pct = 50

[weather]
clear = 60
cloudy = 25
rain = 15

[affinity.infantry]          # attacker family
archer = 75                  # defender family = DEF multiplier in percent
cavalry = 125
```

| field | type | meaning |
|---|---|---|
| `level_cap` | integer ≥ 1 | Highest level. |
| `exp_per_level` | integer ≥ 1 | EXP per level; the remainder is kept on level up. |
| `gold_cap` | integer ≥ 0 | Most gold the army can hold; gold is always clamped to `0..=gold_cap`. |
| `mp_cap` | integer ≥ 0 | Upper bound for MP. |
| `morale_start` | 0..=100 | Morale of every unit at the start of a battle. |
| `morale_loss_pct` | integer ≥ 0 | Morale lost when hit = `ceil(damage * morale_loss_pct / max_hp)`. |
| `confuse_morale` | 0..=100 | At or below this morale a unit may become confused (RULES.md §6). |
| `exp_attack` | list of `[diff, exp]` | EXP for damaging a unit by `target level - own level`. Sorted by strictly increasing `diff`; the last pair whose `diff` ≤ the actual difference applies (the first pair applies below it). |
| `exp_kill` | list of `[diff, exp]` | EXP for defeating a unit (same lookup). |
| `exp_commander` | integer | Extra EXP for defeating an enemy commander. |
| `exp_support` | integer | EXP for a successful heal/morale/confusion strategy (classes can override with `support_exp`). |
| `affinity` | table of tables | `affinity.<attacker family>.<defender family> = percent`: the defender's DEF is multiplied by it. Missing pairs mean 100. The PC original uses 75 for an advantage and 125 for a disadvantage. |
| `counter_divisor` | integer > 0 | Counter-attack chance in percent = `STR * 100 / counter_divisor`. |
| `counter_damage_pct` | integer ≥ 0 | Counter damage in percent of a normal attack. |
| `weather.clear` / `.cloudy` / `.rain` | integers ≥ 0, sum 100 | Chance of each weather, rolled every turn. |

## rules/terrain.toml

One `[[terrain]]` table per terrain type, in display order. The canonical base pack terrain ids and glyphs
are listed in [ASSETS.md](ASSETS.md#canonical-terrain-ids).

```toml
[[terrain]]
id = "forest"
name = "숲"
glyph = "T"
defense = 20
elements = ["fire", "earth"]
boost = ["fire"]
cost = { foot = 2, mountain = 1 }    # no `horse` entry: cavalry cannot enter

[[terrain]]
id = "village"
name = "마을"
glyph = "v"
defense = 10
heal_hp = 10
heal_morale = 5
elements = ["fire"]
cost = { foot = 1, horse = 1, mountain = 1 }

[[terrain]]
id = "river"
name = "강"
glyph = "~"                          # no cost at all: impassable for everyone
```

| field | type | default | meaning |
|---|---|---|---|
| `id` | id | required | Terrain id. |
| `name` | string | required | Display name. |
| `glyph` | one character | required | Character used for this terrain in battle maps. Unique; not a space. |
| `defense` | 0..=100 | 0 | Percent of physical damage removed while standing here. |
| `heal_hp` | 0..=100 | 0 | Percent of max HP restored at the start of the occupant's phase. |
| `heal_morale` | 0..=100 | 0 | Morale restored at the start of the occupant's phase. |
| `elements` | list of ids | `[]` | Strategy elements that can hit a unit standing here (`fire`, `water`, `earth` ...). |
| `boost` | list of ids | `[]` | Elements that deal +25% damage here (fire in a forest). Must also be in `elements`. |
| `cost` | table move type → 1..=255 | `{}` | Movement points to enter, per **move type**. A move type without an entry cannot enter. |
| `tile` | string | the id | Tile key in `gfx/tiles/terrain.toml` (several terrain types can share a look). |

**Move types** are free ids that connect classes to terrain: a class has one `move_type`, and each terrain
lists what it costs for each move type. The base pack uses `foot` (infantry and archer lines, sorcerers,
civilians), `horse` (cavalry line), `mountain` (bandit line, martial artists, beast tamers, tribesmen: the
only move type that may climb mountains) and `slow` (supply wagons, military bands); the mini test pack
calls its mountain type `special`. A class whose move type has a cost on no terrain is a validation error
(its units could never move). Deploy slots must be passable for the move type `foot` when the pack defines it.

## Ranges

Attack ranges (`class.range`) and strategy reach (`strategy.range`) use the same notation: either a named
shape or an explicit list of `[dx, dy]` offsets from the unit (`range = [[0, -1], [0, 1], [-1, 0], [1, 0]]`).

| name | tiles | typical use |
|---|---|---|
| `adjacent4` | the 4 orthogonal neighbours | infantry, cavalry, bandits |
| `adjacent8` | the 8 surrounding tiles | long infantry, guards |
| `archer` | every tile at walking distance exactly 2 | archers |
| `crossbow` | distance 2–3 within a 5×5 square | crossbowmen |
| `catapult` | `crossbow` plus the corners of the 5×5 square and the distance-3 cross | catapults |
| `self` | the unit's own tile | self-only strategies |
| `range8` | the 3×3 square (own tile included) | short strategies |
| `range12` | `range8` plus the four tiles two steps straight out | most strategies |
| `range20` | the 5×5 square without its corners | long strategies |

"Distance" is the Manhattan distance `|dx| + |dy|`.

## rules/classes.toml

One `[[class]]` table per class. The base pack class ids are listed in [ASSETS.md](ASSETS.md#unit-sprites--gfxunitssprite_sidepng--gfxunitsunitstoml).

```toml
[[class]]
id = "short_infantry"
name = "단병"
hanja = "短兵"
family = "infantry"
tier = 1
move = 4
move_type = "foot"
range = "adjacent4"
atk = 8
def = 8
hp = 500
hp_growth = 50
generic = [40, 30, 50]
sprite = "short_infantry"
strategies = [{ level = 6, id = "fire" }]
promote = { to = "long_infantry", level = 15, item = "long_spear" }
desc = "칼과 방패를 든 보병."
```

| field | type | default | meaning |
|---|---|---|---|
| `id` | id | required | Class id. |
| `name` | string | required | Display name. |
| `hanja` | string | `""` | Hanja spelling for the status window. |
| `family` | id | required | Class family: used by `affinity` and by item `families`. Free ids (`infantry`, `cavalry`, `archer`, `bandit`, `support` ...). |
| `tier` | 1..=3 | required | 1 = basic, 2 = first promotion, 3 = final. |
| `move` | integer | required | Movement points. |
| `move_type` | id | required | Key into terrain `cost`; must have a cost on at least one terrain. |
| `range` | [range](#ranges) | required | Attack range. |
| `atk` / `def` | integer | required | Class attack/defence corrections (PC original: 4..16). |
| `hp` | integer > 0 | required | Max HP (troops) at level 1. |
| `hp_growth` | integer | required | Max HP gained per level. |
| `generic` | `[str, int, lead]` | required | Stats of generic (unnamed) units of this class. |
| `strategies` | list of `{ level, id }` | `[]` | Strategies learned at a level. A promoted class also knows the lists of every class before it in its promotion chain. |
| `promote` | `{ to, level, item }` | none | Promotion: using the consumable `item` on an officer of at least `level` changes the class to `to` (in camp). `item` must have a [`promote` effect](#effects). |
| `sprite` | string | required | Unit sprite key: `gfx/units/<sprite>_<side>.png`. |
| `can_counter` | bool | `false` | Can counter-attack adjacent melee attackers. |
| `provokes_counter` | bool | `false` | Attacks of this class can provoke counter-attacks. |
| `strategy_guard` | bool | `false` | Takes half strategy damage and resists strategies as if INT were doubled. |
| `mp_aura` | bool | `false` | Military band: orthogonally adjacent units regain `level / 10 + 1` MP per phase. |
| `support_exp` | integer | none | EXP for single-target support strategies cast by this class (overrides `exp_support`). |
| `desc` | string | `""` | Help text. |

Promotion chains must not loop (`a → b → a`). If two classes promote into the same class, inherited
strategies are taken from only one of them, so give every promoted class a single predecessor.

## Effects

Strategies and consumable items list their effects as inline tables with a `type`:

| effect | fields | on strategies | on items |
|---|---|---|---|
| `{ type = "damage", power = 60 }` | `power` | strategy damage (RULES.md §5) | not allowed (give the item a `strategy`) |
| `{ type = "heal", power = 100 }` | `power` | heal scaled by caster INT/level | restores exactly `power` HP |
| `{ type = "morale", amount = 20 }` | `amount` (negative = morale down) | morale change (negative needs a hit) | morale change |
| `{ type = "status", status = "confused", turns = 2 }` | `status`, `turns` | inflict a status on hit | not allowed |
| `{ type = "promote" }` | — | not allowed | class-up item, used in camp |
| `{ type = "change_class", to = "archer" }` | `to` (class id) | not allowed | class-change item, used in camp |

The only status is `confused` (cannot move or act).

## rules/strategies.toml

```toml
[[strategy]]
id = "fire"
name = "초열"
hanja = "焦熱"
kind = "attack"
mp = 4
range = "range12"
area = "single"
target = "enemy"
element = "fire"
effects = [{ type = "damage", power = 60 }]
fx = "fire"
desc = "적 하나를 불길로 공격한다. 강·성 안에서는 쓸 수 없다."

[[strategy]]
id = "great_heal"
name = "대원조"
kind = "heal"
mp = 16
range = "range8"
area = "all_in_range"
target = "ally"
effects = [{ type = "heal", power = 120 }]
fx = "heal"
```

| field | type | default | meaning |
|---|---|---|---|
| `id` / `name` / `hanja` | | | as for classes |
| `kind` | `attack` · `heal` · `support` | required | Menu grouping and AI hint. |
| `mp` | integer ≥ 0 | required | MP cost. |
| `range` | [range](#ranges) | required | Tiles the strategy can be aimed at, around the caster. |
| `area` | `single` · `cross` · `all_in_range` | required | `single`: the unit on the aimed tile; `cross`: the aimed tile and its 4 neighbours; `all_in_range`: every valid target in reach, no aiming. |
| `target` | `enemy` · `ally` | required | `ally` = the caster's side and its friends, caster included. |
| `element` | id | none | The target tile must list this element in its terrain `elements`; `fire` is impossible in rain and `water` is boosted by rain. Without an element the strategy works anywhere. |
| `effects` | list of [effects](#effects) | required | What happens to each affected unit. |
| `fx` | string | `""` | Effect animation key (`gfx/fx/fx.toml`). |
| `desc` | string | `""` | Help text. |

## rules/items.toml

```toml
[[item]]
id = "bean"
name = "콩"
kind = "consumable"
price = 20
effects = [{ type = "heal", power = 200 }]
battle_use = true
icon = "bean"

[[item]]
id = "fire_scroll"                 # casts a strategy without MP
name = "초열서"
kind = "consumable"
strategy = "fire"
battle_use = true

[[item]]
id = "long_spear"                  # class-up item (see class `promote`)
name = "장창"
kind = "consumable"
price = 300
effects = [{ type = "promote" }]

[[item]]
id = "bronze_sword"
name = "청동검"
kind = "weapon"
price = 200
families = ["infantry", "bandit"]
atk_pct = 110

[[item]]
id = "red_horse"
name = "적토마"
kind = "accessory"                 # price 0: treasure, cannot be bought or sold
move_bonus = 1
regen_morale = 5
```

| field | type | default | meaning |
|---|---|---|---|
| `id` / `name` / `hanja` | | | as for classes |
| `kind` | `weapon` · `armor` · `accessory` · `consumable` | required | Equipment slot, or consumable. |
| `price` | integer | 0 | Shop price. 0 = cannot be bought or sold (treasures, event items). Sells for half. |
| `desc` | string | `""` | Help text. |
| `families` | list of family ids | `[]` | Class families that may equip it; empty = everyone. Equipment only. |
| `atk_pct` | percent | 0 | Weapon: ATK × `atk_pct / 100` (110 = +10%). 0 = no bonus. |
| `def_pct` | percent | 0 | Armor (war manual): DEF × `def_pct / 100`. |
| `move_bonus` | integer | 0 | Accessory (horse): extra movement points. |
| `regen_hp` | percent | 0 | Equipment: percent of max HP regenerated each own phase. |
| `regen_morale` | integer | 0 | Equipment: morale regenerated each own phase. |
| `effects` | list of [effects](#effects) | `[]` | Consumable: applied to the target (heal/morale in battle, promote/change_class in camp). |
| `strategy` | strategy id | none | Consumable: casts this strategy from the user's tile without MP, instead of `effects`. |
| `battle_use` | bool | `false` | Consumable usable in battle (on the user or an adjacent friend). Class items are used in camp and leave it `false`. |
| `icon` | string | `""` | Icon key in `gfx/ui/icons.toml`. |

Each officer has one weapon, one armor and one accessory slot.

## officers.toml

```toml
[[officer]]
id = "liu_bei"
name = "유비"
hanja = "劉備"
courtesy = "현덕"
class = "short_infantry"
level = 5
str = 72
int = 75
lead = 78
lord = true
fixed_class = true
equip = { weapon = "bronze_sword" }
bio = "탁현 출신. 백성을 아끼는 인물."

[[officer]]
id = "jian_yong"
name = "간옹"
class = "sorcerer"
level = 4
str = 30
int = 70
lead = 40
portrait = "jianyong"              # uses gfx/portraits/jianyong.png
```

| field | type | default | meaning |
|---|---|---|---|
| `id` | id | required | Officer id (used by battles, dramas and the campaign). |
| `name` | string | required | Display name (Korean). Drama speakers may use it instead of the id. |
| `hanja` | string | `""` | Hanja name. |
| `courtesy` | string | `""` | Courtesy name (字). |
| `class` | class id | required | Class when the officer joins or appears. |
| `level` | 1..=`level_cap` | required | Level when the officer joins (player) or appears (enemy/ally default). |
| `str` / `int` / `lead` | 0..=100 | required | 무력 (attack, counter chance), 지력 (MP, strategies), 통솔 (defence). |
| `portrait` | string | the id | Portrait key: `gfx/portraits/<key>.png`. |
| `equip` | `{ weapon, armor, accessory }` | empty | Starting equipment; each item must be of the slot's kind. |
| `lord` | bool | `false` | The player's lord: always deployed, and the battle is lost if they retreat. The campaign needs at least one lord among its starting officers. |
| `fixed_class` | bool | `false` | Cannot use class-change items. |
| `bio` | string | `""` | Biography text. |

Player officers keep their level, EXP, class, stats and equipment between battles. An officer who leaves
and joins again starts over from this definition.

**Hanja and the fonts.** The base pack's fonts (`fonts/Galmuri11.ttf`, `fonts/Galmuri9.ttf`) hold only
the Hanja that the base pack's own text uses (they are completed by the `fonts` step of
[tools/assets](../tools/assets/README.md#fonts-fonts)). A Hanja that is not in them — in a `hanja` or
`name` field, a scene or anywhere else in a mod — is drawn as a blank or a box, and neither
`hero-tools validate` nor the game reports it. The `fonts` step scans `data/base` only: for new text
there, rerun it; for a mod, ship fonts that cover your text in your pack's `fonts/` (a child pack's
files replace its parent's), for example ones built by the `fonts` step with your characters added
to `EXTRA_HANJA` in `build_fonts.py`.

## Battles

Each file in `pack.toml` `battles` holds one battle. Top-level fields come first, then the `[map]` and
`[deploy]` tables and the `[[units]]`, `[[events]]` and `[[treasures]]` arrays.

```toml
id = "b01"
name = "들판 전투"
location = "184년 탁현"
objective = "적장 장보를 물리쳐라"
bgm = "battle"
bgm_enemy = "enemy"
turn_limit = 20
reward_gold = 200
intro = "b01_intro"
outro = "b01_outro"
victory = [{ type = "defeat_commander" }]
defeat = [{ type = "unit_retreated", target = "militia" }]
bonus = { condition = { type = "defeat_unit", target = "deng_mao" }, exp = 30, desc = "등무를 물리쳐라" }

[map]
theme = "field"
rows = """
^^^.......
^^..TT..v.
^...TT....
..........
~~~=~~~~~~
..........
..........
cc........
"""

[deploy]
max = 3
slots = [[1, 6], [2, 6], [3, 6], [2, 7]]

[[units]]
side = "enemy"
officer = "zhang_bao"
pos = [1, 1]
ai = "guard"
commander = true
tag = "boss"
drop = "war_manual"

[[units]]
side = "enemy"
name = "황건적"
class = "bandit"
level = 3
pos = [5, 2]

[[units]]
side = "enemy"
name = "황건 기병"
class = "light_cavalry"
level = 4
pos = [9, 0]
ai = "advance"
ai_pos = [3, 4]
group = "rein"                     # hidden until an event spawns the group

[[events]]
trigger = { type = "turn_start", turn = 3, side = "enemy" }
actions = [{ type = "spawn", group = "rein" }, { type = "drama", scene = "b01_rein" }]

[[treasures]]
pos = [8, 1]
item = "red_horse"
gold = 50
```

### Battle fields

| field | type | default | meaning |
|---|---|---|---|
| `id` | id | required | Battle id (campaign nodes refer to it). |
| `name` | string | required | Display name. |
| `location` | string | `""` | Place/year caption. |
| `objective` | string | required | One-line objective shown to the player. |
| `bgm` / `bgm_enemy` | music key | none | Music for the player phase / the enemy phase. |
| `turn_limit` | integer ≥ 1 | required | The battle is lost when this turn ends without victory. |
| `map` | table | required | See [Map](#map). |
| `deploy` | table | required | See [Deployment](#deployment). |
| `units` | array of tables | required | Enemy and allied units: see [Units](#units). |
| `victory` | list of [conditions](#conditions) | required | Any satisfied condition wins. May be empty only if an event grants `victory`. |
| `defeat` | list of conditions | `[]` | Any satisfied condition loses. The lord retreating and the turn limit always lose and need not be listed. |
| `bonus` | `{ condition, exp, desc }` | none | Optional objective: when its condition becomes true, every surviving deployed player unit gets `exp` at victory. |
| `events` | array of tables | `[]` | See [Events](#events). |
| `treasures` | array of tables | `[]` | See [Treasures](#treasures). |
| `reward_gold` | integer ≥ 0 | 0 | Gold for winning. |
| `intro` / `outro` | scene id | none | Drama scene before the first turn / after victory. |

### Map

| field | type | default | meaning |
|---|---|---|---|
| `rows` | string | required (unless `use`) | One line per map row, one terrain glyph per tile. Blank lines and spaces around lines are ignored; every row must have the same width. |
| `legend` | table glyph → terrain id | `{}` | Extra glyphs for this map only (keys are single characters). Checked before the terrain glyphs, so it can also override one. |
| `theme` | string | none | Renderer hint (`field`, `castle`, `snow`, `desert` ...). |
| `image` | media key | none | **Picture layer**: `gfx/maps/<image>.png`, one picture of the whole map drawn instead of the terrain tileset. `rows` stay the rules (movement, defence, healing, strategies); the picture only changes the look. It must be exactly the map's width × height in tiles times the tileset's `tile_size` pixels (16 without a tileset), so that picture and rules line up; the game draws the tileset instead of a picture that is missing or of another size (and logs a warning). A picture has no animated layers. |
| `use` | map id | none | Take the whole map from a [map file](#map-files). The battle's `[map]` then holds nothing else. |

Write `rows` as a TOML multi-line string (`"""`). If a glyph is special in TOML strings (`\`), use a
literal string (`'''`) instead.

### Map files

A map file holds maps apart from battles, as `[[map]]` tables; battles play on one with
`[map] use = "<id>"`. Use map files for a map several battles share, or to ship maps before the
battles that play on them (the original mode's converted maps, for example). The files are listed in
`pack.toml` `maps`; a map nobody uses is loaded and checked all the same.

```toml
# maps/hills.toml
[[map]]
id = "hills"                 # unique in the pack; battles `use` it
name = "구릉지"               # optional, for tools and authors (battles show their own name)
theme = "field"
image = "hills"              # optional picture layer: gfx/maps/hills.png
legend = { "0" = "plain", "1" = "forest" }
rows = '''
0011
0001
'''
```

```toml
# battles/b07.toml
[map]
use = "hills"
```

A map file entry has the fields of a battle's [Map](#map) except `use`, plus `id` and `name`.

### Deployment

| field | type | default | meaning |
|---|---|---|---|
| `max` | integer ≥ 1 | required | Most player officers that may be deployed; at most the number of `slots`. |
| `required` | list of officer ids | `[]` | Officers that must be deployed. The lord is always deployed and need not be listed. |
| `forbidden` | list of officer ids | `[]` | Officers that may not be deployed (never the lord). |
| `slots` | list of positions | required | Deployment tiles, filled in order (required officers first). Each must be inside the map, passable for `foot` and unique. |

The player's officers normally come from the army through the deploy screen and are not listed in
`units`. A `side = "player"` unit is a guest on the player's side (controlled by the player) with one
exception: when its `officer` is in the army at that point, the battle places **the army's officer**
there — with the army's class, level, EXP, stats and equipment, which the unit's `class`, `level`,
`stats` and `equip` do not change — and that officer takes no deploy slot and is not placed a second
time. The unit's position, `ai`, `tag`, `group`, `commander` and `drop` still apply, and the officer's
progress in the battle is kept afterwards like any deployed officer's. A player guest who is not in the
army is built from `officers.toml` and does not join it.

### Units

| field | type | default | meaning |
|---|---|---|---|
| `side` | `enemy` · `ally` · `player` | required | Usually `enemy` or `ally`; `ally` units are friendly and controlled by the AI in their own phase. |
| `officer` | officer id | none | A named officer: name, portrait, class, level, stats and equipment come from `officers.toml`. Each officer appears at most once per battle. |
| `name` | string | none | Display name of a generic unit (ignored for officers). |
| `class` | class id | officer's class | Required for generic units; overrides the officer's class otherwise. |
| `level` | integer | officer's level | Required for generic units. |
| `stats` | `[str, int, lead]` | class `generic` | Stats of a generic unit. |
| `pos` | position | required | Starting tile: inside the map, passable for the unit's move type, not shared with another starting unit or a deploy slot. |
| `ai` | [AI mode](#ai-modes) | `aggressive` | Behaviour. |
| `ai_target` | tag or officer id | none | Target for `ai = "target"`. |
| `ai_pos` | position | none | Destination for `advance`, centre for `guard` (default: the spawn tile). |
| `commander` | bool | `false` | Enemy commander (for `defeat_commander`, extra EXP). |
| `tag` | string | none | Name that conditions and events use for this unit; unique in the battle. |
| `group` | string | none | Reinforcement group: the unit stays off the map until an event `spawn`s the group. If its tile is taken or impassable then, the nearest free passable tile is used. |
| `equip` | `{ weapon, armor, accessory }` | officer's equipment | Equipment of this unit. |
| `drop` | item id | none | Item the player gets when this unit is defeated. |

**Unit references.** Conditions, events and `ai_target` name units by `tag` or officer id. A reference is
valid when it matches a tag or officer of this battle, a required officer, or any officer who can be in
the player's army (starting officers and everyone who joins through `@join`).

#### AI modes

| `ai` | behaviour |
|---|---|
| `aggressive` | Seeks out and attacks the most attractive hostile unit anywhere. |
| `defensive` | Stays put until a hostile unit can be reached this phase, then fights. |
| `hold` | Never moves; attacks or uses strategies from its tile. |
| `guard` | Stays within 3 tiles of `ai_pos` (default: spawn tile). |
| `target` | Heads for `ai_target` and attacks it. |
| `advance` | Moves towards `ai_pos`, attacking targets of opportunity. |
| `flee` | Moves away from hostile units. |

### Conditions

Used by `victory`, `defeat` and `bonus`:

| condition | fields | true when |
|---|---|---|
| `{ type = "defeat_all" }` | — | Every enemy unit on the map has retreated (hidden reinforcements do not count). |
| `{ type = "defeat_unit", target = "boss" }` | `target` | That unit has retreated. |
| `{ type = "defeat_commander" }` | — | Any enemy commander has retreated. |
| `{ type = "reach", who = "liu_bei", pos = [8, 1], radius = 1 }` | `who` (optional), `pos`, `radius` (default 0), `to` (optional) | The unit (any player unit without `who`) stands within Manhattan distance `radius` of `pos` — or, with `to = [x, y]`, anywhere in the rectangle with the corners `pos` and `to` (inclusive; `radius` must then be 0). |
| `{ type = "survive_turns", turns = 10 }` | `turns` | That turn has been completed. |
| `{ type = "unit_retreated", target = "militia" }` | `target` | That unit has retreated (for defeat conditions such as "protect the villagers"). |

### Events

```toml
[[events]]
trigger = { type = "adjacent", a = "guan_yu", b = "boss" }
once = true                          # default
actions = [{ type = "drama", scene = "b01_duel" }, { type = "level_up", target = "guan_yu", amount = 1 }]
```

| field | type | default | meaning |
|---|---|---|---|
| `trigger` | trigger | required | When the event fires. |
| `once` | bool | `true` | Fire only the first time. |
| `stage` | integer | none | Fire only while the battle is at this stage (see below); without it, at every stage. |
| `when` | list of flag conditions | `[]` | Fire only while every condition holds (see below). |
| `actions` | list of actions | required | Run in order. |

**Stages.** Every battle starts at stage 0; a `set_stage` action moves it to another. An event with a
`stage` fires only while the battle is at that stage, so a battle can be told in phases: "first break
through to the city, then hold off the relief army". Events without `stage` fire at every stage.

```toml
[[events]]
trigger = { type = "unit_defeated", target = "che_zhou" }
stage = 0
actions = [{ type = "drama", scene = "gate_taken" }, { type = "set_stage", stage = 1 }]

[[events]]
trigger = { type = "reach", who = "liu_bei", pos = [1, 16] }
stage = 1                            # only after the gate is taken
actions = [{ type = "victory" }]
```

**Conditions.** `when = [{ flag = "gate_open" }, { flag = "route", cmp = "==", value = 2 }]`: the event
fires only while every condition holds. A condition compares a [flag](#flags) with `value` (default 0)
using `cmp` (`==`, `!=`, `<`, `<=`, `>`, `>=`; default `!=`, so `{ flag = "x" }` means "x is set"). The
flag's value is the one this battle's `set_flag` actions gave it, else the campaign's when the battle began.
An event whose conditions do not hold is not used up: it fires later when they do.

Triggers:

| trigger | fields | fires when |
|---|---|---|
| `turn_start` | `turn`, `side` (default `player`) | The phase of `side` starts on `turn`. |
| `unit_defeated` | `target` | That unit retreated. |
| `reach` | `who` (optional), `pos`, `radius` (default 0), `to` (optional) | The unit (any player unit without `who`) moved within `radius` of `pos`, or into the rectangle from `pos` to `to`. |
| `adjacent` | `a`, `b` | Two units stand orthogonally adjacent (duels). |
| `hp_below` | `target`, `pct` (1..=100) | The unit's HP fell below `pct` percent of its maximum. |

Actions:

| action | fields | effect |
|---|---|---|
| `drama` | `scene` | Play a drama scene now. |
| `spawn` | `group` | Bring every unit of that reinforcement group onto the map. |
| `set_ai` | `target`, `ai`, `ai_target` (opt.), `ai_pos` (opt.) | Change a unit's behaviour. The new values replace the old ones completely: a left-out `ai_target` or `ai_pos` is **cleared**, not kept. Without `ai_pos`, `guard` guards the tile the unit stands on now, and `advance` has no destination, so the unit behaves as `aggressive`. |
| `retreat` | `target` | Remove a unit without a fight (duel loser, escape). |
| `level_up` | `target`, `amount` | Grant levels. |
| `give_item` | `item` | Give the player an item (kept after a victory). |
| `give_gold` | `amount` | Give the player gold (kept after a victory). |
| `set_flag` | `flag`, `value` | Set a campaign [flag](#flags). |
| `set_stage` | `stage` | Move the battle to another stage (see above). |
| `set_terrain` | `pos`, `terrain`, `image` (opt.) | Change one tile's terrain for the rest of the battle (a gate opens, a drawbridge comes down); movement and defence follow the new terrain at once. `image` is a media key of `gfx/maps/<image>.png`, one tile in size, drawn over the tile from then on. Without it, a map drawn from the tileset shows the new terrain's tile, while a map with a picture layer keeps its picture there. |
| `victory` / `defeat` | — | End the battle. |

### Treasures

| field | type | default | meaning |
|---|---|---|---|
| `pos` | position | required | Tile inside the map; one treasure per tile. |
| `item` | item id | none | Item given to the first player unit that ends its move there. |
| `gold` | integer ≥ 0 | 0 | Gold given with it. |

Found items and gold, drops and event gifts reach the army only after a **victory**. The battle's level,
EXP, class and equipment changes of deployed officers are kept after a defeat too, and so is the use of
consumables: the battle counts every consumable it uses, and when it ends, won or lost, exactly those
are taken out of the army's inventory (never below 0). A battle uses its own stock, copied from the
inventory when it starts, so an item a scene gives while the battle runs (`@item` in the intro or in a
`drama` event action) reaches the inventory but cannot be used in that battle; validation warns about
it. The outro plays after the battle has ended, and its `@item` works as in any scene.

## campaign.toml

The campaign is a graph of **nodes**. A new game starts at `start` with the starting army; each screen
calls "advance" when it finishes, which follows the node's `next` link.

```toml
title = "영걸전"
start = "prologue"
starting_officers = ["liu_bei", "guan_yu", "zhang_fei"]
starting_gold = 500
starting_items = { bean = 3 }

[[node]]
type = "drama"
id = "prologue"
scene = "oath"
next = "camp1"

[[node]]
type = "camp"
id = "camp1"
title = "탁현 — 출진 준비"
shop = ["bean", "wine", "bronze_sword"]
battle = "b01"
next = "battle1"

[[node]]
type = "battle"
id = "battle1"
battle = "b01"
next = "route"
on_defeat = "retreat"

[[node]]
type = "branch"
id = "route"
flag = "captives"
cmp = ">="
value = 2
then = "mercy"
else = "camp2"

[[node]]
type = "ending"
id = "finale"
scene = "epilogue"
title = "끝"
```

| field | type | default | meaning |
|---|---|---|---|
| `title` | string | required | Campaign title. |
| `start` | node id | required | First node of a new game (not a branch: every flag is 0 then). |
| `starting_officers` | list of officer ids | required | The army at the start (at their `officers.toml` level/class/equipment). At least one must be a `lord`. |
| `starting_gold` | integer | 0 | Gold at the start (clamped to `0..=gold_cap`). |
| `starting_items` | table item id → count | `{}` | Inventory at the start. |
| `node` | array of nodes | required | The graph, in any order. Node ids are unique. |

Node types (`type = ...`):

| type | fields | behaviour |
|---|---|---|
| `drama` | `id`, `scene`, `next` | Play a scene, then go to `next`. |
| `camp` | `id`, `title` (opt.), `shop` (item ids, opt.), `battle` (opt.), `next` | Preparation screen: shop, equipment, class-up items, saving; with `battle` also the deploy screen for that battle. |
| `battle` | `id`, `battle`, `next`, `on_defeat` (opt.) | Fight. Victory goes to `next`; defeat goes to `on_defeat`, or ends the game when it is missing. |
| `branch` | `id`, `flag`, `cmp` (default `!=`), `value` (default 0), `then`, `else` | Evaluated instantly: if `flag cmp value` go to `then`, otherwise `else`. Chains of branches are followed. |
| `ending` | `id`, `scene` (opt.), `title` (opt.) | The end: plays the scene, then the credits with `title`. |

`cmp` accepts `==`, `!=`, `<`, `<=`, `>`, `>=` (or `eq`, `ne`, `lt`, `le`, `gt`, `ge`). A branch whose links can
lead back to itself through other branches only is reported; at run time such a loop is an error.

**Camp rules.** Items with `price = 0` can be neither bought nor sold; others sell for half their price.
Equipment goes into the slot of its `kind` (the old item returns to the inventory) and must allow the
officer's class family. A class-up item works when the officer's class has a `promote` entry with that
`item` and the officer has reached `promote.level`; class-change items do not work on `fixed_class`
officers. After a class change, equipment the new family cannot use returns to the inventory.

## Drama scripts (.drama)

Story scenes are plain text files. A file holds any number of **scenes**; a scene is a list of commands
executed top to bottom. Scene ids are global: campaign nodes, battle `intro`/`outro` and battle `drama`
actions refer to them.

```text
# Lines starting with # are comments.
== oath
@bg village
@bgm peace
@title 제1장 맹세
@narr 난세가 시작되었다.
    작은 마을에 세 사람이 모였다.
@show liu_bei left
@show 장비 right
liu_bei: 함께 백성을 지키자.
장비: 좋소, 형님!
@choice
  - 적을 끝까지 쫓는다 -> pursue
  - 마을을 지킨다 -> guard
@label pursue
@set pursue = 1
@goto recruit
@label guard
@set pursue = 0
@label recruit
@if pursue -> skip_gift
@gold +100
@item bean
@label skip_gift
@join jian_yong
jian_yong: 저도 힘을 보태겠습니다.
@hide all
@fade out
@end
```

### Lines

| line | meaning |
|---|---|
| `== <scene id>` | Starts a new scene (ends the previous one). Every command belongs to a scene. |
| `# ...` | Comment (the whole line). Blank lines are ignored. |
| `speaker: text` | Dialogue. `speaker` is an officer id (`liu_bei`) or an officer's display name (`장비`) — then the officer's name and portrait are shown — or any other name without spaces, up to 24 characters (`전령:`), shown as written without a portrait. Everything after the first `:` is the text. |
| indented line | Continues the previous dialogue, `@narr` or `@title` on a new line of the same box. |
| `@command args` | A command (below). |

A speaker written like an id (lowercase ASCII, digits and `_`) must be an existing officer, so typos are
caught; write free names in Korean or with a capital letter.

### Commands

| command | effect |
|---|---|
| `@bg <key>` / `@bg none` | Background image `gfx/bg/<key>.png` / clear it. |
| `@bgm <key>` / `@bgm stop` | Music `bgm/<key>.ogg` / stop the music. |
| `@sfx <key>` | Play sound `sfx/<key>.ogg` or `.wav`. |
| `@show <who> <left\|center\|right>` | Show a portrait in a slot (`l`, `c`, `r` also work). `who` is an officer id or display name (their portrait key is used), otherwise it is taken as a portrait key itself. |
| `@hide <left\|center\|right\|all>` | Hide one slot, or every slot (`@hide` alone = all). |
| `@wait <ms>` | Pause. |
| `@fade out` / `@fade in` | Fade the screen. |
| `@title <text>` | Large centred caption (chapter titles). |
| `@narr <text>` | Narration box without speaker. |
| `@choice` | Offer a choice; the following `- text -> label` lines are the options. Execution continues at the chosen option's label. |
| `@label <name>` | Jump target (unique within the scene). |
| `@goto <label>` | Jump. |
| `@if <flag> [op value] -> <label>` | Jump when the condition holds. Operators `==`, `!=`, `<`, `<=`, `>`, `>=`; `@if flag -> label` means `flag != 0`. |
| `@set <flag> = n` / `+= n` / `-= n` | Change a [flag](#flags). |
| `@join <officer id>` | The officer joins the army (a banner is shown; nothing happens if already in the army). |
| `@leave <officer id>` | The officer leaves; their equipment returns to the inventory (nothing happens if not in the army). |
| `@gold <±n>` | Give (or take) gold, clamped to `0..=gold_cap`. |
| `@item <item id>` | Give one item. |
| `@end` | End the scene. Added automatically at the end of every scene. |

Labels are checked when the file is loaded: jumping to a missing label is an error. Jumps stay inside
the scene. A scene that loops through `@goto` without showing anything for 10 000 commands is ended.

## Flags

Flags are named integers stored in the save game, all 0 at the start. They connect the three parts of a
pack:

* dramas set them with `@set` and test them with `@if`;
* battle events set them with `set_flag` (merged into the campaign when the battle ends, won or lost);
* campaign `branch` nodes choose the path with them.

A flag that is tested somewhere but never set anywhere is always 0; `hero-tools validate` warns about it.

## Media

Rules and scripts refer to media by **key**; the engine turns keys into paths:

| key used by | file |
|---|---|
| class `sprite` | `gfx/units/<sprite>_player.png`, `_ally.png`, `_enemy.png` + a `[sprites.<sprite>]` entry in `gfx/units/units.toml` |
| officer `portrait` (default: officer id), `@show` | `gfx/portraits/<key>.png` (`gfx/portraits/_unknown.png` is shown when missing) |
| terrain `tile` (default: terrain id) | `[tiles.<key>]` in `gfx/tiles/terrain.toml` (atlas `gfx/tiles/<image>`) |
| strategy `fx` | `[fx.<key>]` in `gfx/fx/fx.toml` + `gfx/fx/<key>.png` |
| item `icon` | `[icons] <key> = [col, row]` in `gfx/ui/icons.toml` |
| battle `bgm` / `bgm_enemy`, `@bgm` | `bgm/<key>.ogg` |
| `@bg` | `gfx/bg/<key>.png` |
| map `image` | `gfx/maps/<key>.png` |
| `@sfx` | `sfx/<key>.ogg` or `sfx/<key>.wav` |

Formats, sizes, sheet layouts and the keys the engine itself uses (UI icons, sound effects, jingles) are
specified in [ASSETS.md](ASSETS.md). Every third-party file must be credited in `CREDITS.md`.

## Validation

Problems are found in three stages. **Errors** of stages 1 and 2 make a pack unusable: the game refuses to
start it and `hero-tools validate` exits with 1. **Errors** of stage 3 (missing media) fail only
`hero-tools validate`; the game still starts and draws placeholders for what is missing (see
[DEVELOPING.md](DEVELOPING.md#running-natively)). **Warnings** point at content that is legal but probably a mistake.

### 1. Loading (always an error; the first one stops loading)

* A listed file is missing, is not valid TOML, or a field has the wrong type or is missing (the message
  names the file and the TOML position).
* A list file uses an unknown top-level table (`[[classes]]` instead of `[[class]]`).
* `pack.toml` paths that leave the pack or use `\`, and files listed twice; `presentation.canvas`
  outside `[320, 200]`..`[1280, 800]`; a pack without `extends` that does not list all five rules files,
  `officers` and `campaign`.
* [Layered packs](#layered-packs-extends): an `extends` that is not a relative `/` directory, a parent
  without a readable `pack.toml`, a pack that extends itself (directly or through others), two packs of
  a chain with the same `id`, more than 4 packs in a chain, a rules file, `officers` or `campaign` that
  no pack of the chain lists.
* Empty or duplicate ids of terrain, classes, strategies, items, officers, campaign nodes, battles and
  maps of map files; two terrain types with the same glyph. (In a chain, battle and map ids must be
  unique within each pack; a nearer pack's battle or map overrides a farther pack's.)
* A battle map or map file entry that cannot be parsed (no rows, rows of different width, unknown
  glyph, legend key longer than one character, legend naming unknown terrain).
* A battle that `use`s a map no map file defines, or that writes `rows`, `legend`, `theme` or `image`
  next to `use`.
* Drama syntax errors (reported as `file: line N: ...`): unknown commands, jumps to unknown labels,
  duplicate labels, commands outside a scene, malformed `@choice`, `@if`, `@set`, `@wait` ...
* The same scene id in two scenes, in one file or across files of one pack.

### 2. Cross-references (`Pack::validate`)

E = error, W = warning.

**pack.toml** — W: a `presentation.canvas` smaller than 480×270 (the camp and battle screens are laid
out for at least that size and overlap on smaller canvases).

**rules/game.toml** — E: `level_cap`/`exp_per_level` below 1, negative `gold_cap`/`mp_cap`/
`morale_loss_pct`/`counter_damage_pct`, `morale_start` or `confuse_morale` outside 0..=100, EXP tables not
strictly sorted or with negative EXP, `counter_divisor` ≤ 0, weather chances negative or not summing to 100,
affinity percentages ≤ 0. W: an empty EXP table, affinity naming a family no class has.

**rules/terrain.toml** — E: no terrain at all, invisible glyph, `defense`/`heal_hp`/`heal_morale` outside
0..=100, a movement cost of 0. W: a cost for a move type no class uses, an element no strategy uses, a
boosted element missing from `elements`.

**rules/classes.toml** — E: empty family, a move type with a cost on no terrain, unknown range shape,
`hp` ≤ 0, empty sprite, learning an unknown strategy, promoting to itself or to an unknown class, a
promotion item that does not exist or has no `promote` effect, promotion cycles. W: empty name, a range
that includes the unit's own tile, negative `hp_growth`, `tier` outside 1..=3, `generic` stats outside
0..=100, learn levels outside 1..=`level_cap`, promotion level above `level_cap`, a class that several
classes promote into.

**rules/strategies.toml** — E: negative `mp`, unknown range shape, an element no terrain allows, no effects,
`promote`/`change_class` effects. W: empty name, negative power, a status lasting 0 turns, attack strategies
aimed at allies, heal strategies aimed at enemies.

**rules/items.toml** — E: unknown class family, unknown `strategy`, `change_class` to an unknown class.
W: empty name, a `promote` item no class uses, `damage`/`status` effects on items, a consumable without
effects or strategy, `battle_use` on class items, heal/morale/strategy consumables without `battle_use`,
equipment bonuses or `families` on consumables, effects/strategy/`battle_use` on equipment, `atk_pct` on a
non-weapon, `def_pct` on non-armor, `move_bonus` on a non-accessory.

**officers.toml** — E: unknown class, level outside 1..=`level_cap`, equipment that does not exist or does
not fit its slot. W: empty name, stats outside 0..=100, equipment not meant for the officer's family.

**Maps** (map file entries) — E: legend naming unknown terrain, an `image` that is not a media key
(`/`-separated parts of letters, digits, `_` and `-`).

**Battles** — E: `turn_limit` 0, legend naming unknown terrain or an `image` that is not a media key
(for maps written in the battle; a `use`d map is checked as a map), no victory condition and no event granting
victory, `spawn` of a group without units, treasures outside the map / on the same tile / with unknown
items / negative gold, negative `reward_gold`, unknown `intro`/`outro` scenes. W: empty name, no enemy
units, a group no event spawns, a treasure that gives nothing, `@item` of a battle consumable in the
battle's intro or in a scene of a `drama` action (the item cannot be used in that battle; see
[Treasures](#treasures)).
*Deployment* — E: `max` 0 or larger than the number of slots, unknown required or forbidden officers, an
officer both required and forbidden, forbidding the lord, more must-deploy officers (required + lord) than
`max`, slots outside the map, not passable for `foot` (or, in packs without `foot`, for any class) or
listed twice. W: a required officer listed twice.
*Units* — E: unknown officer or class, the same officer twice, an officer that is also a required player
officer, generic units without class or level, level outside 1..=`level_cap`, positions outside the map,
impassable for the unit's move type, or shared with another starting unit or a deploy slot, empty or
duplicate tags, `ai = "target"` without `ai_target`, `ai_target` naming nothing, `ai_pos` outside the map,
bad equipment, unknown `drop` items. W: `stats` on named officers, generic units without `name`,
reinforcements on impassable tiles (they are shifted), tags equal to an officer id, equipment not meant
for the unit's family, a `side = "player"` unit naming a starting officer (the army's officer is placed
there, see [Units](#units)).
*Conditions, triggers, actions* — E: unit references that match nothing, positions outside the map,
negative radius, `defeat_all` without enemies on the map at the start, `defeat_commander` without an enemy
commander, `survive_turns`/`turn_start` with turn 0, `hp_below` outside 1..=100, unknown scenes, unknown
`give_item` items, `set_flag` without a name, `set_terrain` to an unknown terrain or with an image that
is not a media key. W: `survive_turns` or `turn_start` after `turn_limit`, events without actions,
`adjacent` naming the same unit twice, `set_ai` to `advance` without `ai_pos`, `level_up` by 0, an event
`stage` that no `set_stage` reaches, a `when` flag that nothing sets.

**Dramas** — E: `@join`/`@leave` of unknown officers, `@item` of unknown items, speakers that look like
ids but name no officer. W: scenes of the pack itself that no campaign node or battle plays (in a
[layered pack](#layered-packs-extends), a parent's scene that only a battle the child replaced played is not
reported: it is the parent's).

**campaign.toml** — E: unknown or branch `start` node, unknown starting officers or items, no lord among the
starting officers, unknown scenes, battles and shop items, branches without a flag, links to unknown nodes.
W: empty title, duplicated starting officers or shop items, starting items with count 0, `starting_gold`
outside 0..=`gold_cap`, shop items with price 0, nodes unreachable from `start`, no reachable ending,
branch loops, battles no campaign node uses.

**Flags** — W: flags that are tested (`@if`, branch nodes) but never set.

### 3. Tool checks (`hero-tools validate` only)

* **Unknown keys** (W): every TOML key the schema does not know, reported with its file and path, e.g.
  a misspelt `rnage` in the archer class of `rules/classes.toml` is reported as field `class[archer].rnage`.
* **Media** (natively, below the pack directory; for a layered pack in every pack of the chain, top pack
  first, index files read from the first pack that has them): E for missing unit sheets and `units.toml` entries,
  `_unknown.png`, music, backgrounds and sound effects used by battles and dramas, a missing
  `terrain.toml`/`fx.toml` or one that is not valid TOML, a missing terrain atlas image, terrain without a
  `[tiles.<key>]` entry, a `tile_size` that is not a positive whole number, strategy effects without an
  `fx.toml` entry or strip, a map picture (`image`) that is missing, is not a PNG or is not exactly the
  map's size in tiles times `tile_size`. W for missing portraits (the
  `_unknown` portrait is shown), a missing `icons.toml` or unknown icon keys. For the media index files
  (`units.toml`, `terrain.toml`, `fx.toml`, `icons.toml`) this checks only the TOML syntax, the files
  they name, `tile_size` and that the entries the pack needs exist; it does not check the other fields'
  types or values (`frame`, `anchor`, `frames`, `fps`, …). An index with such a mistake (say
  `frame = "48"`) passes validation; the game then logs a warning (stderr natively, the browser
  console on the web) and falls back to defaults or placeholders (16×16 unit frames, for example), so
  check a new index in the game.

## hero-tools

`hero-tools` is the command line companion for pack authors and CI. Exit codes: 0 = success, 1 = the pack
has errors (or a simulation failed), 2 = bad command line.

```
hero-tools validate <pack_dir>
hero-tools simulate <pack_dir> [--seeds N] [--battle ID]
hero-tools info <pack_dir>
hero-tools --help | --version
```

* **validate** — loads the pack, runs every check of [Validation](#validation) and prints the errors, then
  the warnings, then a summary line (`0 errors, 2 warnings: OK`).
* **info** — prints the pack's name, the packs it extends, licence, canvas size and how many terrain
  types, classes, strategies, items, officers, battles, scenes (with the number of dialogue lines) and
  campaign nodes it has.
* **simulate** — refuses packs with validation errors, then plays every battle (campaign order first, then
  battles the campaign does not use; or only `--battle ID`) AI against AI with seeds `1..=N` (default 4),
  for at most 200 phases each. The player army is a new game's starting army plus the battle's required
  officers and the officers its conditions and events name, at their `officers.toml` levels (so later
  battles are played under-levelled and their win rates are pessimistic); required officers and the lord
  are deployed first, then the roster up to `deploy.max`. It prints the win rate and average turns per
  battle, warns about battles that are never or always won, and fails (exit 1) when a battle panics, cannot
  be set up or does not finish within 200 phases.

Every command takes the top pack directory of a [layered pack](#layered-packs-extends) and works on the
whole chain.
