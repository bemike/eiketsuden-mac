# 영걸전 Reloaded (Eiketsuden Reloaded)

An open-source reimplementation of the 1995 tactical RPG *Sangokushi Eiketsuden* (三國志英傑傳),
following the rules of its PC version. In the spirit of OpenRCT2 and OpenTTD it has its own engine,
written from scratch in Rust with [macroquad](https://github.com/not-fl3/macroquad), and its own,
openly licensed content. It runs natively on Windows, Linux and macOS and in the browser
(WebAssembly).

유비·관우·장비와 함께 반동탁 연합에서 서주 공방전까지 싸우는 턴제 전략 RPG입니다. 원작 PC판의 전투
규칙을 재현하고, 이야기와 대사는 퍼블릭 도메인 소설 『삼국지연의』를 바탕으로 새로 썼습니다.

**Play in the browser:** <https://jeiel85.github.io/eiketsuden-reloaded/> ·
**Downloads:** [Releases](https://github.com/jeiel85/eiketsuden-reloaded/releases)

> **Not affiliated with KOEI TECMO.** This is an independent fan project. It is not made, endorsed
> or supported by KOEI TECMO GAMES CO., LTD. No material of the original game — no graphics, music,
> sound, text, data or code — is included in this repository or in its releases. The name
> "Eiketsuden" (英傑伝) is used only to say which game this project reimplements; "Sangokushi
> Eiketsuden" and KOEI TECMO are trademarks of their respective owners. See
> [DECISIONS.md](docs/DECISIONS.md) (D3, D5) and [CREDITS.md](CREDITS.md).
>
> 이 프로젝트는 KOEI TECMO와 관계없는 독립 팬 프로젝트이며, 원작의 그래픽·음악·효과음·텍스트·데이터·
> 코드를 포함하지 않습니다.

## What is in it

* The prologue (the coalition against Dong Zhuo) and chapter 1 (from Jieqiao to the struggle for
  Xuzhou): 21 battles with branches in the campaign, 118 officers, dramas between the battles, camps
  with shops, equipment and deployment.
* Battles with the original's rules: terrain, zones of control, class affinities, strategies,
  morale and confusion, items, experience and class changes, weather, events and the enemy AI
  ([RULES.md](docs/RULES.md)).
* Everything the player sees comes from a data pack (`data/base`) of plain TOML files and `.drama`
  scripts, so it can be modded ([MODDING.md](docs/MODDING.md)).
* Save slots and an autosave, Korean text with Hangul and Hanja, keyboard, mouse and touch controls.

## Playing

Download the archive for your system from the releases page, unpack it and start `eiketsuden`
(`eiketsuden.exe` on Windows). Keep the `data` folder next to the executable. The Windows and macOS
builds are not code-signed, so the system may ask for confirmation before the first start.

| action | keys | mouse / touch |
|---|---|---|
| confirm | Z, Enter, Space | click / tap |
| cancel, menu | X, Esc, Backspace | right click |
| move the cursor | arrow keys, WASD | pointer, drag to scroll |
| fullscreen (native) | F11, Alt+Enter | |

In a scene, holding Ctrl, Tab or a cancel key fast-forwards and L or PageUp shows the recent lines.

## Building from source

Requirements: Rust 1.85 or newer. On Linux also `libx11-dev libxi-dev libgl1-mesa-dev
libasound2-dev`.

```sh
cargo run --release -p hero-game                     # play
cargo run --release -p hero-tools -- validate data/base   # check a data pack
cargo run --release -p hero-tools -- simulate data/base   # AI vs AI run of every battle
```

The web build needs the `wasm32-unknown-unknown` target and Python 3 to serve it locally:

```sh
rustup target add wasm32-unknown-unknown
pwsh tools/web/build.ps1 -Serve 8080    # or: tools/web/build.sh --serve 8080
```

Then open <http://localhost:8080/>. [DEVELOPING.md](docs/DEVELOPING.md) has the details: data pack
lookup, where saves live, the UI gallery, how the web build fits together and how releases and the
web demo are published.

## Documentation

| document | contents |
|---|---|
| [ARCHITECTURE.md](docs/ARCHITECTURE.md) | crates (`hero-core`, `hero-game`, `hero-tools`) and how they fit |
| [RULES.md](docs/RULES.md) | the battle rules |
| [MODDING.md](docs/MODDING.md) | data pack formats: rules, officers, battles, campaign, dramas |
| [ASSETS.md](docs/ASSETS.md) | media keys, formats and the asset pipeline |
| [DEVELOPING.md](docs/DEVELOPING.md) | building, running, the web build, publishing |
| [DECISIONS.md](docs/DECISIONS.md) | decisions that are hard to reverse, with their reasons |

## License

* Engine code: [GPL-3.0-or-later](LICENSE).
* Base pack text (`data/base`): CC BY-SA 4.0, as stated in `data/base/pack.toml`.
* Graphics, music, sound effects and fonts: each under its own open licence, with sources, authors
  and licences listed in [CREDITS.md](CREDITS.md).
