# M3 basic service implementation and evidence

Implementation date: **2026-11-13**, branch `refactor`, working tree based on M2
commit `4ffbd96`. This describes actual interfaces alongside the accepted target
design. It does not declare OGC conformance, production capacity or container
validation. On 2026-11-15 the owner approved all existing dependencies;
G-01 is Accepted for the current M1-M3 closure in the
[full inventory](08-m3-dependency-closure.md). New platform/container inputs and
future package/feature additions are outside that approval.

The subsequent [M4 animation checkpoint](10-m4-animation-and-evidence.md)
extends these basic image/tile routes with `geello_time` and validated style/
raster animation bindings; its current cache salt supersedes the M3 salt below.

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
| Server | `src/server/activity.rs`, `src/server/memory.rs` | Shutdown lifetime tracking and conservative aggregate metadata admission |
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
| `GET /api` | OpenAPI 3.0.3 for the implemented HTTP subset, configured external base URL and binary/profile capabilities |
| `GET /api.html` | Offline static API documentation generated from the same contract |
| `GET /maps/{id}/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=256&height=256` | Basic XY bbox image, PNG, stretch aspect policy, published output CRS only |
| `GET /tiles/{id}/{z}/{x}/{y}` | Basic WebMercatorQuad PNG, 256 pixels, buffered metatile; z in 0..30 |

All writes require exactly one strong `If-Match: "resource-version"`, or
`If-None-Match: *` for creation. Missing conditions return `428`; stale versions
and reference/ownership conflicts return `409`. Conflict bodies include bounded
reference keys when available. Validation failures use `400`/`422`; unsupported
media `415`; capacity and preparing errors `503`; oversized uploads `413`.
Application, extractor and routing errors now use sanitized RFC 9457 JSON
problems (`application/problem+json`); 405 responses retain `Allow`. The M3
route contract is served at `/api` and `/api.html`. Subsequent M4 work adds
separately tested OGC Maps/Tiles/Features, WMS/WMTS/WFS subsets and a bounded
WebSocket map route; their current boundaries and non-certifying status are
recorded in the [M4 HTTP checkpoint](11-m4-http-contract-and-evidence.md).

Upload media: GeoJSON `application/geo+json`, GeoTIFF `image/tiff`, Shapefile
`application/zip`, definition `application/toml`, fonts `font/ttf`, `font/otf` or
`font/collection`. Optional headers `X-Geello-CRS`, `X-Geello-Encoding` and
`X-Geello-Sample-Kind` provide interpretation. Raster sample kind must be explicitly
`continuous` or `categorical`. Dataset TOML uses the M2 schema; a managed dataset
definition must refer to a source already registered in the catalog.

Example using curl (use `curl.exe` on Windows PowerShell):

