# M4 HTTP contract and documentation checkpoint

Historical checkpoint. Its remaining-work statements describe that checkpoint,
not current status. The [final M4 record](12-m4-completion-and-evidence.md)
supersedes them with implemented behavior and current verification.


Date: **2026-11-15**, branch `refactor`, Windows x86_64 MSVC.
Trace: **API-007, DOC-001, D-39/D-45/D-51, T-46/T-50/T-53** for the
implemented HTTP and tested OGC Maps/Tiles subset. This increment does not
close G-06 or claim full OGC/WMS/WMTS/WFS/WebSocket conformance. M4 remains in
progress.

## Implemented contract

| Request | Result |
| --- | --- |
| `GET /api` | OpenAPI 3.0.3 JSON describing implemented resource administration, readiness and basic image/tile operations |
| `GET /api.html` | Standalone English static HTML for the same operations, with parameters, request media and response statuses |
| `HEAD` on implemented GET routes | Same routing and headers, no body |
| Invalid typed query/path | `400 application/problem+json` with a fixed sanitized title |
| Unknown route | `404 application/problem+json` without reflecting the requested path |
| Unsupported method on an existing route | `405 application/problem+json` with the route's `Allow` header |

The served JSON contains 47 paths and 50 explicit operations, including experimental OGC Maps/Tiles/Features discovery and rendering plus strict WMS/WMTS KVP subsets. The resource
unpublish action is documented as
`POST /admin/v1/resources/{kind}/{id}:unpublish`; Axum dispatches it through
the existing resource POST handler, which checks that suffix.

`servers[0].url` is the configured `public_base_url`, with its trailing slash
removed. It preserves a configured reverse proxy prefix and ignores `Host`,
`Forwarded` and `X-Forwarded-*` headers. The `x-geello-enabled` extension reports
compiled reader/render features, the initialized backend, maximum dimension,
maximum upload bytes and maximum list items. These are experimental profile
limits, not capacity promises. Neither the document nor HTML exposes configured
external roots, data-directory paths or catalog contents.

The JSON schemas describe actual resource records (including optional
`effective_crs`), operation states, source registration, catalog pages,
readiness and RFC 9457 problems. Upload media are listed with their resource
kind and feature restrictions. A `202` response means accepted preparation:
clients must poll its `operation_id` until `ready` or `failed`. Font uploads
still validate only the container signature; the document does not imply text
shaping or full font acceptance.

The map/tile contract documents finite nonnegative `geello_time`, case-sensitive
typed queries, rejection of unknown/duplicate fields, original XY axes, PNG,
WebMercatorQuad bounds, metatile behavior and conditional responses. ETags
identify encoded bytes, while render revision identifies the pinned graph.
Publication checks precede cached 200/304 responses. The contract describes the
existing exact/strong `If-None-Match` matching implementation; weak tag matching
is not claimed. `X-Geello-Incomplete` remains attached to cached image/tile
responses.

OGC Maps/Tiles uses `/ogc/maps` and `/ogc/tiles` with published map
collections, landing/conformance/API links, collection metadata, the two fixed
`WebMercatorQuad` and `WorldCRS84Quad` matrix sets, map tileset metadata and
PNG tiles. `WorldCRS84Quad` is 2×1 at z0; both grids are top-left/downward,
finite and non-wrapping. Map `tiles.matrix_sets`, `min_zoom` and `max_zoom`
are validated at publication and checked against the pinned map before cache
lookup. Collection discovery reads only published maps and exposes no source
paths or definitions. The conformance response has an empty `conformsTo` list
and explicitly says “tested subset; not an official conformance certification”;
Features, WFS and WebSocket remain experimental subsets. WMS/WMTS are
limited to the documented KVP operations. WFS exposes GetCapabilities,
DescribeFeatureType and GeoJSON GetFeature over published vector datasets.
Its XML POST accepts only a single empty/self-closing GetFeature element with
a small attribute set; the parser is not namespace-aware. Full namespace-aware
XML/query handling and protocol-specific exception responses remain pending.
Nested queries/text, extra trailing elements and mismatched closing names now
return `400` instead of silently discarding input. Unit and router regressions
cover these failures; this does not turn the parser into a full XML implementation.
The current generic XSD is not evidence of schema inference or GeoJSON validation.

OGC API Features exposes published vector datasets with strict bbox/offset/
limit/cursor parsing, source-order pages and original typed GeoJSON IDs.
`rel=next` preserves bbox and returns a generation-bound
`v1.<generation>.<offset>` token. Any catalog publication change expires it
with `410`. This is **not** the accepted fixed-snapshot cursor: retained pins,
absolute creation expiry, restart behavior, immutable query binding, atomic
version selection, unambiguous single-item ID encoding and CRS normalization
still need implementation and refresh/retention regressions. Temporal
filtering also remains pending. `conformsTo` stays empty.

## Request correlation and WebSocket follow-up

Trace: **API-005/006/007, T-39/T-44/T-45/T-46/T-47** (partial evidence).

Every application response carries `X-Request-Id`, including malformed-query,
404/405, capacity/shutdown rejection, HEAD, conditional 304 and WebSocket 101.
A single nonempty visible ASCII ID up to 128 bytes excluding quotes and commas
is echoed. Missing, invalid, oversized or duplicate values are replaced with a
generated ID. Correlation does not change response bodies, ETags or cache keys.
Malformed HTTP rejected before the application remains outside this contract.

The WebSocket route pins one published map graph at upgrade and sends hello
before init. Bounded init/update/retry controls accept only declared CRS values,
PNG and ASCII request IDs up to 128 bytes. Accepted state changes are atomic;
rejected grouped updates preserve the previous state/revision. A single writer
sends ack, frame metadata and one PNG binary message in order. Metadata contains
both the client control revision and the actual resource render revision.

