# G-07 approved performance and resource targets

Approval date: **2026-11-15**. Trace: **G-07, OPS-005, T-52**.
The owner explicitly confirmed the recommended values after reviewing the M5
measurements. The profile and target approval requirement is **Accepted/closed**.
Validation remains specific to the workload, profile, platform and backend below;
approval alone does not establish a passing performance result.

## Approved workload and targets

The targets apply to the `development` and `small` profiles. The initial platform
scope is Windows CPU/DX12 and Linux x86_64 CPU. Measure each combination
separately; CPU results cannot validate GPU or Linux performance. Linux hardware
Vulkan and both Alpine image variants retain their separate acceptance gates.
No separate GPU throughput number or GPU memory ceiling was proposed or approved.

Use the M5 HTTP harness, seed `0x4d352026`, 3,000 generated point features,
committed Latin/CJK font fixtures, the protocol example style/map, a 512 x 512
CRS84 PNG, and the selected profile. The workload uses loopback HTTP without a
reverse proxy. These are acceptance targets for this benchmark, not a general
business-dataset or internet-facing service-level guarantee.

| Metric | Approved target | Measurement |
| --- | --- | --- |
| Warm response latency | p95 <= 50 ms | Repeated sequential requests to the same cached image |
| Warm response throughput | >= 60 requests/s | Successful sequential cached requests divided by elapsed time |
| Cold render latency | p95 <= 8 s | Repeated independent runs with no cached response for the first image request |
| Four-client distinct-request latency | p95 <= 20 s | Four workers, unique `geello_time` values, including queue time |
| Dataset refresh | <= 3 s | Conditional upload through operation status `ready`; next render is measured separately |
| Forced restart readiness | <= 2 s | Process start to `/health/ready` after a forced stop; recovery image rendering is separate |
| Live accounted memory | <= 512 MiB | `max_live_bytes = 536870912`, an application admission ceiling |
| Small-profile sampled peak RSS | <= 256 MiB | `sampled_peak_rss_bytes <= 268435456`, sampled every 10 ms during the harness |

The 512 MiB admission ceiling and the 256 MiB sampled RSS target measure different
things. Reservations can be conservative; driver, allocator, transport and cache
internals can add allocations. RSS is a benchmark target, not an OS-enforced
process limit. No 256 MiB RSS target was approved for `development` or for arbitrary
workloads. The 60 FPS configuration ceiling is not a sustained playback promise.

## Approved profile settings

The numerical `[profile]` settings in
[`server.development.toml`](../../deploy/server.development.toml) and
[`server.small.toml`](../../deploy/server.small.toml) are retained as the approved
starting profiles. They differ in HTTP concurrency and metatiling:

| Setting | Development | Small |
| --- | --- | --- |
| Active HTTP / animation / background | 1 / 1 / 1 | 2 / 1 / 1 |
| Queued work per class | 4 | 4 |
| Metatile size / buffer | 1 / 32 px | 2 / 64 px |
| Operation timeout | 60 s | 60 s |
| Object cache / response memory / response disk | 64 / 16 / 128 MiB | 64 / 16 / 128 MiB |
| Feature cursors / absolute TTL | 128 / 60 s | 128 / 60 s |
| Animation FPS ceiling | 60 | 60 |

Other retained limits include 16 MiB uploads, 32 MiB expanded/source bytes,
10,000 resource/operation admission limits, 100,000 features, 1 GiB each for
managed/derived data, 16 MiB image bytes and maximum image dimension 4,096.
These are allowed admission limits, not evidence that maximum-sized inputs meet
the latency targets. Container profiles (`server.cpu.toml`/`server.gpu.toml`) and
the experimental example have different settings and are outside this approval.

## Existing evidence and remaining validation

The M5 Windows CPU artifacts are retained at `target/m5-development.json` and
`target/m5-small.json`; they identify source/binary hashes and the exact profile.
The source SHA-256 is
`f79a2cb2285ab3d880a5b821cec44467cc40fbbc2fe1919711504e8639d3fff4`, and
the measured binary SHA-256 is
`7dc36b1b1f4a9723738491c8ebe89684bd8485620a77ff08e2caf31c8f016a64`.

| M5 observation | Development | Small |
| --- | --- | --- |
| Warm p95 | 25.78 ms | 16.65 ms |
| Warm throughput | 89.62 requests/s | 85.05 requests/s |
| Four-client distinct-request p95 | 19.26 s | 14.04 s |
| Single cold request | 4.42 s | 5.10 s |
| Refresh | 1.48 s | 1.76 s |
| Forced restart readiness | 0.52 s | 0.51 s |
| Sampled peak RSS | 96.15 MiB | 147.25 MiB |

Each artifact contains 12 warm and 12 concurrent samples but only one cold
request. They meet the observed warm/concurrent/refresh/restart thresholds and
the small-profile RSS target, but do not establish cold-render p95, sustained
load behavior or performance on the current source revision. Repeated cold
samples and fresh, backend-specific Windows DX12/Linux CPU performance runs
remain required before claiming the complete target matrix validated.

Replay with a current release binary using:

```text
python tools/benchmark_m5.py --profile deploy/server.development.toml --backend cpu --requests 40 --output target/g07-development.json
python tools/benchmark_m5.py --profile deploy/server.small.toml --backend cpu --requests 40 --output target/g07-small.json
```

Repeat independent runs to collect cold samples; a single run still measures
one cold request. Record toolchain, hardware, backend, binary hash and workload
identity alongside new evidence. Correctness/platform tests, release source
audits, and owner approval remain distinct from performance validation.
