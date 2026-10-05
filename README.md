# 三国志英杰传 · 原生 Mac 版

基于 [Eiketsuden Reloaded](https://github.com/jeiel85/eiketsuden-reloaded) 的中文 Mac 改造版，在 Apple Silicon 上运行原生 Rust 引擎，读取转换后的中文 DOS 游戏资源。运行时不依赖 DOSBox、Windows 或 Rosetta。

**当前 Mac 版：0.1.12。**

## 下载与运行

[下载最新版（Releases）](https://github.com/bemike/eiketsuden-mac/releases/latest) · [0.1.12 Apple Silicon ZIP](https://github.com/bemike/eiketsuden-mac/releases/download/v0.1.12/eiketsuden-mac-0.1.12-arm64.zip)

1. 下载 `eiketsuden-mac-0.1.12-arm64.zip` 并解压。
2. 将 `三国志英杰传-0.1.12.app` 放到“应用程序”或其他文件夹，双击打开。
3. 首次游玩选择“新的征程”；已有本项目存档时，可继续游戏或读档。

完整版所需资源随应用打包，不需要安装 Rust、Python 或 DOSBox。仅提供 Apple Silicon 包，没有 Intel Mac 包。应用包声明最低 macOS 12；目前只在开发者的 Apple Silicon Mac 上实际验证，未覆盖所有系统版本。应用采用本地临时签名，尚未完成 Apple 开发者签名和公证，首次运行可能被 macOS 安全机制阻止。

## 已补齐的功能

- 中文对白、菜单和中文像素字体补字。
- 开场室内背景与人物显示。
- 战前「本关要点」：查看当前关卡的单挑、宝物坐标、物品和兵种转换提示，分支奖励注明选择条件。
- 战前八格个人背包，以及武将间道具转交、互换。
- 战斗物品获得、使用与结算衔接，豆、酒支持给周围八格友军使用。
- 补回广川关羽单挑逢纪、许昌连战及过场城镇和大地图背景。
- 自动、手动、快速存档和读档。
- 原版静态标题画面、高清圆角应用图标。
- 移动后无攻击目标时默认“待命”，有目标时默认“攻击”。

这是一份仍在逐关试玩和改进的非官方版本，不保证所有剧情、演出、难度和规则与 DOS 版完全一致。更多变化和限制见 [CHANGELOG.md](CHANGELOG.md)。

## 操作和存档

鼠标左键选择、右键取消；Enter/空格确认，Esc 返回，方向键移动菜单或光标；F5 快速保存、F9 快速读取。部分 Mac 键盘需要同时按 Fn。

存档和设置在用户目录中，与 `.app` 分开：

```text
~/Library/Application Support/EiketsudenNative/
```

- `save_original_auto.json`：自动存档。
- `save_original_quick.json`：快速存档。
- `save_original_1.json` 至 `save_original_8.json`：手动存档槽。
- `settings.json`：设置。

升级应用会继续使用同一存档目录。迁移到另一台 Mac 时，先保存并退出，将应用和整个 `EiketsudenNative` 文件夹分别复制过去；只复制应用不会带走进度。目前没有自动云同步，也不支持原 DOS 存档导入。这个仓库和下载包均不包含开发者的存档。

## 构建与打包

需要 Rust stable（上游最低要求 1.85）、macOS 开发工具；打包脚本需要 Python 3.11+ 和 `fonttools`。在 Apple Silicon Mac 上构建：

```bash
cargo build --locked --release -p hero-game --target aarch64-apple-darwin
cargo test --locked --workspace
```

引擎源代码在 `crates/`，基础资源在 `data/base/`。完整中文原版转换数据不提交到 Git 历史，随完整版 Release 分发。重打包时可以使用现有完整版应用内的转换数据：

```bash
python3 -m venv .venv
source .venv/bin/activate
python -m pip install 'fonttools[woff]'
python scripts/package_macos.py \
  --binary target/aarch64-apple-darwin/release/eiketsuden \
  --original-pack '/path/to/三国志英杰传-0.1.12.app/Contents/Resources/data/original' \
  --output dist
```

脚本根据仓库位置找资源，不依赖开发者电脑路径；检查字体、生成 ICNS、签名并校验 ZIP。输出目录已有同名文件时会拒绝覆盖。源码编译也可用 `cargo run --release -p hero-game -- --data /path/to/converted-original` 启动转换后的资源包；该资源包须以 `extends` 指向基础包。原始 DOS 文件转换工具和格式说明见 [docs/ORIGINAL_DATA.md](docs/ORIGINAL_DATA.md)。

Mac 应用版本记录在 `VERSION`；Cargo 中的 `0.4.1` 是所采用的上游引擎版本号。手动源码检查的 GitHub Actions 模板保存在 `docs/upstream-workflows/mac-check.yml.template`，未启用自动构建或发布。

## 项目来源与许可

本项目基于上游 **v0.4.1**，保留其提交历史。原始提交：`202e2771a8b674616a8e75e799c1490411e7b164`。中文适配、功能修复和 Mac 封装由 OpenAI Codex 协助完成。

- 引擎和工具代码：GPL-3.0-or-later，见 [LICENSE](LICENSE)。
- 上游基础包文字、数据：CC BY-SA 4.0；基础素材分别遵循 CC0、CC BY、OFL 或公有领域条款，见 [CREDITS.md](CREDITS.md)。
- 中文原版游戏的图像、对白、音乐和其他素材：归原权利人所有，**不因引擎开源而获得 GPL 或 CC 授权**。本仓库的原版标题图及标题衍生图标也不属于上游的 CC0 美术。
- 营地插画为 AI 生成，高清圆角图标为参考原版标志的 AI 重建。

我们不代表 KOEI TECMO，也不是官方移植。

上游原始说明保存在 [README.upstream.md](README.upstream.md)，其“发布物不含原版素材”等表述仅适用于上游，不能用于描述本仓库的完整版。字体、标题及插画来源见 [CREDITS.md](CREDITS.md)、[docs/ORIGINAL_TITLE_SOURCES.md](docs/ORIGINAL_TITLE_SOURCES.md) 和 [docs/HAN_CAMP_AND_CHINESE_FONTS.md](docs/HAN_CAMP_AND_CHINESE_FONTS.md)。
