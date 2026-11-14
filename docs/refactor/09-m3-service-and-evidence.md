# M3 basic service implementation and evidence

Implementation date: **2026-11-13**, branch `refactor`, working tree based on M2
commit `4ffbd96`. This describes actual interfaces alongside the accepted target
design. It does not declare OGC conformance, production capacity or container
validation. G-01 remains open: the owner authorized candidate-closure tests and
approved the ZIP direct version/features; the [full inventory](08-m3-dependency-closure.md)
still needs formal review.

## Modules and ownership

| Boundary | Implementation | Responsibility |
| --- | --- | --- |
| Library layer two | `src/derived/vector.rs` | Immutable bounded FlatGeobuf shards and checksummed lossless companion records, identity lookup and original source order |
| Library layer two | `src/derived/raster.rs` | Lossless typed native blocks, validity/NoData/alpha, source-wide calibrated display ranges, independent nearest or weighted overview versions |
| Library encoding | `src/encoding.rs` | Budgeted PNG/JPEG encoding after execution; JPEG explicitly supplies quality/background |
| Server | `src/server/catalog.rs` | Single writer, resource graph, optimistic conditions, pins, sharded COW index, atomic generation, deletion recovery, explicit maintenance |
| Server | `src/server/runtime.rs` | Staging/preparation/publication, source fingerprints, versions, lazy repairs/overviews, backend orchestration, caches and singleflight |
| Server | `src/server/archive.rs` | Restricted Shapefile ZIP validation and streamed extraction |
| Server | `src/server/scheduler.rs` | Bounded HTTP/animation/background admission and cancellation |
| Server | `src/server/api.rs`, `src/main.rs` | Axum adapters and explicit startup/shutdown |

Neither the memory layer nor the derived store requires a server runtime. Scene
execution still uses the shared immutable scene and library projection/style
contracts. Server rendering does not reopen original sources or mutable style
files: it pins indexed definitions and immutable derived versions first.

## Run the experimental service

```text
cargo run --locked --features server-full -- --config examples/server.experimental.toml
```

The [experimental profile](../../examples/server.experimental.toml) binds to
loopback, has no external roots, writes under ignored `assets/test/`, and selects
`gpu-preferred`. Every capacity is explicit and provisional pending G-07. Paths
in startup TOML resolve against that file. For a CPU-only build use
`--no-default-features --features server,render-cpu,read-geojson,read-shapefile,read-geotiff`
and change the configuration backend to `cpu`. A GPU-only build must select
`gpu`, since initialization fallback requires a compiled CPU backend.

The proxy must protect `/admin/v1`; there is no built-in identity/authentication
or TLS service. Bind privately until that proxy is configured. No Dockerfile,
Alpine runtime or native PROJ implementation is delivered by M3.

| Feature | Capability |
| --- | --- |
| `derived-store` | Layer-two vector/raster formats, no Axum/Tokio requirement |
| `encode-images` | PNG/JPEG encoding, independent of either rendering backend |
| `server` | Axum/Tokio, configuration/projection/storage/encoding/cache/archive adapters; explicitly select at least one backend and desired readers |
| `server-full` | Server, three readers, CPU and GPU; excludes native PROJ |

## Implemented HTTP contracts

Identifiers are `namespace:local`, with each component beginning with an ASCII
letter or underscore. Administrative resource kinds are `dataset`, `style`,
`map`, `animation`, `font`. Font acceptance currently checks the sfnt container
signature only; typography and full font validation remain M4/G-04.

