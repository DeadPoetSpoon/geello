"""Write a deterministic CycloneDX-style JSON inventory from Cargo.lock."""
from pathlib import Path
import hashlib
import json
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def main():
    metadata = json.loads(subprocess.check_output(["cargo", "metadata", "--locked", "--all-features", "--format-version", "1"], cwd=ROOT, text=True))
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text(encoding="utf-8"))
    checksums = {(p["name"], p["version"]): p.get("checksum") for p in lock["package"]}
    components = []
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        components.append({"type": "library", "name": package["name"], "version": package["version"],
                           "purl": package["id"], "hashes": ([{"alg": "SHA-256", "content": checksums[(package["name"], package["version"])]}]
                                      if checksums[(package["name"], package["version"])] else []),
                           "licenses": ([{"license": {"id": package["license"]}}] if package.get("license") else [])})
    result = {"bomFormat": "CycloneDX", "specVersion": "1.5", "serialNumber": "urn:uuid:geello-cargo-lock",
              "metadata": {"component": {"type": "application", "name": "geello", "version": "0.2.0"},
                            "properties": [{"name": "cargo.lock.sha256", "value": hashlib.sha256((ROOT / "Cargo.lock").read_bytes()).hexdigest()}]},
              "components": components}
    output = ROOT / "docs/refactor/m6-sbom.json"
    output.write_text(json.dumps(result, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(output)


if __name__ == "__main__": main()
