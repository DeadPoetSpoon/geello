# M4 animation implementation checkpoint

Historical checkpoint. Its remaining-work statements describe that checkpoint,
not current status. The [final M4 record](12-m4-completion-and-evidence.md)
supersedes them with implemented behavior and current verification.


Date: **2026-11-15**, branch `refactor`, Windows x86_64 MSVC, Rust/Cargo 1.96.
M3 functional exit conditions are satisfied as recorded in the
[final M3 assessment](09-m3-service-and-evidence.md#final-m3-exit-assessment--2026-11-15).
M4 is **in progress**. This checkpoint implements basic style animation,
stable-ID source-plane geometry animation and HTTP time selection; it does not claim text,
WebSocket, OGC conformance or M4 completion.

## Implemented boundaries

| Layer | Interface | Responsibility |
| --- | --- | --- |
| Memory library | `animation::Track`, `BindingClock`, `SymbolTrack`, `GeometryBinding`, `Trajectory` | Validated immutable tracks; deterministic arbitrary-time scalar/color/vector evaluation, symbol overrides and whole-feature source-plane transforms, without IO or Tokio |
| Configuration | `AnimationTrack::prepare`, `prepare_style_animations`, `prepare_geometry_animations`, `resolve_vector_style_at` | Compile TOML values and units, bind stable rule/symbol/feature IDs, reject conflicting writers and incompatible source CRS, preserve original-property filters/base values |
| Catalog | Commit-time graph validation | Recheck map bindings against the actual generation being published, including style/animation replacements; reject broken replacements without changing authority |
| Server | `render_map_at`, `render_tile_at` | Pin current resources, prepare and execute at explicit time, include time in area/tile singleflight and object/response cache identities |
| HTTP | Basic map/image, tile and discovery routes | Optional case-sensitive `geello_time` in seconds, default zero; `/api` OpenAPI 3.0.3 and `/api.html` static docs describe only implemented routes |

`Track::new` admits retained keyframes before cloning. Evaluation checks
cancellation and charges binary-search work; no previous frame, mutable source
or external lookup is retained. The configuration adapter admits temporary
conversion and retained binding metadata separately. Prepared scenes contain
resolved values and do not evaluate tracks inside CPU/GPU execution.

## Current style contract

| Parameter | Target | Keyframes and unit |
| --- | --- | --- |
| `size` | Point symbol | Finite nonnegative number; required `px` or `map` matching the base expression |
| `angle` | Point symbol | Finite raw degrees, including multiple turns; omit length unit |
| `width` | Line symbol or an existing point stroke | Finite nonnegative number; required `px` or `map` matching base width |
| `fill` | Point or area fill | `#RRGGBB` or `#RRGGBBAA`; omit length unit |
| `stroke-color` | Line symbol or an existing point stroke | Same hex color contract; omit length unit |
| `opacity` | Point, line, area or raster symbol | Number in `[0,1]`; omit length unit |

Missing targets/strokes and multiple bindings writing the same
item/rule/symbol/parameter fail preparation/publication. Different items have
independent targets. Unsupported parameters fail explicitly: geometry
transforms and trajectories use the separate geometry schema below.
Text layout targets remain excluded by the accepted design.

Keyframe times must strictly increase within a positive finite duration. Step
switches at the exact keyframe; linear interpolates between neighbors and holds
the nearest endpoint outside the track range. A binding has nonnegative finite
start, positive finite speed and `once` or `loop` repeat. Before start the
original-property base value applies; once-ended holds the last endpoint;
loop wraps to local time zero at an exact duration boundary. Loop clock overflow
returns an error; once-ended saturates at its terminal state. Angles interpolate
raw values: 350 to 10 passes through 180, without shortest-arc normalization.

RGB interpolation decodes sRGB to linear light, premultiplies by color alpha,
interpolates, unpremultiplies and encodes sRGB. Black/white midpoint is
`#BCBCBC`; transparent red to opaque blue midpoint is half-alpha blue. Alpha-zero
colors are canonical transparent black. Independent symbol opacity applies
afterward. Raster opacity replaces the base value during colorization exactly
once; raw samples, validity, calibration, resampling and original bytes stay
unchanged.

## HTTP selection and cache identity

```http
GET /maps/demo:overview/image?bbox=-1,-1,1,1&crs=OGC:CRS84&width=256&height=256&geello_time=0.5
GET /tiles/demo:overview/0/0/0?geello_time=0.5
```

Time must be finite and nonnegative; negative/NaN/infinite values return 400
before rendering. Unknown or duplicate query fields are rejected by the strict
query schema; extractor, routing and application errors use sanitized RFC 9457
`application/problem+json` responses. 405 responses retain `Allow`.
`0`, `-0` and omitted time share canonical zero identity. Time does not change
the resource `RenderRevision`; encoded-byte ETag changes when pixels/encoding
change. Time is included in render, crop/encode and disk cache identities.
The scene semantics salt is now `geello-geometry-render-v4`, avoiding reuse of
older binaries' responses. Publication checks still precede cached 200/304.
The bounded HTTP cache supports these explicit-time requests; future WebSocket
intermediate frames must bypass the disk response cache.

Upload style and animation resources before publishing the referring map via
the existing conditional `/admin/v1/resources/{kind}/{id}/content` operations.
Poll each operation to `ready`. These are application-schema examples for the
implemented subset; existing published maps must be replaced using their ETag.

Style body:

```toml
schema_version = 1
id = "demo:points-style"
[[rules]]
id = "base"
[[rules.symbols]]
id = "marker"
kind = "point"
size = { value = 6.0, unit = "px" }
fill = "#3388FF"
```

Animation body:

```toml
schema_version = 1
id = "demo:pulse"
duration = 2.0
[[tracks]]
id = "diameter"
parameter = "size"
interpolation = "linear"
unit = "px"
keyframes = [{ time = 0.0, value = 6.0 }, { time = 1.0, value = 24.0 }, { time = 2.0, value = 6.0 }]
```

Map body (requires a published vector dataset `demo:points`):

```toml
schema_version = 1
id = "demo:overview"
output_crs = ["OGC:CRS84", "EPSG:3857"]
[[items]]
id = "points"
dataset = "demo:points"
style = "demo:points-style"
[[items.animation_bindings]]
id = "pulse"
animation = "demo:pulse"
track = "diameter"
target = { rule = "base", symbol = "marker" }
start = 0.0
speed = 1.0
repeat = "loop"
```

## Geometry animation increment

The second increment implements **ANI-002, D-29, T-31** for the memory,
configuration and explicit-time HTTP surfaces. `animation::GeometryBinding`
owns typed stable effective IDs, a source CRS, one binding clock, an optional
`Trajectory` and ordered `GeometryAction` values. There is no source IO,
projection, viewport, Tokio dependency or previous-frame state in this layer.
Geometry is evaluated before projection; feature properties, original identity,
source ordinal and original Z/M provenance remain unchanged.

The [independent demonstration](../../examples/geometry-animation/README.md)
contains uploadable GeoJSON/style/animation/map files and image/tile requests.
Animation resources now have optional `geometry_tracks` in addition to optional
style `tracks`. A geometry track declares `source_crs` as the exact effective
`{ definition, space }` pair and `interpolation` as `step` or `linear`.
Map items bind those tracks using `geometry_bindings`, independently of
symbol-targeted `animation_bindings`. Each binding requires `feature_ids`, an
explicit positive `max_features`, `start`, `speed` and `repeat`.

| Operation | Required values | Interpretation |
| --- | --- | --- |
| Trajectory | `anchor`, `points`, `timing`; `times` for timestamp mode | Position the original anchor at a point on the source-plane polyline before transforms |
| `translate` | Keyframe XY pairs | Finite offsets in source coordinate units |
| `rotate` | Original explicit `anchor`; numeric keyframes | Raw counterclockwise degrees; 350 to 370 traverses 360 |
| `scale` | Original explicit `anchor`; XY keyframe pairs | Nonnegative dimensionless factors, including zero |

All anchors use original source coordinates and follow the trajectory and every
preceding transform. Transforms execute in configured order; repeated transform
kinds are legal. Rotation angles interpolate raw values; reduction modulo 360
occurs only for trigonometric evaluation. Complete lines, polygon exteriors and
holes, multi-geometries and nested collections are transformed together. A
rotated rectangle becomes a polygon with all four corners. Null/empty geometry
remains null/empty. There is no vertex morph, geodesic distance or world wrapping.
Geometry scaling does not change pixel-sized symbol dimensions.

Distance timing distributes the animation duration over cumulative Euclidean
segment lengths in the source coordinate plane. Repeated adjacent points add
no distance; an entirely stationary distance path is rejected. Timestamp mode
requires one strictly increasing finite timestamp per point within duration,
and permits stationary segments. Step timing switches at exact knots; linear
timing interpolates within a segment. Endpoints hold outside the path's own
timestamp range. Pre-start, speed, once and loop behavior follows style clocks.

Selection uses typed effective IDs: numeric signed/unsigned 42 compare equally;
string `"42"` is distinct. IDs starting with the reserved `generated:` prefix
are rejected; generated ordinals are unsuitable for cross-refresh binding.
Selections are independent of viewport. Two bindings selecting the same item
and feature are rejected; combine repeated operations in one ordered track.
Separate items can bind the same source independently. Selected count,
retained ID/keyframe/action bytes, source clones, output clones and diagnostic
storage are admitted before allocation; operation work and cancellation remain
bounded by the caller/server profile. The current basic renderer scans the
bounded source dataset rather than using a specialized animated-feature index;
there is no scalability/performance claim.

Publication validates geometry track existence, vector-only targets and exact
source coordinate interpretation against the generation being committed.
Replacing animation/data with an incompatible source CRS fails without
changing authority. Each request rebinds stable IDs to the pinned source and
recomputes transforms from original geometry at the requested map time, so
refresh neither accumulates transformations nor resets time. Missing IDs are
skipped with bounded scene diagnostics and `x-geello-incomplete: true`; image
and tile cache records preserve that flag. WebSocket clock/refresh lifecycle
remains pending with the WebSocket implementation.

New dataset catalog records retain optional `effective_crs` separately from
disposable derived files. Missing derived data therefore remains lazily
rebuildable after restart. Old catalog records without this field deserialize
unchanged and can validate new geometry bindings through an existing derived
manifest; the next explicit refresh/lazy rebuild records the metadata. No
source bytes or data-version identity are changed merely to add this field.

## Verification and remaining work

Trace: **ANI-001/003/004, D-28/D-30/D-31, T-30/T-32/T-33**, plus
STO-003/T-38/T-39 for graph replacement and cache time identity. These are the
implemented style/time portions. The geometry increment adds ANI-002/T-31
evidence above; connection refresh semantics remain pending.

The first increment's `tests/animation_contracts.rs` had six offline tests: arbitrary-time order,
endpoint/step boundaries, raw rotations, clocks, color interpolation,
invalid values/units, budget/cancellation release, TOML binding and preserving
original feature properties. Two additional server regressions validate vector
image/tile time separation, canonical zero/revalidation, failed style replacement
rollback, and raster alpha exactly once with unchanged source bytes.

At the first checkpoint, `cargo test --all-features --locked` passed **112 ordinary
tests / 7 opt-in ignored tests**. No direct dependency, feature or lockfile
change was needed for this increment; it uses the approved M3 closure. The
backend-free animation tests pass independently of server/Tokio/renderers.
CPU-only server, GPU-only server and backend-free library checks pass; all-feature
Clippy with warnings denied and formatting pass. Documentation verification
covers 49 local links/anchors and 19 TOML/JSON examples.

The geometry increment adds six offline library/configuration/example regressions and
one HTTP/service regression. The HTTP documentation increment adds five
contract tests covering live schemas, proxy URL isolation, rejection privacy,
HEAD/Allow behavior, output admission and operation/revalidation schemas.
Geometry tests cover source-plane distance/timestamps,
following anchors, ordered/repeated transforms, holes/collections/rectangles,
typed ID selection, original-property preservation, finite/range/CRS failures,
admission/cancellation, offscreen selection, projection order, data refresh,
missing IDs, animation/dataset rollback, legacy metadata and restart/lazy
reconstruction. HTTP image/tile time separation is exercised. All-feature tests
now pass **124 ordinary tests / 7 opt-in ignored tests** on Windows/MSVC.
This animation checkpoint did not alter direct dependencies, features or
`Cargo.lock`; the subsequent HTTP/WebSocket increment adds Axum's `ws` feature
and its locked transitive support.
Backend-free animation suites pass both without configuration (9 tests) and
with `config-toml` (12 tests). Backend-free library, CPU-only server and GPU-only
server compilation pass; all-feature/all-target Clippy with warnings denied
and formatting checks pass. Documentation verification covers 54 local
links/anchors and 23 TOML/JSON examples/files, including the committed demo.

At this earlier animation checkpoint, the generated OpenAPI/static documentation listed only
administration, readiness, image/tile and the then-implemented OGC Maps/Tiles subset. The
subsequent [HTTP checkpoint](11-m4-http-contract-and-evidence.md) records the added tested
Features, WMS/WMTS/WFS and bounded WebSocket routes. Their conformance response remains empty
and non-certifying; full WFS/XML, feature snapshot pagination and WebSocket playback/backpressure
remain pending. Static docs contain no JavaScript, CDN or viewer.
The [HTTP checkpoint](11-m4-http-contract-and-evidence.md) records route/schema
boundaries, maintenance commands, migration notes and remaining protocol work.

Remaining M4 deliverables: isolated Unicode fonts/shaping and global/curved label layout,
complete agreed public protocol subset and schemas, WebSocket clock/update/
frame-pair/backpressure lifecycle, and independent protocol demonstrations.
G-04 and G-06 still require their own evidence; this checkpoint does not close
them. M5 native PROJ, Linux/Vulkan/Alpine and measured production profiles remain
separate.
