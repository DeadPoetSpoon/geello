"""Serve the independent map preview and a read-only, fixed-upstream proxy."""

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import unquote, urlsplit
from urllib.request import Request, urlopen

PAGE = Path(__file__).resolve().parents[1] / "examples/web-preview"
MAX_RESPONSE = 32 * 1024 * 1024


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream", default="http://127.0.0.1:3000")
    parser.add_argument("--port", type=int, default=8080)
    parser.add_argument("--timeout", type=float, default=1800,
                        help="Upstream response timeout in seconds (default: 1800)")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("--timeout must be positive")
    upstream = args.upstream.rstrip("/")
    parsed = urlsplit(upstream)
    if parsed.scheme not in ("http", "https") or not parsed.netloc or parsed.query or parsed.fragment:
        parser.error("--upstream must be an HTTP(S) base URL, optionally with a path prefix")

    class Handler(BaseHTTPRequestHandler):
        def reply(self, code, content, media):
            self.send_response(code)
            self.send_header("Content-Type", media)
            self.send_header("Content-Length", str(len(content)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.end_headers()
            self.wfile.write(content)

        def do_GET(self):
            try:
                self.get()
            except ConnectionError:
                pass  # A newer view can cancel an image request.

        def get(self):
            route = urlsplit(self.path)
            if route.path == "/preview-config.json":
                views = json.loads((PAGE / "views.json").read_text(encoding="utf-8"))
                return self.reply(200, json.dumps({**views, "upstream": upstream}).encode(), "application/json")
            files = {"/": "index.html", "/index.html": "index.html", "/preview.js": "preview.js", "/preview-ol.js": "preview-ol.js", "/preview.css": "preview.css", "/vendor/ol.js": "vendor/ol.js", "/vendor/ol.css": "vendor/ol.css"}
            if route.path in files:
                name = files[route.path]
                if name.startswith("vendor/"):
                    media = "text/javascript" if name.endswith(".js") else "text/css"
                    return self.reply(200, (PAGE / name).read_bytes(), media + "; charset=utf-8")
                media = {"html": "text/html", "js": "text/javascript", "css": "text/css"}[name.rsplit(".", 1)[1]]
                return self.reply(200, (PAGE / name).read_bytes(), media + "; charset=utf-8")
            if not route.path.startswith("/service/"):
                return self.reply(404, b"Not found", "text/plain")
            path = route.path[len("/service"):]
            decoded = unquote(path)
            allowed = path in ("/health/ready", "/api", "/api.html", "/wms", "/wmts", "/wfs") or path.startswith(("/ogc/maps", "/ogc/tiles", "/ogc/features", "/maps/", "/tiles/"))
            if not allowed or "\\" in decoded or any(part in (".", "..") for part in decoded.split("/")):
                return self.reply(404, b"Not found", "text/plain")
            target = upstream + path + ("?" + route.query if route.query else "")
            try:
                with urlopen(Request(target, headers={"Accept": self.headers.get("Accept", "*/*")}), timeout=args.timeout) as response:
                    body = response.read(MAX_RESPONSE + 1)
                    if len(body) > MAX_RESPONSE:
                        return self.reply(502, b"Upstream response exceeds preview limit", "text/plain")
                    self.reply(response.status, body, response.headers.get("Content-Type", "application/octet-stream"))
            except HTTPError as error:
                self.reply(error.code, error.read(MAX_RESPONSE), error.headers.get("Content-Type", "text/plain"))
            except (URLError, TimeoutError, OSError):
                self.reply(502, b"Map service unavailable. Check the upstream server.", "text/plain")

    server = ThreadingHTTPServer(("127.0.0.1", args.port), Handler)
    print(f"Geello preview: http://127.0.0.1:{server.server_port} (service: {upstream})", flush=True)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
