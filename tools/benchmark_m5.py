"""M5 HTTP cold/hot/refresh/contention/restart workload and process RSS sampling.

Uses a built server, temporary owned storage and standard-library HTTP only.
Reports combined request times; phase timings come from m5_benchmark.rs.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import ctypes
import hashlib
import json
import os
from pathlib import Path
import platform
import random
import socket
import statistics
import subprocess
import tempfile
import threading
import time
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]


def rss(pid):
    if os.name == "nt":
        class Counters(ctypes.Structure):
            _fields_ = [("cb", ctypes.c_ulong), ("faults", ctypes.c_ulong)] + [(n, ctypes.c_size_t) for n in
                ("peak", "working", "qpeak", "quota", "qpeak2", "quota2", "page", "pagepeak")]
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        api = ctypes.WinDLL("psapi")
        api.GetProcessMemoryInfo.argtypes = [ctypes.c_void_p, ctypes.POINTER(Counters), ctypes.c_ulong]
        handle = kernel.OpenProcess(0x0400 | 0x0010, False, pid)
        if not handle: return None
        try:
            counter = Counters()
            counter.cb = ctypes.sizeof(counter)
            return counter.working if api.GetProcessMemoryInfo(handle, ctypes.byref(counter), counter.cb) else None
        finally:
            kernel.CloseHandle(handle)
    try:
        for line in Path(f"/proc/{pid}/status").read_text().splitlines():
            if line.startswith("VmRSS:"): return int(line.split()[1]) * 1024
    except OSError:
        return None


def stats(values, seconds):
    ordered = sorted(values)
    def quantile(q): return ordered[min(len(ordered) - 1, int((len(ordered) - 1) * q + .999999))]
    return {"count": len(values), "p50_ms": statistics.median(values), "p95_ms": quantile(.95),
            "p99_ms": quantile(.99), "requests_per_second": len(values) / seconds}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--binary", type=Path, default=ROOT / "target/release" / ("geello.exe" if os.name == "nt" else "geello"))
    p.add_argument("--backend", choices=["cpu", "gpu"], default="cpu")
    p.add_argument("--features", type=int, default=3000)
    p.add_argument("--requests", type=int, default=40)
    p.add_argument("--profile", type=Path, default=ROOT / "examples/server.experimental.toml")
    p.add_argument("--output", type=Path, required=True)
    a = p.parse_args()
    rng = random.Random(0x4D352026)
    features = [{"type": "Feature", "id": i, "geometry": {"type": "Point", "coordinates": [110 + rng.random() * 15, 30 + rng.random() * 15]},
                 "properties": {"name": ["中国地图", "office ffi"][i % 2], "payload": "x" * 512}} for i in range(a.features)]
    content = json.dumps({"type": "FeatureCollection", "features": features}, ensure_ascii=False).encode()
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0)); port = probe.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    with tempfile.TemporaryDirectory(prefix="geello-m5-") as directory:
        directory = Path(directory)
        config = a.profile.read_text()
        config = config.replace('127.0.0.1:3000', f'127.0.0.1:{port}').replace('../assets/test/m3-server-data', 'data')
        config = config.replace('backend = "gpu-preferred"', f'backend = "{a.backend}"')
        import re
        config = re.sub(r'^data_directory = .*$', 'data_directory = "data"', config, flags=re.M)
        config = re.sub(r'^backend = .*$', f'backend = "{a.backend}"', config, flags=re.M)
        path = directory / "server.toml"; path.write_text(config)
        process = None; peaks = []; stop = threading.Event()
        def sample():
            while not stop.wait(.01):
                if process and process.poll() is None:
                    value = rss(process.pid)
                    if value is not None: peaks.append(value)
        sampler = threading.Thread(target=sample, daemon=True); sampler.start()
        def start():
            nonlocal process
            process = subprocess.Popen([str(a.binary.resolve()), "--config", str(path)], stdout=subprocess.DEVNULL, stderr=log)
            tick = time.perf_counter()
            for _ in range(400):
                try:
                    _, data, _ = request("/health/ready")
                    assert json.loads(data)["backend"] == a.backend
                    return (time.perf_counter() - tick) * 1000
                except OSError:
                    if process.poll() is not None: raise RuntimeError((directory / "server.log").read_text())
                    time.sleep(.025)
            raise TimeoutError("startup")
        def request(route, data=None, media=None, etag="*", method=None):
            headers = {"Content-Type": media, "If-None-Match" if etag == "*" else "If-Match": etag} if media else {}
            tick = time.perf_counter()
            with urlopen(Request(base + route, data=data, headers=headers, method=method or ("PUT" if media else "GET")), timeout=90) as response:
                body = response.read(); response_headers = dict(response.headers)
            return (time.perf_counter() - tick) * 1000, body, response_headers
        def publish(kind, resource, media, data, etag="*"):
            tick = time.perf_counter()
            operation = json.loads(request(f"/admin/v1/resources/{kind}/{resource}/content", data, media, etag)[1])["operation_id"]
            for _ in range(2000):
                status = json.loads(request("/admin/v1/operations/" + operation)[1])
                assert status["status"] != "failed", status
                if status["status"] == "ready": return (time.perf_counter() - tick) * 1000
                time.sleep(.01)
            raise TimeoutError("publication")
        try:
            with (directory / "server.log").open("w") as log:
                startup = start()
                publication = publish("dataset", "demo:cities", "application/geo+json", content)
                for kind, identity, media, file in [
                    ("font", "demo:latin", "font/ttf", "tests/fixtures/fonts/latin.ttf"),
                    ("font", "demo:cjk", "font/otf", "tests/fixtures/fonts/cjk.otf"),
                    ("style", "demo:labels", "application/toml", "examples/m4-protocols/style.toml"),
                    ("map", "demo:labels", "application/toml", "examples/m4-protocols/map.toml")]:
                    publish(kind, identity, media, (ROOT / file).read_bytes())
                route = "/maps/demo:labels/image?bbox=110,30,125,45&crs=OGC:CRS84&width=512&height=512"
                cold, image, headers = request(route)
                assert image.startswith(b"\x89PNG")
                tick = time.perf_counter(); hot = [request(route)[0] for _ in range(a.requests)]
                hot = stats(hot, time.perf_counter() - tick)
                # Distinct crops share no response entry; query, preparation and
                # backend work run under the real scheduler and cache budgets.
                tick = time.perf_counter()
                with ThreadPoolExecutor(max_workers=4) as pool:
                    concurrent = list(pool.map(lambda i: request(route + f"&geello_time={i + 1}")[0], range(a.requests)))
                concurrent = stats(concurrent, time.perf_counter() - tick)
                _, body, resource_headers = request("/admin/v1/resources/dataset/demo:cities")
                etag = next(v for k,v in resource_headers.items() if k.lower() == "etag")
                features[0]["geometry"]["coordinates"] = [120., 40.]
                changed = json.dumps({"type": "FeatureCollection", "features": features}, ensure_ascii=False).encode()
                refresh = publish("dataset", "demo:cities", "application/geo+json", changed, etag)
                after, _, changed_headers = request(route)
                # Refresh must invalidate the map's pinned revision/cache key.
                old_revision = next(v for k,v in headers.items() if k.lower() == "x-geello-render-revision")
                new_revision = next(v for k,v in changed_headers.items() if k.lower() == "x-geello-render-revision")
                assert old_revision != new_revision
                # Force-stop recovery also exercises journal/derived restart.
                process.kill(); process.wait(timeout=10)
                restart = start()
                recovered, recovered_image, _ = request(route)
                assert recovered_image.startswith(b"\x89PNG")
                disk = sum(f.stat().st_size for f in directory.rglob("*") if f.is_file())
                output = {"schema_version": 1, "seed": "0x4d352026", "platform": platform.platform(), "backend": a.backend,
                    "features": a.features, "source_sha256": hashlib.sha256(content).hexdigest(), "source_bytes": len(content),
                    "binary_sha256": hashlib.sha256(a.binary.read_bytes()).hexdigest(), "profile": config,
                    "startup_ms": startup, "publication_ms": publication, "cold_request_ms": cold, "hot": hot,
                    "four_clients_distinct_time": concurrent, "refresh_ms": refresh, "after_refresh_ms": after,
                    "forced_restart_ms": restart, "recovery_render_ms": recovered, "sampled_peak_rss_bytes": max(peaks) if peaks else None,
                    "disk_bytes_after_restart": disk, "rss_sampling_ms": 10, "cached_result_sha256": hashlib.sha256(image).hexdigest()}
                a.output.parent.mkdir(parents=True, exist_ok=True)
                a.output.write_text(json.dumps(output, indent=2), encoding="utf-8")
                print(a.output)
        finally:
            stop.set(); sampler.join(timeout=2)
            if process and process.poll() is None: process.terminate(); process.wait(timeout=10)


if __name__ == "__main__": main()
