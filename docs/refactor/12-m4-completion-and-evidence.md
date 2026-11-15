# M4 implementation and functional exit evidence

Date: **2026-11-15**. Branch: `refactor`, based on `122f89d` plus the working
tree. Windows x86_64 MSVC, Rust/Cargo 1.96.0. This record supersedes the remaining
work lists in the earlier [animation](10-m4-animation-and-evidence.md) and
[HTTP](11-m4-http-contract-and-evidence.md) checkpoints.

The owner authorized completing all M4 work and resolving confirmations using
the recommended choices. The implemented boundary is the agreed bounded query
subset, with no GML/full-WFS or official OGC certification claim. Conformance
arrays remain empty. Production performance profiles, Linux/Vulkan/Alpine and
native PROJ remain M5; the numerical profile remains experimental.

## Exit matrix

| Trace | Implemented behavior | Reproducible evidence |
| --- | --- | --- |
| TXT-001/004, D-24/27, T-26/29, G-04 | Explicit immutable font bytes and face indices, matching exact weight/style/stretch stacks, no system scan or synthetic style, `und`/explicit language, advanced Unicode shaping/bidi/fallback | `tests/text_contracts.rs`; Arabic, Devanagari, CJK, ligature/combining clusters and isolated fallback; committed renamed OFL subsets and hashes |
| TXT-002/003, D-25/26, T-27/28 | Grapheme-safe word/glyph wrap, point/polygon/line candidates, marker obstacles with own-marker exemption, true polygon containment including holes, global priority/collision ordering and deterministic ties; curved cluster placement/upright reversal, short-line midpoint, 150px first/300px repeat, 30-degree neighboring and 90-degree total angular envelope | `tests/text_contracts.rs`; CPU pixel comparison for reversed curved labels; `explicit_fonts_render_through_publication_maps_and_buffered_metatiles` checks real font publication, map output, shared adjacent crops and invalid-font rollback |
| ANI-001–004, T-30–33 | Stateless arbitrary-time style/color/raster/geometry evaluation, typed stable IDs and source-plane transforms before projection, source preservation and refresh validation | `tests/animation_contracts.rs`, timed vector/raster/geometry server regressions, independent geometry example |
| DAT-005–007, API-003/004, T-08–10/42/43 | Exact intersection and whole-feature CRS84 output; declared typed equality aliases, exact bounded counts, OR IDs/AND predicates, stable source order; Features null geometry versus WFS null exclusion; bounded namespace-aware WFS KVP/XML and version-wide schema | `exact_feature_queries_typed_equalities_hits_and_namespace_aware_wfs_agree`; diagonal false-positive, 2^53+1 integer, bool/string, hits, FES escaped literals, IDs and heterogeneous XSD |
| API-003, OPS-003, T-42/49 | Immutable resource pin/query in authenticated opaque v2 cursors; absolute TTL, capacity without forced live eviction, independent maintenance, refresh/unpublish continuation, restart/expiry 410 | Two snapshot/capacity tests in `tests/http_contracts.rs`; changed query, token tampering, refresh/unpublish, capacity, expiry and projected source normalization |
| API-001/002/007, T-40/41/46 | Published Maps/Tiles/Features discovery, WMS request-order multi-map composition with one global label pass, CRS84/EPSG:4326 axes, final PNG/JPEG/background; WMTS fixed matrix dimensions/limits; encoded-byte ETags and legacy XML errors | `tests/http_contracts.rs`; PNG pixels, reversed layering, unequal lon/lat bounds, JPEG background tolerance, matrix metadata and publication checks |
| API-005/006, T-44/45 | One-map WS hello/init/atomic groups/ordered ack, distinct state/resource revisions, one pending render/latest-state cancellation, monotonic clock, pause/seek/retry, refresh redraw, unpublish event/close, single metadata/image writer, bounded send/init timeouts and PNG/JPEG | WebSocket unit/session tests and real loopback TCP upgrade; burst updates deliver only latest unstarted frame, paused refresh retains time, JPEG binary length, failed retry remains suspended, stuck sink releases pins |
| DOC-001, T-47 | Generated OpenAPI and offline HTML, complete working public subset and migration boundary; independent clients outside server hosting | 47 documented paths/50 operations, live-router/schema contract suite, `tools/verify_m4_demo.py` and `examples/m4-protocols` |