```sh
curl --request PUT 'http://127.0.0.1:3000/admin/v1/resources/dataset/demo:points/content' \
  --header 'If-None-Match: *' --header 'Content-Type: application/geo+json' \
  --data-binary '@points.geojson'
curl --request PUT 'http://127.0.0.1:3000/admin/v1/resources/dataset/demo:roads/content' \
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
active builders and pinned versions. Startup reclaims abandoned private
`staging/upload-*` directories after journal/operation recovery under the writer
lock; unknown staging entries and registered source paths remain untouched.
`Catalog::reconcile_derived` reports bounded unregistered derived entries,
including missing/malformed manifests. Such entries are retained for diagnosis
and do not by themselves fail unrelated preparation.

`ServerState::prune_operations(before, max_remove)` provides explicit,
bounded terminal-ledger retention. The caller supplies an absolute filesystem
modification-time cutoff and a batch cap; there is no implicit production TTL.
Active operations and repair diagnostics referenced by retry suppression remain
recorded. Successful removal returns later operation lookups as `404`, including
after restart; it never removes resource records or original files. Automatic
maintenance scheduling and deployment retention numbers remain unconfigured.

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
retain base resolution but read a planned native window. Planning uses every
nearest/bilinear center or the same adaptive inverse footprints as area
resampling, including curved interiors. Large footprints still fail admission
explicitly; no corner-only clipping or silent quality reduction occurs.
Vector requests restore complete geometries and currently prepare every feature
of each map item to preserve symbol ordering and full geometry measurements.
Large-scene admission can fail; viewport pruning remains an optimization audit.

## Queues, caches and identity

Each HTTP/animation/background class has reserved active capacity and a bounded
queue. Blocking workers retain permits until actual completion. Cancellation
checks are cooperative; a disconnected last singleflight waiter cancels render
work. The scheduler shares parent memory admission across child operations.

Persistent catalog records, generation metadata, resource pins and operation
ledger entries share that same memory root. Startup reserves parser workspace
before reading each indexed member; COW mutation reserves before graph copies
and retains the replacement reservation before pointer publication. Failure
leaves the old complete generation live. Metadata accounting uses a conservative
envelope of 32 times its JSON byte count plus 1 KiB, with additional COW/parser
workspace. This deliberately estimates allocations; it is not an RSS limit or
a production sizing recommendation. Ledger pruning and pin release return their
reservations. Shutdown cancels a child work context while leaving the persistent
memory root available for terminal operation writes.

Administrative record/list/operation JSON uses bounded counting serialization;
record/list clones have admission before allocation. Encoded HTTP images reserve
the outgoing copy. Body streams retain output admission until consumption or
drop. Axum/Hyper socket buffers, allocator overhead and cache internals remain
separate allocation-audit obligations; an accounted application body does not
assert control over the transport's copies.

`begin_shutdown()` closes handler/task/upload admission and cancels scheduled
work. `shutdown().await` waits for handlers, preparation/repair/overview tasks,
singleflight orchestration and actual blocking closures, including work whose
async waiter was aborted. It then releases object-cache values and closes Foyer.
Ctrl-C starts that sequence before Axum waits for graceful request completion,
so an unfinished upload body cannot prevent cancellation indefinitely. Blocking
work remains cooperative: a non-cooperative closure can delay shutdown. Callers
must still stop all writers before an offline checkpoint; shutdown does not
revoke separately held public catalog handles or make a live backup consistent.

Moka caches accounted rendered area images and immutable decoded native blocks
under one shared weighted capacity and finite TTL; Foyer stores encoded
responses in memory/disk using the synchronous Psync engine and no recompression.
The encoded cache uses a bounded `GLRC0002` frame: fixed prefix/header length,
BLAKE3 of header plus payload, small JSON metadata and raw PNG bytes. It avoids
JSON byte-array expansion and checks framing/checksum before allocating decoded
payloads. Configured Foyer memory payload capacity is reserved at startup;
in-flight reads and caller-held encoded results have separate admission. Cache
engine indexes, allocator overhead and OS I/O buffers remain external estimates.
Keys include graph revision, normalized view/crop/output and actual backend.
Area render and requested-tile encode have independent singleflight maps.
Only requested tile crops are encoded. Integrity failures discard the response
entry; readiness exposes a failure counter. Source snapshots are never cached.
Native-block keys contain the DerivedVersion, encoded member hash and byte
length; these backend-independent numeric objects do not require an actual
render backend in their key. Checksum/decode runs only on a miss. Concurrent
misses coalesce; a caller affected by another operation's cancellation/deadline/
budget failure retries with its own context. Failed loads are not retained.
Native samples, masks, alpha and metadata contribute to the conservative object
weight; oversized entries bypass retention. Values retain memory admission for
their entire cached lifetime. A different-view raster HTTP test proves a new
render without another native decode.

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

The initial ordinary suite passed **69 tests**; opt-in device/scale tests and owner large-data tests
remain ignored by default. Reduced feature builds verify backend-free library,
encoding alone, derived storage/encoding, CPU+GeoJSON server and GPU+TIFF server.
This is Windows evidence only. The actual server binary also started with the
experimental profile; HTTP `/health/ready` reported `ready`, backend `gpu`,
zero live accounted bytes and zero cache failures. The foreground PTY was then
interrupted; this is not evidence of a clean process shutdown or power-loss
durability.

## M3 exit review — 2026-11-15

Review baseline: commit `48c81f5`. At the initial review, M3 was not exit-ready:
the interactive GPU preview did not close storage/admission and recovery
obligations. The dated follow-ups below record fixes and evidence; the final
exit assessment supersedes that initial checkpoint.

Three regressions were reproduced against the baseline before fixing them:
rebuilding a damaged content-addressed shard still left restart broken; an
unregistered derived directory without a manifest blocked unrelated uploads;
and dropping an incomplete upload left its operation in `staging`.

| Requirement / scenario | Review change and evidence |
| --- | --- |
| ADM-008, T-63 | Verify reused index-member size/hash; atomically restore authoritative bytes during rebuild/publication before switching the pointer. Regression corrupts an existing shard, rebuilds, and successfully reopens it. |
| STO-003, ADM-008, T-38/T-63 | Windows read-only pointer fault rejects replacement after COW writes; both loaded state and restart retain the old complete generation, and retry succeeds after the fault is removed. This is filesystem-failure evidence, not a physical power-loss experiment. |
| STO-003, ADM-008, T-49/T-63 | Retain unidentifiable derived artifacts, expose bounded diagnostics, and allow unrelated preparation; test covers absent and malformed manifests. |
| ADM-007, T-62 | An upload ownership guard records failure on handler cancellation and other exits before preparation ownership transfers. Ledger-write failures still require restart reconciliation; no durability claim beyond successful filesystem writes. |
| ADM-007, OPS-003, T-49/T-62 | Explicit terminal retention preserves active work, honors the supplied cutoff/cap, releases ledger capacity, and keeps removal effective after restart. Startup reclaims known abandoned upload staging while preserving unknown content. |
| ADM-007, T-62/T-63 | Restart validates operation IDs and filename agreement before recovery writes; a damaged path-bearing identity fails startup without an out-of-ledger write. |
| ADM-002, T-56/T-57 | Restricted extraction additionally rejects truncated, encryption-flagged and CRC-damaged archives in ordinary offline tests. |
| ADM-008, OPS-008, T-64 | Four concurrent writer threads publish and look up 3,000 minimal resource records, restart from the indexed generation with incomplete COW artifacts ignored, report an unregistered managed directory, rebuild/compact, then restart and query again. |

The 3,000-resource experiment passed on this Windows MSVC workstation in a
debug build: writes plus lookups **87.5853035 s**, indexed restart
**313.7632 ms**, full experiment **91.44 s**. This measures a serialized catalog
writer under concurrent callers, not parallel disk commits, large source
payloads, production capacity, or Linux/container recovery.

Reproduce the additional scale experiment:

```text
cargo test --all-features --locked --test server_contracts multi_thousand_resource_catalog_contention_and_restart -- --ignored --nocapture
```

No direct dependency, Cargo feature or lockfile changes were required. The
post-review ordinary suite passes **76 tests**, with **6 opt-in tests ignored**
by default (device, owner-data and scale experiments).
`cargo clippy --all-features --all-targets --locked -- -D warnings` also passes.
The 3,000-resource experiment above was run explicitly in addition to the
ordinary suite; ignored does not mean failed or automatically validated.

### Storage and approval follow-up — 2026-11-15

G-01 for the existing M1-M3 closure is **Accepted**, following the owner's
explicit approval. The reproducible inventory still contains 382 packages;
promoting the existing FlatBuffers 24.12.23 package to an optional direct
dependency adds one workspace edge (955 total), with no new third-party package,
version or resolved feature. The generator checks the versioned approval record
and labels later third-party changes as candidates rather than approving them.

| Requirement / scenario | Additional implementation and evidence |
| --- | --- |
| STO-001, OPS-003 / T-36–T-38 | Encode full XY FGB features, Hilbert order and packed R-tree in admitted memory. No upstream writer or system temporary spool is used. Disk writes use exact byte admission under the derived staging root. An exact-total-byte quota succeeds; one byte less fails and releases staging/accounting. |
| DAT-003, STO-001 / T-08/T-36 | Companion XY coordinates, raster affine coefficients and display ranges use IEEE bit integers. Schema 2 prevents decimal parser drift; nested FGB geometry tests independently decode complete coordinates and original ordinals, including holes, empty multi-line components and nested collections. |
| OPS-003 / T-49 | JSON records/manifests use a counting pass with cancellation/byte limits before output allocation. Metadata opens have a retained conservative reservation; vector cursors share immutable manifest metadata rather than copying it. Build-time shard/tile metadata also retains admission. This is explicit allocation accounting, not an RSS claim or a completed server-wide audit. |
| RAS-003, PRJ-001 / T-16/T-23/T-49 | Library `sampling_window` and `DerivedRaster::resample_view` plan/load the required native window before shared resampling. A curved, sheared-grid test compares all three methods with full-source results, including masks/alpha, while a 512 KiB operation rejects full-source loading but accepts the small view. Cancellation releases admission. |
| STO-003 / T-38 | Reused derived directories are checked against staged member bytes before preparation can succeed. Corrupt content is rejected; immutable published bytes are not silently overwritten. Offline quarantine/rebuild is the recovery policy. |
| ADM-008, OPS-008 / T-63 | Corrupt authoritative pointer fails closed. A checksummed offline checkpoint is restored to the same absolute root, preserving the damaged tree, resource ETag, DataVersion and managed content; the resource is then available through the restarted administration service. This test starts with a stopped catalog and does not assert live backup or clean draining of all server background workers. |

The follow-up ordinary suite passes **81 tests**, with **6 opt-in tests ignored**.
Local validation covers **43 links/anchors** and **16 TOML/JSON examples**.
All-feature Clippy, reduced-feature checks, documentation links/embedded formats
and the opt-in real GPU server map test also pass on Windows MSVC. No container
or physical power-loss evidence is inferred.

### Offline checkpoint and damaged-authority recovery

1. Stop the server process and its writers completely, including background
   preparation and maintenance. Await `shutdown()` to drain server-owned work,
   then release server/catalog handles and stop any separately managed writers.
   Keep the process stopped for the entire backup/restore procedure.
2. Copy the complete data directory to a separate backup, retaining managed
   originals, detached content, registry pointer/shards/generations/deletion
   journals, operation ledger and staging. Save byte counts and BLAKE3 checksums
   for every copied file; verify the copy before treating it as a checkpoint.
   Back up external original files separately with their registered absolute
   paths and a matching content snapshot; registration never owns those files.
3. Record the exact binary/schema, data root, pointer bytes and catalog generation
   with the checkpoint. A coherent checkpoint is the authority; never guess the
   highest generation, register orphan directories, or mix a pointer with shards
   or deletion journals from a different point in time.
4. When authority is damaged, retain the failed data tree for diagnosis. Restore
   the verified complete checkpoint into an empty directory at the **same
   absolute data root**, because source/managed/derived paths are currently
   absolute. Ensure external sources match the checkpoint. This is an explicit
   rollback to checkpoint state, not recovery of acknowledged later changes.
5. Start one server writer. Startup validates pointer/member checksums, graph
   and recovery records. Compare generation, resource counts, ETags and source
   fingerprints against the checkpoint, then exercise administration and map
   requests. Failure keeps service unavailable; do not silently start an empty
   catalog. Without a verified checkpoint, reconstruction requires an explicit
   owner-approved re-registration plan, not automatic directory discovery.

Derived content is disposable. Schema-1 experimental derived directories are
incompatible with schema 2. With the process stopped, quarantine affected
**derived version directories only**, preserving original sources and registry
records. Missing-manifest repair then builds schema 2 under the same DataVersion
and publishes a new DerivedVersion. For damaged members use the same explicit
quarantine procedure; a changed original rejects repair and requires conditional
administrative refresh. Do not edit checksums to make corrupt bytes appear valid.

### Aggregate admission, shutdown and process recovery follow-up

| Requirement / scenario | Implementation and evidence |
| --- | --- |
| OPS-003, ADM-008 / T-49/T-63 | Shared catalog/ledger reservations include retained metadata, parser workspace, COW and snapshot pins. An aggregate catalog failure preserves pointer bytes/generation, reopens successfully, and a smaller restart profile fails closed. |
| ADM-007, OPS-003 / T-49/T-62 | Multiple individually valid operation files exhaust the aggregate restart budget without modifying ledger bytes. Raising the explicit profile permits restart. Terminal retention releases reservations; final state drop returns shared accounting to zero. |
| OPS-003 / T-49 | Administrative JSON and image output retain admission in the body stream. Tests hold, consume and drop responses and observe reservation release. |
| OPS-003, ADM-007 / T-49/T-62/T-63 | Shutdown rejects new requests, cancels an unfinished upload and queued preparation, writes failed terminal status, and allows immediate reopen. An aborted async waiter cannot let shutdown finish before its actual blocking closure exits. Repeated shutdown is idempotent. |
| STO-003, ADM-007 / T-38/T-62/T-63 | A child test process publishes a managed dataset, starts an incomplete upload, and is killed by its parent without shutdown/Drop. Restart reacquires the writer lock, preserves the published version/content, marks the interrupted operation failed and reclaims private staging. This is an actual process-kill experiment on Windows, not power-loss evidence. |

Reproduce the process-kill experiment:

```text
cargo test --all-features --locked --test server_contracts process_kill_recovers_published_authority_and_interrupted_upload -- --nocapture
```

At that checkpoint the ordinary all-feature suite passed **89 tests**, with **6 opt-in tests
ignored**. The process-kill test is ordinary coverage and runs a real child
process; the six ignored cases remain separate GPU, owner-data and scale
experiments. Reduced base-library, derived/GeoJSON and CPU-server feature checks
also pass. Local documentation checks cover **43 links/anchors** and
**16 TOML/JSON examples**.

The first aggregate-accounting implementation passed the same 3,000-resource /
four-writer experiment with writes/lookups **264.8020239 s**, indexed restart
**459.6038 ms**, full run **269.19 s**. This exposed avoidable repeated JSON
counting of every catalog record on each mutation. Retained record estimates
now reuse the total and recompute only the affected shard, including same-version
records; the replacement reservation is still admitted before pointer commit.

The optimized run passed with writes/lookups **164.9101148 s**, indexed restart
**518.9104 ms**, full run **169.44 s** on the same Windows MSVC debug profile.
It improves the first accounting implementation but remains slower than the
earlier pre-accounting run; no production throughput target is claimed. The
3,000 definitions are minimal records, not large configuration bodies or source
payloads. Re-run release/platform benchmarks before confirming G-07 limits.

### Allocation and corruption follow-up — 2026-11-15

The remaining audit added admission to depth-first managed/derived directory
accounting, including cancellation and work checkpoints; retry-suppression
identities and retained resource snapshots now hold shared reservations rather
than unbounded strings. Graph-version metadata is admitted before collection,
map/style definitions remain admitted through asynchronous preparation, and a
flight is published only after its fallible definition admission succeeds.
Encoded response payloads retain their reservation while a caller's `Arc` is
live; cached encoded frames use the separate reserved Foyer capacity. Accounted
file reads reject growth and release on budget/cancellation
failure. These checks bound application allocations; they do not claim exact RSS
or filesystem metadata accounting.

Additional ordinary tests cover directory traversal cancellation/budget release,
metadata file admission, corrupt derived member quarantine without silent
replacement, changed external source refusal until an explicit conditional
refresh, and malformed/truncated/invalid-offset GeoTIFF replacement rollback.
The full all-feature run passed **95 ordinary tests**. A subsequent warm-cache
retention regression also passes, bringing the verified ordinary suite to
**96 tests** (7 library unit tests and 35 server-contract tests included).
It verifies that a caller-held encoded result retains admission, a cloned `Arc`
shares it, and final drop releases it without another render/encode. The real
GPU test passes on the local device when selected and remains an opt-in platform
check. All-feature/all-target Clippy with warnings denied, base-library,
derived/GeoJSON and CPU-server checks pass. Documentation validation covers
**44 local links/anchors** and **16 TOML/JSON examples**.

HTTP middleware now admits at most `active_http + queued_per_class` handlers,
including suspended registration bodies; excess requests receive `503
request-capacity`. A disconnect returns the permit and registration bodies have
the explicit operation timeout. A slow-body regression checks saturation and
release. This is application admission after Axum accepts the request; proxy
connection/header limits and kernel/Hyper transport buffers remain deployment
responsibilities. This additional regression brings ordinary coverage to
**97 tests**, including **36** server-contract tests.

### Startup, retry retention and cache internals follow-up

Server TOML loading has a 1 MiB bootstrap ceiling before runtime profiles are
available. One opened file supplies both the measured length and bounded read;
growth/shrinkage is rejected. This is an experimental parser input ceiling,
not an owner-approved production resource profile. Normal resource bodies still
use the explicit runtime profile.

Repair/overview retry identities include resource kind, ID and publication
version. Stale entries are pruned when requesting work or pruning operation
history; current failed repairs retain their diagnostics and suppress retries.
An explicit successful refresh makes its previous failed repair ledger eligible
for explicit retention cleanup. Map compatibility checks now use accounted
resource copies and managed reconciliation admits its metadata workspace.

Foyer's two alternating flush buffers are page-aligned from
`response_memory_bytes`, its submit threshold uses the same capacity and it uses
one flusher. Startup reserves memory retention, both flush buffers, the submit
threshold and concurrent producer overshoot using `active_http + queued_per_class`
maximum framed responses. This deliberately increases reported startup admission;
an insufficient profile fails rather than ignoring these cache costs. Internal
indexes, allocator overhead, recovery scratch and OS I/O still need measurement.

An all-render-waiter disconnect regression holds all HTTP work permits, abandons
the queued render, observes pin/memory release, and verifies that a new request
renders successfully without stale singleflight state. Together with the startup
ceiling test, ordinary coverage is **99 tests**, including **8** library unit tests
and **37** server-contract tests. The changed-source test also verifies that
refresh releases the old failed-repair operation from retention protection.

The catalog crash test kills six separate child processes at the shard write,
generation write, pointer switch, deletion journal, content-to-trash rename and
deletion commit boundaries. Restart selects old/new complete authority according
to the pointer, rolls back or completes deletion, and remains stable on a second
restart. Test hooks are compiled only for unit tests; production has no
environment-variable fault switch. This extends process-crash evidence without
claiming physical power-loss durability or Linux directory-sync validation.
The real GPU server image test, all-feature/all-target Clippy with warnings
denied, formatting and reduced-feature checks also pass after these changes.

### Final M3 exit assessment — 2026-11-15

**Implemented; Windows functional exit conditions satisfied.** This is the M3
milestone exit, not complete first-stage or cross-platform gate acceptance.
The assessment uses the explicit M3 exits in the roadmap rather than a test
count or an indefinite allocation audit:

| M3 exit obligation | Evidence |
| --- | --- |
| Registered source to HTTP image/tile under bounded concurrency | Managed/external fixtures, actual CPU/GPU map rendering, metatile reuse, requested-only encoding, request/worker admission and all-waiter cancellation/retry |
| Publication, refresh, live pins and complete cache identity | Conditional conflicts, dependent/pinned deletion refusal, failed refresh rollback, changed-source refusal, versioned overview publication and backend-separated keys |
| Persistent cache correctness and diagnostics | `disk_response_cache_survives_restart_and_revalidates_current_publication` proves restart hits without rendering/encoding, correct 304 and unpublish rejection; checksum corruption is tested after disk reopen |
| Incomplete result preservation | `incomplete_scene_summary_survives_area_reuse_and_disk_restart` verifies incompatible geometry diagnostics across area reuse, tile encoding and persisted responses; cache identity advanced to `geello-basic-render-v2` |
| Resource lifecycle and archive safety | Single-file and valid Shapefile ZIP upload, external registration, record-only retention, managed deletion and external refusal; traversal, alias, link, nested, ratio, truncated, encrypted and CRC-failure fixtures |
| Indexed authority and recoverability | Four-writer 3,000-resource experiment, damaged-shard rebuild, offline checkpoint, pointer replacement failure, operation/deletion journals and six actual child-process commit/deletion kills with repeated restart |
| Application allocation and worker lifetime | Shared catalog/ledger admission, bounded file reads/directory traversal/JSON output, retained response accounting, explicit Foyer flush/submit capacities, coherent catalog page generation and owned asynchronous driver draining on caller abandonment |
| Initialization-only fallback | Injected strict/preferred GPU startup failures verify explicit fallback and lock release; CPU startup skips GPU initialization. A real destroyed GPU device returns backend errors on repeated execution and never changes to CPU |

The last device-fault experiment exposed an uncaptured wgpu validation error.
GPU execution now dispatches pending device loss before target allocation,
captures submit/map validation errors, starts its resource-owning completion
worker before awaiting, and drains submitted work before returning an error.
The owned server driver keeps scheduler capacity and shutdown tracking until
that asynchronous work actually finishes. Semaphore waiters preserve their
position while polling cancellation.

Before M4 additions, `cargo test --all-features --locked` passed **104 ordinary
tests / 7 opt-in ignored tests**. Explicit real-device tests passed for library
parity, server map rendering and destroyed-device failure on NVIDIA GeForce
RTX 4090 / DX12 / driver `32.0.16.2114`; parity max channel delta **1**, mean
**0.005859375**. The destroyed-device experiment tests a fault between executions,
not every possible physical device failure timing. All-feature/all-target Clippy
with warnings denied, CPU-only server, GPU-only server, backend-free library and
formatting checks pass. Historical counts above identify earlier checkpoints.

## Remaining acceptance boundaries

- G-03/G-08/G-09: Windows functional evidence above is present. Linux filesystem
  and physical power-loss durability remain unvalidated; retain current authority
  on OS write errors and preserve diagnostic artifacts. Archive/reader support
  remains the documented validated layout/codec subset.
- G-07: formal capacity, retention profiles, performance and RSS/GPU residency
  require measurement and owner confirmation. The completed application
  admission audit establishes explicit bounds for owned work, not exact RSS;
  cache engine indexes/recovery, allocator, kernel and transport buffers remain
  measured deployment costs. Proxy connection/header limits remain required.
- M4: text/font shaping, geometry animation integration, OGC/WMS/WMTS/WFS
  subsets, WebSocket, consistent extractor errors, OpenAPI and static API docs.
- M5/G-05/G-10/G-11: native PROJ, Linux/musl, real Vulkan and default GPU Alpine
  plus independent CPU Alpine; container-specific device mapping, fallback,
  volume recovery and separate performance evidence are still required.

M4 can now build on the M3 resource/scheduler contracts. No dependency version,
platform guarantee or numerical production profile was changed by this exit
assessment. Regressions in these M3 contracts remain M3 defects to fix.
