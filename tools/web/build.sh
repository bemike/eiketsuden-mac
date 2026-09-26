#!/usr/bin/env sh
# Builds the WebAssembly version of the game and assembles a static site.
#
# usage: tools/web/build.sh [--dev] [--data <pack dir>] [--out <dir>] [--serve <port>]
#
#   --data   data pack copied to <out>/data/base (default: data/base of this repository)
#   --out    output directory (default: target/web-dist, git-ignored); only ever cleared if an
#            earlier run of this script created it
#   --dev    debug profile (faster to compile, much slower to run)
#   --serve  afterwards serve the site on http://localhost:<port>/ with tools/web/serve.py
#            (a no-cache variant of `python3 -m http.server`)
#
# Relative --data / --out paths are taken relative to the current directory. The layout
# (index.html, mq_js_bundle.js, hero_web.js, eiketsuden.wasm, data/base/) matches what the GitHub
# Pages workflow publishes. Browsers cannot load WebAssembly from file:// URLs, so the folder has
# to be served over HTTP. Open http://localhost:<port>/#gallery for the UI gallery.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
data="$root/data/base"
out="$root/target/web-dist"
profile="release"
serve=""
marker=".eiketsuden-web-dist"

absolute() {
    case "$1" in
        /*) printf '%s\n' "$1" ;;
        *) printf '%s/%s\n' "$(pwd)" "$1" ;;
    esac
}

while [ $# -gt 0 ]; do
    case "$1" in
        --data) data=$(absolute "$2"); shift 2 ;;
        --out) out=$(absolute "$2"); shift 2 ;;
        --dev) profile="debug"; shift ;;
        --serve) serve="$2"; shift 2 ;;
        -h|--help) sed -n '2,16p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

cd "$root"

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
    exec "$python" tools/web/serve.py --port "$serve" --dir "$out"
fi