Playing time uses a monotonic, speed-scaled anchor; paused time stays fixed.
View/output updates and retry sample that anchor without rewinding to the last
rendered frame. One render failure suspends automatic output; a valid update
or retry redraws, success resumes playback, and another failure stays suspended.
Each send uses the experimental profile's operation timeout. Any failed or
timed-out send drops the connection and its retained pin. Shutdown also drops
an upgraded transport. A client reconnects by supplying a complete init state
to current resource versions; no server-side session persistence is required.

Frames now use `WorkClass::Animation` for preparation, execution and encoding,
and bypass both rendered-area caches and the hybrid HTTP response cache and
singleflight tables. Native block caching remains shared. A regression holds
the sole HTTP active slot and renders an animation frame, then proves through
execution counts and restart that animation neither populates nor consumes
HTTP response caches. It also checks retained memory and pin release.

Seven WebSocket tests include a bounded-channel session exercising failed init,
failed retry, repaired publication/update, full-state reconnect to a changed
render revision, and a sink stuck during hello. A separate loopback TCP test
checks actual HTTP 101/accept headers and correlation, hello ordering, ack/frame/
PNG pairing and lengths, grouped invalid-update atomicity, retry without revision
change, ping/pong, and connection-pin release. Its router check also exhausts
HTTP request slots and verifies correlation on capacity rejection. This is
focused implementation evidence, not a general WebSocket load/conformance suite.

Latest-state receive/coalescing queues, stale frame disposal, paused redraw on
refresh, unpublish event/close after a started pair, fair shared-device admission,
JPEG, clock overflow handling and complete backpressure remain unfinished.

Both documentation routes use normal HTTP request admission and shutdown.
The JSON parser workspace is admitted before allocation, including the dynamic
server URL, and output uses bounded serialization and an accounted response
body. A profile whose registry output ceiling cannot accommodate the document
returns a controlled `503` budget problem and releases admission. HTML is
embedded static bytes with no per-request document construction. It uses no
JavaScript, CDN, iframe, remote font or viewer. Its relative `api` link works
under a reverse proxy prefix. A restrictive content security policy and
`nosniff` header accompany the HTML.

## Error compatibility and migration

Valid request parameters and successful resource/render schemas are unchanged.
Malformed typed query/path requests previously returned Axum plain-text
rejections. They now return a stable RFC 9457 body with `type`, `title` and
`status`; caller values, deserializer type details and invalid byte sequences
are not reflected. Routing 404/405 bodies follow the same format. Clients that
parsed plain-text framework messages should use status and problem `type`.
Application failures retain their existing problem categories and bounded
dependency references. Lower-level malformed HTTP rejected before Axum routing
is outside this application contract. Existing resource records and image
caches require no migration.

## Regeneration and verification

The source of the generated documents is
[`tools/generate_http_docs.py`](../../tools/generate_http_docs.py). Python 3
is maintenance tooling only, with no third-party packages or Rust/runtime
dependency. Both generated files are checked-in artifacts embedded in the binary.

```text
python tools/generate_http_docs.py
python tools/generate_http_docs.py --check
cargo test --all-features --locked --test http_contracts
```

The seven HTTP regressions check every documented method against the live
router, all local schema references, unique operation IDs/parameters, absence
of unimplemented public protocols, configured URL isolation and binary flags.
They validate actual readiness/catalog/resource/operation/problem payloads
against the document's schema subset, exercise a real upload/publication/
unpublish/deletion lifecycle, confirm strong resource ETags, sanitize invalid
query/path values, preserve 405 `Allow`, and check HEAD/static HTML. Memory
tests retain and drop a JSON response, then exercise an insufficient output
ceiling. These are implementation contract checks, not an official OpenAPI or
OGC conformance certification.

Final Windows/MSVC verification:

| Command | Result |
| --- | --- |
| `cargo test --all-features --locked` | 135 passed, 0 failed, 7 explicitly ignored |
| `cargo clippy --all-features --all-targets --locked -- -D warnings` | Pass |
| `cargo fmt -- --check` | Pass |
| `python tools/generate_http_docs.py --check` | Pass; 47 paths, 50 operations |
| `cargo check --no-default-features --features server,read-geojson,render-cpu --locked` | Pass |
| `cargo check --no-default-features --features server,read-geojson,render-gpu --locked` | Pass |
| `git diff --check` | Pass |

Ignored tests require real GPU access, owner-provided large local assets, or
opt-in catalog scale measurements. The library group contributes 18 passes
and one ignored test; ordinary integration groups contribute 117 passes and
six ignored tests. The preceding [animation checkpoint](10-m4-animation-and-evidence.md)
records the geometry/time portion. This HTTP increment enables Axum's `ws`
feature and records the resulting locked WebSocket transitive dependencies.
All-feature/all-target Clippy with warnings denied, formatting and generated
document freshness checks pass. CPU-only and GPU-only server builds with the
GeoJSON reader compile successfully. No real GPU or official protocol
conformance suite is run by this documentation increment.
Earlier document checks covered local links/anchors and embedded examples;
the generated route/schema/HTML contracts are rechecked by the HTTP suite.

Remaining M4 work includes Unicode shaping/bidi/fallback and font fixtures,
global/curved label layout, complete agreed legacy operations/error XML and
namespace-aware WFS queries, true fixed-snapshot feature pagination, the
WebSocket lifecycle gaps above, and independent protocol demonstrations.
G-04/G-06 remain open. Each protocol must extend its documentation and tests
when implemented; current subsets do not close the M4 exit criteria.
