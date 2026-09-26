#!/usr/bin/env sh
# Builds the WebAssembly version of the game and assembles a static site.
#
# usage: tools/web/build.sh [--dev] [--data <pack dir>] [--out <dir>] [--serve <port>]
#
#   --data   data pack copied to <out>/data/base (default: data/base)
#   --out    output directory (default: target/web-dist, git-ignored); only ever cleared if an
#            earlier run of this script created it
#   --dev    debug profile (faster to compile, much slower to run)
#   --serve  afterwards serve the site with `python3 -m http.server <port>`
#
# The layout (index.html, mq_js_bundle.js, hero_web.js, eiketsuden.wasm, data/base/) matches what
# the GitHub Pages workflow publishes. Browsers cannot load WebAssembly from file:// URLs, so the
# folder has to be served over HTTP. Open http://localhost:<port>/#gallery for the UI gallery.
set -eu

data="data/base"
out="target/web-dist"
profile="release"
serve=""
marker=".eiketsuden-web-dist"

while [ $# -gt 0 ]; do
    case "$1" in
        --data) data="$2"; shift 2 ;;
        --out) out="$2"; shift 2 ;;
        --dev) profile="debug"; shift ;;
        --serve) serve="$2"; shift 2 ;;
        -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

cd "$(dirname "$0")/../.."

if [ "$profile" = "release" ]; then
    cargo build -p hero-game --target wasm32-unknown-unknown --release
else
    cargo build -p hero-game --target wasm32-unknown-unknown
fi

if [ -e "$out" ]; then
    if [ ! -e "$out/$marker" ]; then
        echo "$out exists but was not created by this script; refusing to overwrite it" >&2
        exit 1
    fi
    rm -rf "$out"
fi
mkdir -p "$out/data"
touch "$out/$marker"

cp web/index.html web/mq_js_bundle.js web/hero_web.js "$out/"
cp "target/wasm32-unknown-unknown/$profile/eiketsuden.wasm" "$out/"

if [ -d "$data" ]; then
    cp -R "$data" "$out/data/base"
    if [ ! -f "$data/pack.toml" ]; then
        echo "warning: $data has no pack.toml: only the UI gallery (#gallery) will work" >&2
    fi
else
    echo "warning: data pack $data not found: the page will show the 'pack not found' error screen" >&2
fi

echo "site ready in $out ($(wc -c < "$out/eiketsuden.wasm") bytes of wasm)"

if [ -n "$serve" ]; then
    python=$(command -v python3 || command -v python || true)
    if [ -z "$python" ]; then
        echo "python is needed for --serve (or serve $out with any static web server)" >&2
        exit 1
    fi
    echo "serving on http://localhost:$serve/  (UI gallery: http://localhost:$serve/#gallery; Ctrl+C stops)"
    exec "$python" -m http.server "$serve" --directory "$out"
fi