| Method and route | Current behavior |
| --- | --- |
| `GET /health/ready` | Ready backend, initialization diagnostic, catalog generation/counts, accounted live bytes and cache failure count |
| `GET /admin/v1/resources?offset=0&limit=20` | Bounded indexed list and generation; administrative offset browsing is not M4 snapshot feature pagination |
| `GET /admin/v1/resources/{kind}/{id}` | Current record and strong resource ETag |
| `PUT /admin/v1/resources/{kind}/{id}/content` | Raw streamed single file or Shapefile ZIP; `202` and operation record after upload completes |
| `PUT /admin/v1/resources/{kind}/{id}/source` | External path registration JSON; `202` and operation record |
| `POST /admin/v1/resources/{kind}/{id}:unpublish` | Atomically stop new use; return updated record/ETag; existing pins can finish |
| `DELETE /admin/v1/resources/{kind}/{id}` | Remove record, retain managed/external original content; `204` |
| `DELETE /admin/v1/resources/{kind}/{id}/content` | Managed content and record deletion with recovery journal; external ownership returns `409`; success `204` |
| `GET /admin/v1/operations/{operationId}` | Persistent preparation status/result/diagnostic |
| `GET /maps/{id}/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=256&height=256` | Basic XY bbox image, PNG, stretch aspect policy, published output CRS only |
| `GET /tiles/{id}/{z}/{x}/{y}` | Basic WebMercatorQuad PNG, 256 pixels, buffered metatile; z in 0..30 |

All writes require exactly one strong `If-Match: "resource-version"`, or
`If-None-Match: *` for creation. Missing conditions return `428`; stale versions
and reference/ownership conflicts return `409`. Conflict bodies include bounded
reference keys when available. Validation failures use `400`/`422`; unsupported
media `415`; capacity and preparing errors `503`; oversized uploads `413`.
Errors produced by application handlers use RFC 9457 JSON. Axum extractor and
routing rejections are not yet normalized to that schema.

Upload media: GeoJSON `application/geo+json`, GeoTIFF `image/tiff`, Shapefile
`application/zip`, definition `application/toml`, fonts `font/ttf`, `font/otf` or
`font/collection`. Optional headers `X-Geello-CRS`, `X-Geello-Encoding` and
`X-Geello-Sample-Kind` provide interpretation. Raster sample kind must be explicitly
`continuous` or `categorical`. Dataset TOML uses the M2 schema; a managed dataset
definition must refer to a source already registered in the catalog.

Example using curl (use `curl.exe` on Windows PowerShell):

```sh
curl --request PUT 'http://127.0.0.1:3000/admin/v1/resources/dataset/demo:points' \
  --header 'If-None-Match: *' --header 'Content-Type: application/geo+json' \
  --data-binary '@points.geojson'
curl --request PUT 'http://127.0.0.1:3000/admin/v1/resources/dataset/demo:roads' \
  --header 'If-None-Match: *' --header 'Content-Type: application/zip' \
  --data-binary '@roads.zip'
curl 'http://127.0.0.1:3000/admin/v1/operations/OPERATION_ID'
```

External registration body (absolute path inside an explicitly configured root):

```json
{"path":"C:/geello-input/points.geojson","format":"geojson","crs":"OGC:CRS84","encoding":null,"sample_kind":null}
```

Operation response shape:

```json
{"operation_id":"op-example","resource":{"kind":"dataset","id":"demo:points"},"status":"preparing","version":null,"diagnostics":[]}
```

Operations progress `staging` → `preparing` → `ready` or `failed`. The
`unpublished` status exists in the schema; current synchronous unpublish returns
the resource itself, without creating an operation. Upload preparation is eager
before publication. A missing derived manifest triggers one lazy deduplicated
repair on first use, responds `503 preparing` with its operation ID, and serves
only after the same DataVersion is restored. A changed source makes that repair
fail; an explicit conditional registration/upload is required to refresh.

## Publication, storage and retention

The directory contains `managed/`, `external/`, `registry/shards/`,
`registry/generations/`, `registry/deletions/`, `staging/`, `operations/`,
`derived/`, and `response-cache/`. `registry/catalog.toml` is the generation
pointer. Requests consult the loaded index, never recursively scan resource
directories. Definitions are snapshotted in immutable indexed records as well
as their original files; changes to an external file do not silently alter
serving state. Explicit re-registration is the refresh mechanism.

