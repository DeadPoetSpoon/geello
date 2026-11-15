"""Native dependencies must fail before any implicit download/build fallback."""
import os
from pathlib import Path
import subprocess
import tempfile

env = os.environ.copy()
env.pop("GEELLO_NATIVE_ROOT", None)
with tempfile.TemporaryDirectory(prefix="geello-native-absent-") as directory:
    env["CARGO_TARGET_DIR"] = str(Path(directory) / "target")
    result = subprocess.run(["cargo", "check", "--locked", "--no-default-features", "--features", "native-proj"],
                            env=env, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    assert result.returncode != 0, result.stdout
    assert "native-proj requires GEELLO_NATIVE_ROOT" in result.stdout, result.stdout
    assert "building libproj from source" not in result.stdout, result.stdout
print("PASS: native dependencies absent; build stops with no bundled fallback")
