"""Publish 230800_1.shp and the jd imagery/outline composition (stdlib only)."""

import argparse
from collections import Counter
import json
import struct
import zipfile

import publish_imagery_demo as publisher

ROOT = publisher.ROOT
PAGE = ROOT / "examples/web-preview"
ASSET = PAGE / "annotations"
PALETTE = ["#00d9ff", "#ffbd45", "#76ff69", "#ff6ba8"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base", nargs="?", default=publisher.BASE)
    args = parser.parse_args()
    publisher.BASE = args.base.rstrip("/")
    # Reuse the original imagery already published; avoid uploading 1 GB again.
    publisher.request("/admin/v1/resources/dataset/preview:jd")
    source = ROOT / "assets/test/230800_1.shp"
    generated = ROOT / "assets/test/generated/web-preview"
    generated.mkdir(parents=True, exist_ok=True)
    archive = generated / "230800_1.zip"
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as output:
        for extension in (".shp", ".shx", ".dbf", ".prj", ".cpg"):
            path = source.with_suffix(extension)
            output.write(path, path.name)
    # Read only the DBF type column to generate deterministic matching colors.
    counts = Counter()
    with source.with_suffix(".dbf").open("rb") as dbf:
        header = dbf.read(32)
        records = struct.unpack_from("<I", header, 4)[0]
        header_size, record_size = struct.unpack_from("<HH", header, 8)
        offset = 1
        type_column = None
        for _ in range((header_size - 33) // 32):
            field = dbf.read(32)
            if field[:11].split(b"\0")[0] == b"LXMC":
                type_column = (offset, field[16])
            offset += field[16]
        if type_column is None:
            raise ValueError("LXMC column is missing")
        dbf.seek(header_size)
        for _ in range(records):
            record = dbf.read(record_size)
            if record[:1] != b"*":
                start, size = type_column
                counts[record[start:start + size].decode("utf-8").strip()] += 1
    categories = [{"value": name, "color": PALETTE[i % len(PALETTE)], "count": count}
                  for i, (name, count) in enumerate(counts.items())]
    ASSET.mkdir(exist_ok=True)
    style = ['schema_version = 1', 'id = "preview:annotations"']
    for index, category in enumerate(categories):
        style.extend([
            '', '[[rules]]', f'id = "type-{index}"',
            'filter = { op = "eq", field = "/LXMC", value = ' + json.dumps(category["value"], ensure_ascii=False) + ' }',
            '[[rules.symbols]]', 'id = "outline"', 'kind = "line"',
            'rings = "all"', 'width = { value = 2.0, unit = "px" }',
            f'color = "{category["color"]}"',
        ])
    (ASSET / "style.toml").write_text("\n".join(style) + "\n", encoding="utf-8")
    publisher.publish("dataset", "preview:annotations", "application/zip", archive,
                      {"X-Geello-CRS": "EPSG:3857", "X-Geello-Encoding": "UTF-8"})
    publisher.publish("style", "preview:annotations", "application/toml", ASSET / "style.toml")
    publisher.publish("map", "preview:jd-annotations", "application/toml", ASSET / "map.toml")
    config_path = PAGE / "views.json"
    config = json.loads(config_path.read_text(encoding="utf-8"))
    config["defaultMap"] = "preview:jd-annotations"
    config["maps"]["preview:jd-annotations"] = {
        "bounds": config["maps"]["preview:jd"]["bounds"],
        "wfsDataset": "preview:annotations", "baseMap": "preview:jd",
        "typeField": "LXMC", "categories": categories,
    }
    config_path.write_text(json.dumps(config, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"features": sum(counts.values()), "categories": categories}, ensure_ascii=True))
    print("Preview: http://127.0.0.1:8080/?map=preview%3Ajd-annotations")
    print("WFS: http://127.0.0.1:8080/?map=preview%3Ajd-annotations&protocol=wfs")


if __name__ == "__main__":
    main()