Only the affected type/namespace/hash index shard is rewritten; publication
rechecks the current ETag and resource graph. No cascade occurs. Unpublished
dependents still protect removal/deletion; complete in-flight graphs protect
physical versions. Record-only removal tracks detached managed bytes separately
from retired versions. Managed deletion journals the rename to private trash,
then commits the record removal; restart rolls back before commit or purges
after commit. An interrupted operation whose version was committed recovers as
`ready`; other interrupted operations become `failed`.

`Catalog::collect_garbage` reclaims only explicitly retired unpinned versions;
`reconcile` reports bounded unknown managed directories without adopting or
deleting them. `rebuild_index` reconstructs shards from authoritative loaded
records; `compact_index` removes obsolete index files under the single-writer
mutex. There is no import-from-directory reconstruction after loss of the
authoritative catalog. Runtime GC also reclaims unregistered hash-verified
derived directories and abandoned private derived staging while excluding all
active builders and pinned versions. Managed upload staging and operation
ledger retention still need a deployment cleanup policy.

ZIP packages contain one flat SHP/SHX/DBF set with optional PRJ/CPG. Names are
ASCII, normalized to lowercase; absolute paths, separators, traversal, duplicate
case aliases, symlinks/special files, encryption, nested archives and compression
other than Stored/Deflate are rejected. Central metadata and actual extraction
enforce input/member/expanded/file-count/ratio limits. No TAR or resumable upload.

DataVersion fingerprints source bytes, sidecars and interpretation/definition;
DerivedVersion hashes the complete derived manifest; RenderRevision includes
the pinned resource graph. The first raster request lazily schedules one
background overview operation. Base data serves immediately, then a completed
overview set switches atomically with unchanged DataVersion. Failures leave
the base live and are persisted; attempts are bounded to avoid retry storms.
Categorical overviews use nearest sample centers; continuous overviews retain
effective per-band weights through odd dimensions and alpha reduction.

Same-CRS raster views select a conservative level only for compatible sampling
and read its intersecting window plus kernel halo. Nonlinear cross-CRS requests
currently read the bounded full base level; large requests can fail admission
instead of silently clipping the curved footprint or reducing quality.
Vector requests restore complete geometries and currently prepare every feature
of each map item to preserve symbol ordering and full geometry measurements.
Large-scene admission can fail; viewport pruning remains an optimization audit.

## Queues, caches and identity

Each HTTP/animation/background class has reserved active capacity and a bounded
queue. Blocking workers retain permits until actual completion. Cancellation
checks are cooperative; a disconnected last singleflight waiter cancels render
work. The scheduler shares parent memory admission across child operations.

Moka caches accounted rendered area images with finite TTL; Foyer stores encoded
responses in memory/disk using the synchronous Psync engine and no recompression.
Keys include graph revision, normalized view/crop/output and actual backend.
Area render and requested-tile encode have independent singleflight maps.
Only requested tile crops are encoded. Integrity failures discard the response
entry; readiness exposes a failure counter. Source snapshots are never cached.
Decoded block/object caches beyond area images remain an optimization gap.

Successful PNG responses include strong encoded-byte ETag, `Cache-Control:
no-cache`, `X-Geello-Backend`, `X-Geello-Render-Revision`, and
`X-Geello-Incomplete`. Revalidation checks the current published graph before
cache lookup; unpublished resources cannot be revived through a warm cache.
CPU is synchronous; GPU initialization/render waits use the library async API.
`gpu-preferred` falls back only during initialization, never after render failure.

## Verification evidence

Platform: Windows x86_64 MSVC, Rust/Cargo 1.96.0. Ordinary fixtures are generated
offline in temporary directories. No owner-provided large files were changed.
All values below are experimental measurements, not capacity promises.

