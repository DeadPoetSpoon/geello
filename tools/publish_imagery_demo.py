"""Stream the original assets/test/jd.tif to the experimental map server."""

import json
import argparse
import time
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
BASE = "http://127.0.0.1:3000"
ASSET = ROOT / "examples/web-preview/imagery"


def request(path, body=None, headers=None, method="GET"):
    req = Request(BASE + path, data=body, headers=headers or {}, method=method)
    try:
        with urlopen(req, timeout=1800) as response:
            return response.read()
    except HTTPError as error:
        raise RuntimeError(f"{method} {path}: {error.code}: {error.read().decode()}") from error


def publish(kind, resource_id, media, path, extra=None):
    route = f"/admin/v1/resources/{kind}/{resource_id}"
    try:
        with urlopen(BASE + route, timeout=60) as response:
            condition = {"If-Match": response.headers["ETag"]}
    except HTTPError as error:
        if error.code != 404:
            raise
        condition = {"If-None-Match": "*"}
    headers = {"Content-Type": media, "Content-Length": str(path.stat().st_size),
               **condition, **(extra or {})}
    # urllib streams a binary file through HTTPConnection without reading it all.
    with path.open("rb") as content:
        operation = json.loads(request(route + "/content", content, headers, "PUT"))
    print(f"Preparing: {kind}/{resource_id} ({operation['operation_id']})", flush=True)
    deadline = time.monotonic() + 1800
    while time.monotonic() < deadline:
        state = json.loads(request(f"/admin/v1/operations/{operation['operation_id']}"))
        if state["status"] == "ready":
            print(f"Ready: {kind}/{resource_id}", flush=True)
            return
        if state["status"] == "failed":
            raise RuntimeError(json.dumps(state, indent=2, ensure_ascii=False))
        time.sleep(1)
    raise TimeoutError(operation["operation_id"])


def main():
    global BASE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", nargs="?", default=BASE)
    args = parser.parse_args()
    BASE = args.base.rstrip("/")
    request("/health/ready")
    image = ROOT / "assets/test/jd.tif"
    print(f"Uploading original: {image} ({image.stat().st_size:,} bytes)", flush=True)
    publish("dataset", "preview:jd", "image/tiff", image, {
        "X-Geello-CRS": "EPSG:3857", "X-Geello-Sample-Kind": "continuous"})
    publish("style", "preview:jd", "application/toml", ASSET / "jd-style.toml")
    publish("map", "preview:jd", "application/toml", ASSET / "jd-map.toml")
    print("Imagery preview: http://127.0.0.1:8080/?map=preview%3Ajd")


if __name__ == "__main__":
    main()
