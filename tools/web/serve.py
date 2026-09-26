#!/usr/bin/env python3
"""Serve the assembled web build (target/web-dist) for local testing.

Like `python -m http.server`, but
  * responses carry `Cache-Control: no-store`, so the browser always picks up a rebuilt
    eiketsuden.wasm or edited data file on reload;
  * `.wasm` is always served as `application/wasm` (the Windows registry can map it to
    something else);
  * it listens on 127.0.0.1 unless --bind says otherwise.

usage: python tools/web/serve.py [--port 8080] [--bind 127.0.0.1] [--dir target/web-dist]
"""

import argparse
import functools
import http.server


class NoCacheHandler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        **http.server.SimpleHTTPRequestHandler.extensions_map,
        ".wasm": "application/wasm",
        ".js": "text/javascript",
    }

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--dir", default="target/web-dist")
    args = parser.parse_args()
    handler = functools.partial(NoCacheHandler, directory=args.dir)
    with http.server.ThreadingHTTPServer((args.bind, args.port), handler) as server:
        host = "localhost" if args.bind in ("127.0.0.1", "0.0.0.0") else args.bind
        print(f"serving {args.dir} on http://{host}:{args.port}/ "
              f"(UI gallery: http://{host}:{args.port}/#gallery; Ctrl+C stops)", flush=True)
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass


if __name__ == "__main__":
    main()
