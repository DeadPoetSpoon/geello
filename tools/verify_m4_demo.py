"""Offline end-to-end M4 demo using the built server and Python standard library."""
import hashlib
import importlib.util
import json
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]
sys.dont_write_bytecode = True


def main():
    manifest = json.loads((ROOT / "tests/fixtures/fonts/manifest.json").read_text(encoding="utf-8"))
    for font in manifest["fonts"]:
        for name, digest in [(font["fixture"], font["fixture_sha256"]), (font["license"], font["license_sha256"])]:
            assert hashlib.sha256((ROOT / "tests/fixtures/fonts" / name).read_bytes()).hexdigest() == digest, name
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    with tempfile.TemporaryDirectory(prefix="geello-m4-") as directory:
        directory = Path(directory)
        config = (ROOT / "examples/server.experimental.toml").read_text(encoding="utf-8")
        config = config.replace('listen = "127.0.0.1:3000"', f'listen = "127.0.0.1:{port}"')
        config = config.replace('public_base_url = "http://127.0.0.1:3000"', f'public_base_url = "{base}"')
        config = config.replace('data_directory = "../assets/test/m3-server-data"', 'data_directory = "data"')
        config = config.replace('backend = "gpu-preferred"', 'backend = "cpu"')
        path = directory / "server.toml"
        path.write_text(config, encoding="utf-8")
        binary = ROOT / "target/debug" / ("geello.exe" if __import__("os").name == "nt" else "geello")
        with (directory / "server.log").open("w") as log:
            process = subprocess.Popen([str(binary), "--config", str(path)], cwd=ROOT,
                stdout=log, stderr=log, creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
            try:
                for _ in range(200):
                    try:
                        with urlopen(base + "/health/ready", timeout=1) as result:
                            assert json.load(result)["backend"] == "cpu"
                        break
                    except OSError:
                        assert process.poll() is None, (directory / "server.log").read_text()
                        time.sleep(0.05)
                else:
                    raise TimeoutError("Server startup")

                def request(route, data=None, media=None):
                    headers = {"Content-Type": media, "If-None-Match": "*"} if media else {}
                    with urlopen(Request(base + route, data=data, headers=headers,
                            method="PUT" if media else "GET"), timeout=30) as result:
                        return result.read()

                for kind, identity, media, relative in [
                    ("font", "demo:latin", "font/ttf", "tests/fixtures/fonts/latin.ttf"),
                    ("font", "demo:cjk", "font/otf", "tests/fixtures/fonts/cjk.otf"),
                    ("dataset", "demo:cities", "application/geo+json", "examples/m4-protocols/data.geojson"),
                    ("style", "demo:labels", "application/toml", "examples/m4-protocols/style.toml"),
                    ("map", "demo:labels", "application/toml", "examples/m4-protocols/map.toml"),
                ]:
                    operation = json.loads(request(f"/admin/v1/resources/{kind}/{identity}/content", (ROOT / relative).read_bytes(), media))
                    for _ in range(300):
                        state = json.loads(request("/admin/v1/operations/" + operation["operation_id"]))
                        if state["status"] == "ready":
                            break
                        assert state["status"] != "failed", state
                        time.sleep(0.05)
                    else:
                        raise TimeoutError(f"Publication: {identity}")
                assert request("/maps/demo:labels/image?bbox=110,30,125,45&crs=OGC:CRS84&width=512&height=512").startswith(b"\x89PNG")
                assert request("/wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=demo:labels&STYLES=&CRS=EPSG:4326&BBOX=30,110,45,125&WIDTH=512&HEIGHT=512&FORMAT=image/jpeg").startswith(b"\xff\xd8")
                assert request("/wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile&LAYER=demo:labels&STYLE=default&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX=0&TILEROW=0&TILECOL=0&FORMAT=image/png").startswith(b"\x89PNG")
                page = json.loads(request("/ogc/features/collections/demo:cities/items?bbox=110,30,125,45&limit=1"))
                assert page["numberMatched"] == 2 and page["numberReturned"] == 1
                next_url = next(link["href"] for link in page["links"] if link["rel"] == "next")
                with urlopen(next_url, timeout=15) as result:
                    assert json.load(result)["features"][0]["id"] == "shanghai"
                hits = json.loads(request("/wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=demo:cities&RESULTTYPE=hits"))
                assert hits["numberMatched"] == 2 and hits["numberReturned"] == 0
                xml = (ROOT / "examples/m4-protocols/query.xml").read_bytes()
                with urlopen(Request(base + "/wfs", data=xml, headers={"Content-Type": "application/xml"}), timeout=15) as result:
                    assert json.load(result)["numberReturned"] == 2
                spec = importlib.util.spec_from_file_location("m4_client", ROOT / "examples/m4-protocols/websocket-client.py")
                client = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(client)
                client.capture(f"ws://127.0.0.1:{port}/ws/maps/demo:labels", directory / "frame.png")
                print("M4 demo PASS: font/license hashes, publication, Unicode map, WMS JPEG/axes, WMTS, snapshot, WFS KVP/XML and real WebSocket pair")
            finally:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=5)


if __name__ == "__main__":
    main()
