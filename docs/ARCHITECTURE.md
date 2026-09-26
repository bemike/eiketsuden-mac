# Architecture

Eiketsuden Reloaded follows the OpenRCT2 model: an original, open-source engine plus data.
The engine never embeds content; everything the player sees comes from a **data pack**.

```
┌──────────────────────────── hero-game (bin: eiketsuden) ────────────────────────────┐
│ macroquad: window, input, audio, rendering (native Win/Linux/macOS + wasm32)        │
│  app.rs ── screen flow driven by campaign nodes                                     │
│  screens/ title · drama · camp (deploy/equip/shop/save) · battle · saveload · ending │
│  ui/ widgets · gfx.rs virtual canvas · assets.rs · audio.rs · platform/ (storage)   │
└───────────────▲─────────────────────────────────────────────────────────────────────┘
                │ pure Rust API (no I/O)
┌───────────────┴──────────────────────── hero-core ───────────────────────────────────┐
│ data.rs rules schema · battledef.rs · map.rs · pack.rs (load + validate)            │
│ script.rs .drama parser · drama.rs runner · campaign.rs army/flow · save.rs          │
│ battle/ state · movement/ZOC · combat · strategies · events · AI                     │
└───────────────▲─────────────────────────────────────────────────────────────────────┘
                │
┌───────────────┴──────── hero-tools (bin) ────────┐   ┌──── data/base (base pack) ────┐
│ validate <pack> · simulate <pack> (AI vs AI)     │   │ pack.toml, rules/, officers,   │
│ original probe|extract <dir> (hero-import)       │   │ campaign, battles/, dramas/,   │
└──────────────────────────────────────────────────┘   │ gfx/, bgm/, sfx/, fonts/       │
                                                        └────────────────────────────────┘
```

## Crates

| Crate | Role | Depends on |
|---|---|---|
| `hero-core` | Game rules, data schema, pack loading/validation, drama scripting, campaign and save state. Deterministic (seeded [`Rng`](../crates/hero-core/src/rng.rs)); no graphics, no clock, no file I/O except the optional `DirSource`. | serde, toml, serde_json, thiserror |
| `hero-game` | The game executable (`eiketsuden` / `eiketsuden.exe` / `eiketsuden.wasm`). | hero-core, macroquad |
| `hero-tools` | Command line tools for pack authors and CI: `validate`, `simulate`, `info`; `original probe` / `original extract` for the importer. | hero-core, hero-import |
| `hero-import` | Optional, experimental importer for an owned copy of the original game: edition probe and shareable manifest, LS11 / 6-byte-table containers, text, planar sprites and palettes → a media overlay folder that the game reads with `--original <dir>` (native only). See [ORIGINAL_DATA.md](ORIGINAL_DATA.md). | encoding_rs, png, sha2, serde, serde_json |

## Data flow

1. **Startup** — `hero-game` resolves the pack directory (`--data <dir>`, env `EIKETSUDEN_DATA`,
   `<exe dir>/data/base`, `./data/base`), reads `pack.toml`, then every file from
   `PackManifest::text_files()` (async `load_file`, works for both fetch and disk), and calls
   `Pack::load`. Media files are loaded lazily by key.
2. **Campaign** — `CampaignState` holds the current node id. The app shows the screen for the node
   (`Drama` → drama screen, `Camp` → camp screen, `Battle` → battle screen, `Ending` → ending) and calls
   `CampaignState::advance` when the screen finishes. `Branch` nodes are resolved inside `advance`.
3. **Drama** — `DramaRunner::next` yields presentation `Step`s and applies side effects (flags, gold,
   items, officers joining) to the campaign.
4. **Battle** — `BattleState::new` builds units from the roster, `begin` starts turn 1. The battle
   screen turns input into `Action`s, calls `apply`, and animates the returned `BattleEvent`s. AI
   phases use `next_ai_unit` + `ai_actions`. After victory `CampaignState::apply_battle_result`
   copies progress back.
5. **Saving** — `SaveGame` (versioned JSON) holds the campaign and an optional battle in progress.
   Natively saves live in the user data directory; on the web in `localStorage` (via `web/hero_web.js`).

## Determinism and testing

All randomness goes through the battle's serialized `Rng`, so a battle replays identically from a
save, and `hero-tools simulate` can run every battle of the campaign AI-vs-AI across many seeds in CI
to catch unwinnable maps, stuck AI and panics.

## Rendering model

The game renders to a fixed **480×270 virtual canvas** (16×16 px tiles, 30×17 tiles visible) and scales
it to the window with integer factors when possible (nearest-neighbour, letterboxed). All UI
coordinates are virtual pixels. Text uses the Galmuri pixel fonts (OFL), which cover Hangul, Latin
and the Hanja used in officer names.

See also: [RULES.md](RULES.md) (combat formulas), [MODDING.md](MODDING.md) (data formats),
[ASSETS.md](ASSETS.md) (media conventions), [DECISIONS.md](DECISIONS.md).
