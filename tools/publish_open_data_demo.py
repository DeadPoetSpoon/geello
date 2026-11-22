"""Download checksummed public-domain samples, publish them and verify live rendering."""

import argparse
import base64
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import time
from urllib.error import HTTPError
from urllib.parse import urlencode
from urllib.request import Request, urlopen
import zipfile

import publish_imagery_demo as publisher

ROOT = Path(__file__).resolve().parents[1]
EXAMPLE = ROOT / "examples/open-data"
DATA = ROOT / "assets/test/open-data"
MANIFEST = json.loads((EXAMPLE / "sources.json").read_text(encoding="utf-8"))
MAPS = ("vector", "raster", "overlay", "landsat")


def digest(path):
    with path.open("rb") as content:
        return hashlib.file_digest(content, "sha256").hexdigest()


def download(source):
    path = DATA / source["file"]
    if not path.exists():
        temporary = path.with_suffix(path.suffix + ".part")
        print(f"Downloading: {source['url']}", flush=True)
        headers = {"User-Agent": "geello-open-data-demo"}
        if source.get("transport") == "github-raw-blob":
            headers["Accept"] = "application/vnd.github.raw+json"
        request = Request(source["url"], headers=headers)
        with urlopen(request, timeout=300) as response:
            if source.get("transport") == "github-blob":
                temporary.write_bytes(base64.b64decode(json.load(response)["content"]))
            else:
                with temporary.open("wb") as output:
                    shutil.copyfileobj(response, output)
        if digest(temporary) != source["sha256"]:
            raise ValueError(f"Checksum mismatch: {temporary}")
        temporary.replace(path)
    if digest(path) != source["sha256"]:
        raise ValueError(f"Checksum mismatch: {path}; move it aside before retrying")
    return path


def web_mercator_polygons(raw):
    output = bytearray(raw)
    offset = 100
    changed = 0
    latitude_limit = 85.0511287798066

    def bounded(position):
        nonlocal changed
        longitude, latitude = struct.unpack_from("<dd", output, position)
        if abs(longitude) > 180 + 1e-9 or abs(latitude) > 90:
            raise ValueError("Unexpected out-of-domain source coordinate")
        normalized = (max(-180.0, min(180.0, longitude)),
                      max(-latitude_limit, min(latitude_limit, latitude)))
        changed += normalized != (longitude, latitude)
        struct.pack_into("<dd", output, position, *normalized)

    while offset < len(output):
        words = struct.unpack_from(">i", output, offset + 4)[0]
        start = offset + 8
        kind = struct.unpack_from("<i", output, start)[0]
        if kind != 5:
            raise ValueError(f"Expected Polygon, got {kind}")
        parts, points = struct.unpack_from("<ii", output, start + 36)
        for index in range(points):
            bounded(start + 44 + parts * 4 + index * 16)
        bounded(start + 4)
        bounded(start + 20)
        offset = start + words * 2
    bounded(36)
    bounded(52)
    print(f"Prepared Web Mercator-compatible polygons: {changed} coordinate/bounds adjustments")
    return bytes(output)


