"""Exercise a built Alpine image with persistent storage and actual HTTP rendering."""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--image", required=True)
    p.add_argument("--cpu-only", action="store_true")
    p.add_argument("--require-gpu", action="store_true", help="requires real host Vulkan mapping via --device and ICD mounts")
    p.add_argument("--device", action="append", default=[])
    p.add_argument("--mount", action="append", default=[])
    a = p.parse_args()
    suffix = "cpu" if a.cpu_only else "gpu"
    with tempfile.TemporaryDirectory(prefix="geello-container-") as directory:
        config = (ROOT / f"deploy/server.{suffix}.toml").read_text()
        if a.require_gpu:
            config = config.replace('backend = "gpu-preferred"', 'backend = "gpu"')
        path = Path(directory) / "server.toml"
        path.write_text(config)
        volume = subprocess.check_output(["docker", "volume", "create"], text=True).strip()
        options = ["--mount", f"type=volume,src={volume},dst=/data", "--mount", f"type=bind,src={path},dst=/etc/geello/server.toml,readonly"]
        for device in a.device:
            options += ["--device", device]
        for mount in a.mount:
            options += ["--mount", mount]
        identity = None
        try:
            identity = subprocess.check_output(["docker", "run", "-d", "-p", "127.0.0.1::3000", *options, a.image], text=True).strip()
            port = subprocess.check_output(["docker", "port", identity, "3000/tcp"], text=True).strip().rsplit(":", 1)[1]
            base = "http://127.0.0.1:" + port
            for _ in range(120):
                try:
                    with urlopen(base + "/health/ready", timeout=1) as response:
                        ready = json.load(response)
                    break
                except OSError:
                    time.sleep(.25)
            else:
                raise RuntimeError(subprocess.check_output(["docker", "logs", identity], stderr=subprocess.STDOUT, text=True))
            assert ready["backend"] == ("gpu" if a.require_gpu else "cpu"), ready
            packages = subprocess.check_output(["docker", "exec", identity, "apk", "info"], text=True)
            if a.cpu_only:
                assert "vulkan" not in packages.lower(), packages
            else:
                assert "vulkan-loader" in packages, packages
            for kind, resource, media, relative in [
                ("dataset", "demo:cities", "application/geo+json", "examples/m4-protocols/data.geojson"),
                ("font", "demo:latin", "font/ttf", "tests/fixtures/fonts/latin.ttf"),
                ("font", "demo:cjk", "font/otf", "tests/fixtures/fonts/cjk.otf"),
                ("style", "demo:labels", "application/toml", "examples/m4-protocols/style.toml"),
                ("map", "demo:labels", "application/toml", "examples/m4-protocols/map.toml"),
            ]:
                req = Request(base + f"/admin/v1/resources/{kind}/{resource}/content", data=(ROOT / relative).read_bytes(),
                              method="PUT", headers={"Content-Type": media, "If-None-Match": "*"})
                with urlopen(req, timeout=30) as response:
                    operation = json.load(response)["operation_id"]
                for _ in range(200):
                    with urlopen(base + "/admin/v1/operations/" + operation, timeout=5) as response:
                        result = json.load(response)
                    assert result["status"] != "failed", result
                    if result["status"] == "ready": break
                    time.sleep(.1)
                assert result["status"] == "ready", result
            route = "/maps/demo:labels/image?bbox=110,30,125,45&crs=OGC:CRS84&width=256&height=256"
            with urlopen(base + route, timeout=30) as response:
                first = response.read()
            assert first.startswith(b"\x89PNG")
            subprocess.run(["docker", "stop", "--time", "30", identity], check=True)
            inspected = json.loads(subprocess.check_output(["docker", "inspect", identity], text=True))[0]
            assert inspected["State"]["ExitCode"] == 0, inspected["State"]
            subprocess.run(["docker", "start", identity], check=True)
            for _ in range(120):
                try:
                    with urlopen(base + route, timeout=3) as response: assert response.read() == first
                    break
                except OSError: time.sleep(.25)
            else: raise RuntimeError("persistent restart failed")
            print(json.dumps({"image": a.image, "ready": ready, "cpu_only": a.cpu_only,
                              "publication_render_restart_sigterm": "pass"}, indent=2))
        finally:
            if identity: subprocess.run(["docker", "rm", "-f", identity], check=False)
            subprocess.run(["docker", "volume", "rm", volume], check=False)


if __name__ == "__main__": main()