The experimental example now uses aligned **4x4 tiles with 128px buffer**;
padding is clipped at finite grid boundaries. Smaller explicitly configured
profiles remain supported and are used by compact service tests. Labels are
laid out in the full buffered metatile before crop/encoding. There is no exact
cross-metatile consistency guarantee or general font/typographic coverage claim.

## Frozen public contract choices

Typed application/OGC query names are case-sensitive; legacy KVP names are
case-insensitive. Unknown fields and duplicates fail, including legacy names
that differ only by case. `geello_time` is nonnegative finite seconds; WMS
`TIME` is an alias and the two cannot accompany each other. WMS maps are registry
IDs in request order; only empty styles are accepted. WMS CRS:84 is XY,
EPSG:4326 BBOX is latitude/longitude and is normalized to CRS84 before rendering.
WMTS publishes PNG/default style and the two fixed grids, with map-specific
zoom limits. JPEG quality is 90 for WMS; flattening uses BGCOLOR once.

Feature IDs use `n` plus a decimal integer or `s` plus UTF-8 hexadecimal. Numeric
42 and string `"42"` are distinct (`n42`, `s3432`). Missing and null properties
are distinct; equality performs no string/number coercion. Queryables select
top-level scalar source properties and have unique nonreserved aliases. A raw
source name is retained for reader-wide type validation; a single-token pointer
is accepted as a configuration spelling. Invalid type replacements fail before
publication. Feature query output does not apply animation.

Valid RFC3339 datetime instants/intervals match timeless records. Bbox intersects
complete geometry; Features includes null geometry, excludes explicit empty
geometry for bbox; WFS excludes both for bbox. Counts scan the bounded pinned
dataset and fail on work/deadline/memory exhaustion, never return guessed hits.
Pagination pins the complete query, source version and absolute creation expiry;
access never renews expiry. New queries require publication; existing cursors
can continue after unpublish until expiry. Restart intentionally loses cursors.

WFS XML uses WFS 2.0, FES 2.0 and GML 3.2 namespaces. One Query supports equality
and AND, ResourceId OR, or CRS84 BBOX. Maximum XML depth is 16 and node count is
1024 within the body ceiling. No DTD, external entity/IO, arbitrary expression,
transactions or GML response is accepted. DescribeFeatureType scans the entire
pinned version; missing fields become optional, nulls nillable, integers use
`xs:integer`, mixed/structured values use `xs:anyType`. XML names are `p` plus
UTF-8 hex with original names documented. This XSD explicitly does not validate
JSON and is not a GML application schema.

WS protocol version is 1; variant input is `{"id":"default"}` and normalized
ack state stores `"default"`. Init requires view and variant; omitted playback
defaults to paused time 0/speed 1, omitted output to PNG/30fps. Configured maximum
is advertised and over-limit controls reject atomically. JPEG requires explicit
quality 1–100. Request IDs are visible ASCII, 1–128 bytes; controls are at most
64KiB. Invalid controls preserve state. Retry keeps the control revision. Every
accepted control is acknowledged in receive order; superseded unstarted frames
need not be sent. Metadata uses `frame_id`, `revision`, `time`,
`render_revision`, `content_type`, `bytes`, dimensions/backend/incomplete flag.
Exactly the next binary message supplies the image. A started pair completes
before another control/event, or transport is dropped. Resource refresh preserves
time; logical playing time advances during suspension. Native block cache remains
shared, while intermediate frames bypass HTTP rendered/response caches.