| Trace | Evidence |
| --- | --- |
| STO / DAT, T-16/T-23 | `derived_contracts`: FGB identity/typed properties/null/empty/Z/M/order, full geometry bbox/exact query, disk/corruption failure, raw float bits, masks, weighted 3×3 overview and alpha calibration |
| ADM-001–008, T-55–63 | `server_contracts`: uploads, valid Shapefile ZIP, traversal/absolute/separator/alias/link/nested/ratio rejection, external registration, stale ETag, failed replacement, reference/pin protection, detached content, managed deletion |
| STO / ADM / OPS, T-38/T-60–64 | Concurrent shard updates, indexed restart, orphan reporting, deletion journal rollback/roll-forward, interrupted operation reconciliation, missing-store repair |
| RAS / STO, T-16/T-39 | Actual TIFF registration → base render → lazy overview publication, unchanged DataVersion/new revision, old pinned store retained then reclaimed |
| REN / API / OPS, T-36–41/T-46–50 | PNG pixels/304, current-publication checks, one metatile render/two requested encodes under concurrent duplicate requests, scheduler abandonment; encoded envelope integrity/replacement unit test |
| G-09 / T-64 | Opt-in 1,000-resource test: sequential writes **18.678781 s**, index rebuild/compaction then indexed restart **192.3996 ms**; final repeat **18.3768335 s / 185.5841 ms**; temporary local disk, debug build |
| G-02 / T-22 | Opt-in real GPU server map test passed on the workstation; actual backend header `gpu` and decoded red point verified. Existing workstation device is NVIDIA RTX 4090/DX12; this service test does not inventory its driver or validate Linux Vulkan |

Reproduce:

```text
cargo fmt --check
cargo clippy --all-features --all-targets --locked -- -D warnings
cargo test --all-features --locked --lib --tests
cargo test --all-features --locked --test server_contracts thousand_resource -- --ignored --nocapture
cargo test --all-features --locked --test server_contracts gpu_server -- --ignored --nocapture
```

The ordinary suite passes **69 tests**; opt-in device/scale tests and owner large-data tests
remain ignored by default. Reduced feature builds verify backend-free library,
encoding alone, derived storage/encoding, CPU+GeoJSON server and GPU+TIFF server.
This is Windows evidence only. The actual server binary also started with the
experimental profile; HTTP `/health/ready` reported `ready`, backend `gpu`,
zero live accounted bytes and zero cache failures. The foreground PTY was then
interrupted; this is not evidence of a clean process shutdown or power-loss
durability.

## Remaining acceptance boundaries

- G-01: review all 382 resolved packages, 106 added package versions, 954 edges,
  licenses/notices and native build inputs. ZIP approval is recorded; the
  transitive closure is still a candidate.
- G-03/G-08: broaden malformed/truncated/encrypted ZIP and TIFF matrices,
  power-loss/filesystem fault injection, source/derived corruption recovery,
  per-allocation budget audit and native FGB temporary-file disk accounting.
  FGB upstream uses a system temporary file; the implementation admits a
  conservative transient shard envelope, but exact physical-volume accounting
  and externally imposed filesystem quota still require validation.
- G-09: repeated multi-thousand-resource contention/restart and failure runs,
  malformed orphan/staging/operation retention, damaged-authority recovery policy.
  The one-thousand experiment is a baseline, not gate closure.
- G-07: formal resource/performance limits and RSS/GPU accounting remain open;
  metadata, serializer/cache-internal allocations and transport buffers are
  not claimed to equal the operation memory estimate.
- M4: text/font shaping, animation, OGC/WMS/WMTS/WFS subsets, WebSocket,
  consistent extractor errors, OpenAPI and static API documentation.
- M5/G-10/G-11: Linux/musl and default GPU Alpine plus independent CPU Alpine,
  driver/device mapping, initialization fallback fault test, native PROJ and
  full platform/performance acceptance.

The functional M3 closed loop is implemented and exercised. The remaining
acceptance boundaries above prevent treating this as fully validated production
resource management or the completion of the entire first-stage rewrite.
