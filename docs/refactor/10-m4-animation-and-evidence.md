# M4 animation implementation checkpoint

Date: **2026-11-15**, branch `refactor`, Windows x86_64 MSVC, Rust/Cargo 1.96.
M3 functional exit conditions are satisfied as recorded in the
[final M3 assessment](09-m3-service-and-evidence.md#final-m3-exit-assessment--2026-11-15).
M4 is **in progress**. This checkpoint implements basic style animation and
HTTP time selection; it does not claim complete geometry animation, text,
WebSocket, OGC conformance or M4 completion.

## Implemented boundaries

| Layer | Interface | Responsibility |
| --- | --- | --- |
| Memory library | `animation::Track`, `BindingClock`, `SymbolTrack` | Validated immutable tracks; deterministic arbitrary-time scalar/color evaluation and resolved symbol overrides, without IO or Tokio |
| Configuration | `AnimationTrack::prepare`, `prepare_style_animations`, `resolve_vector_style_at` | Compile TOML values and units, bind stable rule/symbol IDs, reject conflicting writers, evaluate original-property filters/base values before applying overrides |
| Catalog | Commit-time graph validation | Recheck map bindings against the actual generation being published, including style/animation replacements; reject broken replacements without changing authority |
| Server | `render_map_at`, `render_tile_at` | Pin current resources, prepare and execute at explicit time, include time in area/tile singleflight and object/response cache identities |
| HTTP | Existing basic map/image and tile routes | Optional case-sensitive `geello_time` in seconds, default zero |

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
transforms and trajectories require their subsequent M4 schema/implementation.
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
query schema; normalizing all extractor errors to RFC 9457 remains M4 work.
`0`, `-0` and omitted time share canonical zero identity. Time does not change
the resource `RenderRevision`; encoded-byte ETag changes when pixels/encoding
change. Time is included in render, crop/encode and disk cache identities.
The scene semantics salt is now `geello-timed-render-v3`, avoiding reuse of
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

## Verification and remaining work

Trace: **ANI-001/003/004, D-28/D-30/D-31, T-30/T-32/T-33**, plus
STO-003/T-38/T-39 for graph replacement and cache time identity. These are the
implemented style/time portions; ANI-002/T-31 and connection refresh semantics
remain pending.

`tests/animation_contracts.rs` has six offline tests: arbitrary-time order,
endpoint/step boundaries, raw rotations, clocks, color interpolation,
invalid values/units, budget/cancellation release, TOML binding and preserving
original feature properties. Two additional server regressions validate vector
image/tile time separation, canonical zero/revalidation, failed style replacement
rollback, and raster alpha exactly once with unchanged source bytes.

At this checkpoint, `cargo test --all-features --locked` passes **112 ordinary
tests / 7 opt-in ignored tests**. No direct dependency, feature or lockfile
change was needed for this increment; it uses the approved M3 closure. The
backend-free animation tests pass independently of server/Tokio/renderers.
CPU-only server, GPU-only server and backend-free library checks pass; all-feature
Clippy with warnings denied and formatting pass. Documentation verification
covers 49 local links/anchors and 19 TOML/JSON examples.

Remaining M4 deliverables: stable-ID source-plane geometry transforms and
trajectories, isolated Unicode fonts/shaping and global/curved label layout,
complete agreed public protocol subset and schemas, WebSocket clock/update/
frame-pair/backpressure lifecycle, OpenAPI/static docs and independent demos.
G-04 and G-06 still require their own evidence; this checkpoint does not close
them. M5 native PROJ, Linux/Vulkan/Alpine and measured production profiles remain
separate.
