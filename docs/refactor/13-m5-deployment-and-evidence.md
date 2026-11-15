# M5 deployment, performance and evidence

Status: implemented infrastructure with measured Windows and Linux CPU/native evidence. Linux hardware Vulkan and Alpine container execution remain unavailable on this host; the associated acceptance claims stay open.

## Native projection

`native-proj` is opt-in and is deliberately absent from `server-full`, the default feature set and both ordinary container builds. It uses a reviewed local `proj-sys 0.27.0` patch plus `proj 0.31.0`. The patch removes upstream's automatic system/source fallback and requires `GEELLO_NATIVE_ROOT` to contain a target-specific `geello-native.json`. The build and runtime checks verify exact PROJ 9.6.2, SQLite 3.54.1, libtiff 4.7.2 and zlib 1.4 versions, SHA-256 for every listed file, `proj.db`, dynamic linking, disabled network access, two-dimensional CRS types and strict `ALLOW_BALLPARK=NO`/`ONLY_BEST=YES` operations. No grid is bundled; a required missing grid fails the operation.

Build a distribution with `python tools/build_native_proj.py`, then set `GEELLO_NATIVE_ROOT` to its printed `install` directory and `PROJ_NETWORK=OFF`. Windows and Linux manifests are recorded in [m5-native-windows.json](m5-native-windows.json) and [m5-native-linux.json](m5-native-linux.json). `python tools/verify_native_gate.py` proves the absent-distribution failure path. The native contract tests cover round-trip XY, invalid CRS, missing grid, source identity, cancellation release and explicit native service cache identity.

## Images and source deployments

`Dockerfile` is the GPU-preferred Alpine image: a pinned Rust 1.96 Alpine build stage produces x86_64 musl, the runtime includes the Vulkan loader and CPU renderer, runs as UID 10001, persists `/data`, and handles SIGTERM. Vendor Vulkan devices and ICD paths must be mapped by the deployment platform; loader presence is not GPU evidence. `Dockerfile.cpu` builds an independent CPU-only feature set and runtime without the Vulkan package. `tools/verify_container.py` publishes fixtures, renders, sends SIGTERM, restarts with the same volume and checks the CPU/GPU package boundary. Docker registry digests are intentionally not hard-coded until registry access is available; the image tags are pinned to Rust 1.96.0/Alpine 3.23.3 in the Dockerfile and should be replaced with verified digests before release.

For a source deployment, install Rust 1.96.0, CMake, a C/C++ toolchain and the target Rust target. Run `cargo build --locked --release --no-default-features --features server-full` for a native host, or use the Dockerfiles for musl. Copy one of `deploy/server.gpu.toml`, `deploy/server.cpu.toml`, `deploy/server.development.toml` or `deploy/server.small.toml`; change `data_directory` to an owned persistent directory, set an explicit external base URL, put TLS/authentication in the reverse proxy, and run `geello --config /path/server.toml`. The service accepts SIGINT and SIGTERM and drains admission before closing.

Catalog files are authoritative only after atomic publication. On migration, stop writes, copy the complete data directory, run `geello` once to recover the journal, inspect `reconcile`/`reconcile_derived`, and take a snapshot before removing any retired generation. A changed source requires an explicit refresh and creates a new derived version; do not edit derived shards or `proj.db` in place.

## Fixed-seed measurements

`cargo run --release --example m5_benchmark --features server-full -- 3000 2000 cpu` measures source parse, derived generation/storage, derived query, projection, style resolution, raster colorize, multilingual layout/scene, execution/readback, PNG encoding, repeated render latency, cancellation release, catalog publication, four-writer updates, lookup, restart, rebuild and cleanup. It uses seed `0x4d352026`, 3,000 synthetic vector features, 2,000 catalog records and 1,024² raster samples. `tools/benchmark_m5.py` measures HTTP startup, publication, cold/warm cache, distinct concurrent requests, refresh, forced restart, recovery render, RSS sampling and disk growth. Current artifacts are `target/m5-phases-cpu.json`, `target/m5-phases-gpu.json`, `target/m5-http-cpu.json`, `target/m5-http-gpu.json`, `target/m5-development.json` and `target/m5-small.json`.

Windows 11 / Xeon W-2255 / RTX 4090 DX12 and Vulkan evidence is real: GPU initialization was about 876–1,362 ms and repeated render p50 was about 26.6 ms (Vulkan) / 19.2 ms (DX12) for the larger multilingual scene; CPU p50 was about 19.2 ms. The HTTP CPU run measured cold render about 3.7–5.1 s, warm p50 about 7.7–15.0 ms, four-client distinct-request p95 about 14–19 s, peak sampled RSS about 101–154 MiB in the development/small profiles, refresh about 1.5–1.8 s and forced restart about 0.5 s. These are workstation observations, not service-level objectives.

The catalog run measured 2,000 records, 10,000 lookups at roughly 0.0014–0.0015 ms p95, restart about 111–142 ms and rebuild about 86–161 ms. Four-writer updates completed with per-worker times in the approximately 0.6–3.2 s range. Disk/RSS values are estimates from the harness; GPU residency excludes driver/Vello allocations. Cancellation tests report zero retained accounted bytes. Development and small profile values are proposed starting points only; G-07 owner approval of latency, throughput, queue, memory, GPU and catalog limits remains open.

## Platform matrix

| Check | Result | Evidence |
| --- | --- | --- |
| Windows MSVC CPU/server/full | Pass | `target/m5-windows-tests.log` |
| Windows DX12 real GPU | Pass | `target/m5-dx12.log`, RTX 4090, driver 32.0.16.2114 |
| Windows native PROJ | Pass | `target/m5-native-test.log`, `target/m5-native-server.log` |
| WSL Linux x86_64 CPU/native | Pass | `target/m5-linux-tests.log`, `/root/m5-linux-final.log` |
| WSL Vulkan hardware | Unavailable | actual adapter was llvmpipe; test rejects `DeviceType::Cpu` |
| Alpine CPU/GPU containers | Unavailable | Docker daemon/service unavailable or timed out |
| Native absent fail-fast | Pass | `target/m5-native-gate.log` |
| HTTP docs freshness | Pass | `python tools/generate_http_docs.py --check` |

