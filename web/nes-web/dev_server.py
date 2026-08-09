#!/usr/bin/env python3
"""Serve the WASM frontend and collect optional ?debug=1 browser reports."""

import argparse
import json
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


ROOT = Path(__file__).resolve().parent


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, directory=str(ROOT), **kwargs)

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        super().end_headers()

    def do_POST(self):
        if self.path.split("?", 1)[0] != "/__debug":
            self.send_error(404)
            return

        try:
            length = min(int(self.headers.get("Content-Length", "0")), 1_000_000)
            record = json.loads(self.rfile.read(length))
        except (ValueError, json.JSONDecodeError) as error:
            self.send_error(400, str(error))
            return

        print(f"[browser-debug] {json.dumps(record, ensure_ascii=False)}", flush=True)
        self.send_response(204)
        self.end_headers()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=8000)
    args = parser.parse_args()

    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"Serving {ROOT} on http://{args.host}:{args.port}/", flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
