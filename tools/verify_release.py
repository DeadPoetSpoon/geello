"""Fail-closed release audit for the checked-out M6 source tree."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def fail(message):
    raise SystemExit("release audit failed: " + message)


def main():
    if subprocess.check_output(["git", "status", "--porcelain"], cwd=ROOT, text=True):
        fail("working tree must be clean")
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    if cargo["package"]["version"] != "0.2.0":
        fail("release version must match generated API version 0.2.0")
    if not any(package["name"] == "geello" and package["version"] == cargo["package"]["version"] for package in lock["package"]):
        fail("lockfile does not contain workspace version")
    if "native-proj" in cargo["features"]["server-full"] or "native-proj" in cargo["features"]["server"]:
        fail("native-proj leaked into ordinary server features")
    openapi = json.loads((ROOT / "src/server/http/openapi.json").read_text(encoding="utf-8"))
    if "/health/live" not in openapi["paths"]:
        fail("generated OpenAPI misses liveness route")
    for path in ["Dockerfile", "Dockerfile.cpu", "deploy/server.gpu.toml", "deploy/server.cpu.toml"]:
        text = (ROOT / path).read_text(encoding="utf-8")
        if "server.toml" not in text and path.startswith("Dockerfile"):
            fail(path + " has no config contract")
    native = json.loads((ROOT / "docs/refactor/m5-dependency-closure.json").read_text(encoding="utf-8"))
    expected = {"proj": "9.6.2", "sqlite": "3.54.1", "tiff": "4.7.2", "zlib": "1.4"}
    if {name: spec["version"] for name, spec in native["native_sources"].items()} != expected:
        fail("native source pins changed without evidence update")
    manifest = ROOT / "tests/fixtures/fonts/manifest.json"
    if hashlib.sha256(manifest.read_bytes()).hexdigest() != native["font_manifest_sha256"]:
        fail("font manifest hash drifted")
    subprocess.run([sys.executable, "tools/generate_http_docs.py", "--check"], cwd=ROOT, check=True)
    print(json.dumps({"status": "pass", "version": cargo["package"]["version"],
                      "lock_sha256": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest(),
                      "native_versions": expected}, indent=2))


if __name__ == "__main__":
    main()