| Boundary | Error/close behavior |
| --- | --- |
| OGC/application HTTP | Sanitized RFC9457 problem+json; invalid input 400, missing/unpublished 404, cursor 410, validation 422, storage 500, admission/preparing/backend/deadline 503 |
| Legacy adapter | Same HTTP status with WMS ServiceExceptionReport or OWS ExceptionReport XML; InvalidParameterValue, OperationNotSupported or NoApplicableCode. Outer routing/admission errors may use problem+json; 405 preserves Allow |
| WS invalid JSON/control/binary | JSON error code, no state mutation; connection remains available |
| WS render failure | `render-failed` error, suspend automatic output until valid update/retry succeeds |
| WS refresh/unpublish | `resource-changed` redraw; `resource-unpublished` then empty close after current pair |
| WS init/send deadline or broken transport | Drop connection; client discards incomplete pair and reconnects with complete init; no resumable session |
| Oversized RFC6455 transport message | Axum/tungstenite enforces 64KiB frame/message bounds; transport closes/errors; no application retry on a partially sent pair |

G-06 is closed for this frozen application subset and its tested schemas,
with `conformsTo: []`; the absence of certification is an explicit boundary.
G-04 is closed for the implemented text path and committed fixture repertoire.

## Dependencies and assets

The [M4 package record](m4-dependency-closure.json) binds the current lockfile,
421 third-party package versions/sources/checksums/licenses/features and 45
package/feature deltas from the accepted M3 record. It records the owner's
current authorization, without altering the historical M3 approval or approving
future versions. Direct additions use already selected cosmic-text 0.19.0 and
quick-xml 0.42.0; Axum enables its selected ws feature. No system font scan,
native PROJ, HTTP source fetching, authentication stack or frontend is added.

The [font manifest](../../tests/fixtures/fonts/manifest.json) records exact source
archives/static faces, source/fixture/license SHA256, versions, face index,
coverage and renamed family. Noto Sans 2.015, Naskh Arabic 2.021, Devanagari 2.007
and CJK SC 2.004 retain OFL notices. `tools/generate_test_fonts.py` uses approved
FontTools 4.68.0 only for maintenance; ordinary tests require no Python/network.
The demo verifier checks fixture and license hashes before starting.

## Verification

Recorded on Windows/MSVC with NVIDIA GeForce RTX 4090, DX12 driver
`32.0.16.2114`. The real GPU comparison reports general scene mean channel delta
0.005859375 (max 1), and multilingual text outline mean delta **0**.

```text
cargo test --all-features --locked
cargo clippy --all-features --all-targets --locked -- -D warnings
cargo fmt -- --check
cargo check --no-default-features --lib --locked
cargo check --no-default-features --features text,config-toml --lib --locked
cargo check --no-default-features --features server,read-geojson,render-cpu --locked
cargo check --no-default-features --features server,read-geojson,render-gpu --locked
cargo test --all-features --locked --test gpu_contracts -- --ignored --nocapture
python tools/generate_http_docs.py --check
cargo build --all-features --locked
python tools/verify_m4_demo.py
git diff --check
```

The independent demo starts an isolated temporary CPU server, uploads exact
committed resource files, checks Unicode PNG/WMS JPEG/axis order/WMTS, follows
a real snapshot link, queries WFS hits and XML, and performs a real RFC6455
handshake and metadata/image capture. Temporary data and process are removed.

Final result: **146 ordinary tests passed, 0 failed, 7 explicitly ignored**.
All-feature/all-target Clippy with warnings denied, formatting, the four feature
matrix commands above, generated HTTP freshness, dependency lock hash, demo
font/license hashes, the real DX12 GPU comparison and the independent demo pass.
Documentation checks cover 78 local links, Markdown fences and demonstration
TOML/GeoJSON/XML; `git diff --check` passes. The metadata inventory is 47 paths
and 50 operations. The ordinary suite's seven
ignored tests comprise three GPU tests, two owner-asset tests and two catalog
scale experiments. All three GPU tests were additionally run explicitly and
passed: multilingual CPU/GPU parity, destroyed-device failure without CPU
switching, and real GPU HTTP map execution. Ignored owner-data/scale tests are separate from M4 functional exit;
Linux/Vulkan, native projection, Alpine and measured production targets remain
M5 obligations. These are not silently counted as passes.

**M4 functional exit: Implemented and verified on this Windows workstation.**
The agreed implementation scope has no remaining M4 task; certification and M5
platform/performance work remain separately scoped. No Git commit, release or
deployment is performed as part of this completion.