def prepare():
    from PIL import Image, TiffImagePlugin

    DATA.mkdir(parents=True, exist_ok=True)
    for source in MANIFEST["sources"]:
        path = download(source)
        if path.suffix == ".zip":
            with zipfile.ZipFile(path) as archive:
                member = next(name for name in archive.namelist() if name.endswith(".shp"))
                with archive.open(member) as content:
                    header = content.read(100)
                shape_type = struct.unpack_from("<I", header, 32)[0]
                index = archive.getinfo(member[:-4] + ".shx")
                count = (index.file_size - 100) // 8
                if count != source["features"]:
                    raise ValueError(f"Unexpected feature count: {path}: {count}")
                with zipfile.ZipFile(DATA / f"{source['id']}.zip", "w", zipfile.ZIP_DEFLATED) as output:
                    polygon = web_mercator_polygons(archive.read(member)) if shape_type == 5 else None
                    for suffix in (".shp", ".shx", ".dbf", ".prj", ".cpg"):
                        name = member[:-4] + suffix
                        content = archive.read(name)
                        if polygon and suffix == ".shp":
                            content = polygon
                        elif polygon and suffix == ".shx":
                            content = content[:36] + polygon[36:68] + content[68:]
                        output.writestr(Path(name).name, content)
                print(f"Verified: {path.name}: {count} features, shape type {shape_type}")
        elif path.suffix == ".png":
            with Image.open(path) as source_image:
                image = source_image.convert("RGB")
            width, height = image.size
            if width != height * 2:
                raise ValueError("Expected a global 2:1 Plate Carree raster")
            tags = TiffImagePlugin.ImageFileDirectory_v2()
            tags[33550] = (360.0 / width, 180.0 / height, 0.0)
            tags[33922] = (0.0, 0.0, 0.0, -180.0, 90.0, 0.0)
            tags[34735] = (1, 1, 0, 3, 1024, 0, 1, 2, 1025, 0, 1, 1, 2048, 0, 1, 4326)
            tags.tagtype[33550] = 12
            tags.tagtype[33922] = 12
            tags.tagtype[34735] = 3
            image.save(DATA / "natural-earth-rgb.tif", compression="raw", tiffinfo=tags)
            print(f"Prepared GeoTIFF: {width} x {height}, RGB, EPSG:4326")
        else:
            with Image.open(path) as image:
                if list(image.size) != source["dimensions"] or image.mode != "RGB":
                    raise ValueError(f"Unexpected source TIFF layout: {path}")
                image.load()
                print(f"Verified original GeoTIFF: {image.size}, {source['crs']}")


def publish():
    publisher.request("/health/ready")
    publisher.publish("font", "open:latin", "font/ttf", ROOT / "tests/fixtures/fonts/latin.ttf")
    for source in MANIFEST["sources"]:
        raster = source["id"] in ("raster", "landsat")
        filename = "natural-earth-rgb.tif" if source["id"] == "raster" else source["file"]
        path = DATA / (filename if raster else f"{source['id']}.zip")
        extra = {"X-Geello-CRS": source.get("crs", "EPSG:4326")}
        if raster:
            extra["X-Geello-Sample-Kind"] = "continuous"
        else:
            extra["X-Geello-Encoding"] = "UTF-8"
        publisher.publish("dataset", "open:" + source["id"],
                          "image/tiff" if raster else "application/zip", path, extra)
    for name in ("countries", "borders", "rivers", "cities", "raster"):
        publisher.publish("style", "open:" + name, "application/toml", EXAMPLE / f"{name}-style.toml")
    for name in MAPS:
        publisher.publish("map", "open:" + name, "application/toml", EXAMPLE / f"{name}-map.toml")


