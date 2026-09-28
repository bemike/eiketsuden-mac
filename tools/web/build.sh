#!/usr/bin/env sh
# Builds the WebAssembly version of the game and assembles a static site.
#
# usage: tools/web/build.sh [--dev] [--data <pack dir>] [--out <dir>] [--serve <port>]
#
#   --data   data pack copied to <out>/data/base (default: data/base of this repository), with the
#            packs it extends
#   --out    output directory (default: target/web-dist, git-ignored); only ever cleared if an
#            earlier run of this script created it
#   --dev    debug profile (faster to compile, much slower to run)
#   --serve  afterwards serve the site on http://localhost:<port>/ with tools/web/serve.py
#            (a no-cache variant of `python3 -m http.server`)
#
# Relative --data / --out paths are taken relative to the current directory. The site is laid out
# by tools/web/assemble.py (Python 3.11+; with fontTools it subsets the fonts), the same step the
# GitHub Pages workflow runs. Browsers
# cannot load WebAssembly from file:// URLs, so the folder has to be served over HTTP. Open
# http://localhost:<port>/#gallery for the UI gallery.
set -eu

root=$(cd "$(dirname "$0")/../.." && pwd)
data="$root/data/base"
out="$root/target/web-dist"
profile="release"
serve=""

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
        -h|--help) sed -n '2,17p' "$0"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

cd "$root"

if [ "$profile" = "release" ]; then
    cargo build -p hero-game --target wasm32-unknown-unknown --release
else
    cargo build -p hero-game --target wasm32-unknown-unknown
fi

# The first Python that is 3.11 or newer (macOS ships an older python3).
python=""
for candidate in python3 python; do
    if command -v "$candidate" >/dev/null 2>&1 &&
        "$candidate" -c 'import sys; sys.exit(sys.version_info < (3, 11))' >/dev/null 2>&1; then
        python=$(command -v "$candidate")
        break
    fi
done
if [ -z "$python" ]; then
    echo "python 3.11+ is needed to assemble the site (tools/web/assemble.py)" >&2
    exit 1
fi
"$python" tools/web/assemble.py --wasm "target/wasm32-unknown-unknown/$profile/eiketsuden.wasm" \
    --out "$out" --data "$data"

if [ -n "$serve" ]; then
    exec "$python" tools/web/serve.py --port "$serve" --dir "$out"
fi