def verify(require_gpu, preview):
    from PIL import Image

    results = DATA / "results"
    results.mkdir(parents=True, exist_ok=True)
    health = json.loads(publisher.request("/health/ready"))
    if require_gpu and health["backend"] != "gpu":
        raise RuntimeError(f"GPU required, got: {health}")
    report = {"backend": health["backend"], "checks": []}

    def image_request(base, route, name, signature, check_colors=True):
        started = time.monotonic()
        try:
            with urlopen(base + route, timeout=180) as response:
                body = response.read()
                backend = response.headers.get("x-geello-backend")
        except HTTPError as error:
            raise RuntimeError(f"HTTP {error.code}: {route}: {error.read().decode()}") from error
        if not body.startswith(signature):
            raise ValueError(f"Invalid image: {route}")
        if require_gpu and base == publisher.BASE and backend != "gpu":
            raise RuntimeError(f"Render did not report GPU: {route}: {backend}")
        output = results / name
        output.write_bytes(body)
        with Image.open(output) as image:
            image.load()
            if check_colors and len(image.convert("RGB").getcolors(maxcolors=1) or []) == 1:
                raise ValueError(f"Uniform image: {route}")
            size = list(image.size)
        return {"file": name, "size": size, "backend": backend,
                "seconds": round(time.monotonic() - started, 3), "bytes": len(body)}

    for name in MAPS:
        geographic = [-79.1, 23.4, -76.4, 25.7] if name == "landsat" else [70, 15, 140, 55]
        if name == "landsat":
            west, south, east, north = geographic
            projected = [math.radians(west) * 6378137,
                         math.log(math.tan(math.pi / 4 + math.radians(south) / 2)) * 6378137,
                         math.radians(east) * 6378137,
                         math.log(math.tan(math.pi / 4 + math.radians(north) / 2)) * 6378137]
        else:
            projected = [-19926189, -15538711, 19926189, 15538711]
        for crs, bounds, suffix in (("OGC:CRS84", geographic, "region" if name == "landsat" else "china"),
                                    ("EPSG:3857", projected, "mercator" if name == "landsat" else "world")):
            bbox = ",".join(map(str, bounds))
            query = urlencode({"bbox": bbox, "crs": crs, "width": 960, "height": 600})
            report["checks"].append(image_request(publisher.BASE,
                f"/maps/open:{name}/image?{query}", f"{name}-{suffix}.png", b"\x89PNG"))
        query = urlencode({"SERVICE": "WMS", "VERSION": "1.3.0", "REQUEST": "GetMap",
                           "LAYERS": f"open:{name}", "STYLES": "", "CRS": "EPSG:4326",
                           "BBOX": ",".join(str(geographic[index]) for index in (1, 0, 3, 2)),
                           "WIDTH": 640, "HEIGHT": 400, "FORMAT": "image/jpeg"})
        report["checks"].append(image_request(publisher.BASE,
            "/wms?" + query, f"{name}-wms.jpg", b"\xff\xd8"))
        for matrix in ("WebMercatorQuad", "WorldCRS84Quad"):
            zoom = 6 if name == "landsat" else 2
            column, row = (18, 27) if name == "landsat" else (2, 1)
            if name == "landsat" and matrix == "WorldCRS84Quad":
                column, row = 36, 23
            query = urlencode({"SERVICE": "WMTS", "VERSION": "1.0.0", "REQUEST": "GetTile",
                               "LAYER": f"open:{name}", "STYLE": "default", "TILEMATRIXSET": matrix,
                               "TILEMATRIX": zoom, "TILEROW": row, "TILECOL": column, "FORMAT": "image/png"})
            report["checks"].append(image_request(publisher.BASE,
                "/wmts?" + query, f"{name}-{matrix}.png", b"\x89PNG", check_colors=False))
    for source in MANIFEST["sources"]:
        if "features" not in source:
            continue
        dataset = "open:" + source["id"]
        hits = json.loads(publisher.request("/wfs?" + urlencode({
            "SERVICE": "WFS", "VERSION": "2.0.0", "REQUEST": "GetFeature",
            "TYPENAMES": dataset, "RESULTTYPE": "hits"})))
        if hits["numberMatched"] != source["features"]:
            raise ValueError(f"Incorrect WFS count: {dataset}: {hits}")
        page = json.loads(publisher.request(f"/ogc/features/collections/{dataset}/items?limit=5"))
        if page["numberReturned"] != 5 or len(page["features"]) != 5:
            raise ValueError(f"Incorrect Features page: {dataset}")
        report["checks"].append({"dataset": dataset, "features": hits["numberMatched"],
                                 "features_page": page["numberReturned"]})
    if preview:
        render_time = time.time()

        def tile(index):
            row, column = divmod(index, 4)
            query = urlencode({"SERVICE": "WMTS", "VERSION": "1.0.0", "REQUEST": "GetTile",
                               "LAYER": "open:overlay", "STYLE": "default", "TILEMATRIXSET": "WebMercatorQuad",
                               "TILEMATRIX": 3, "TILEROW": row + 2, "TILECOL": column + 4,
                               "FORMAT": "image/png", "geello_time": render_time})
            return image_request(preview.rstrip("/"), "/service/wmts?" + query,
                                 f"burst-{index}.png", b"\x89PNG", check_colors=False)
        with ThreadPoolExecutor(max_workers=8) as pool:
            report["checks"].extend(pool.map(tile, range(8)))
    report["health_after"] = json.loads(publisher.request("/health/ready"))
    (results / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(f"PASS: {len(report['checks'])} checks; backend={health['backend']}; {results / 'report.json'}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", nargs="?", default=publisher.BASE)
    parser.add_argument("--prepare-only", action="store_true")
    parser.add_argument("--verify-only", action="store_true")
    parser.add_argument("--require-gpu", action="store_true")
    parser.add_argument("--preview", help="Also check eight simultaneous WMTS requests through this preview URL")
    args = parser.parse_args()
    if args.prepare_only and args.verify_only:
        parser.error("--prepare-only and --verify-only are mutually exclusive")
    publisher.BASE = args.base.rstrip("/")
    if not args.verify_only:
        prepare()
        if args.prepare_only:
            return
        publish()
    verify(args.require_gpu, args.preview)


if __name__ == "__main__":
    main()
