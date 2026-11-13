# Public APIs, configuration, and protocol contracts

This document describes the **target**. None of the new APIs or schemas is implemented by this documentation change. Behavioral requirements are Accepted; Rust identifiers and example field spellings below are proposed interface forms to carry those requirements into implementation. They are not a claim of source compatibility, a released wire schema, or executable examples for v0.2.0.

## 1. Public library surfaces

Use conceptual modules `data`, `projection`, `style`, `animation`, `text`, `scene`, `render`, `readers`, and `storage`. The first seven belong to in-memory layer 1; the last two to layer 2. The server module is feature-gated and depends on these surfaces.

| Type/surface | Required contract |
| --- | --- |
| `PropertyValue` | Null/Bool/String/I64/U64/F64/Array/Object without flattening; missing is distinct |
| `FeatureId` | String or full-range integer, with numeric canonical equality and source provenance |
| `Feature`, `VectorBatch` | Native XY geometry, distinct null/empty state, properties, source ordinal, source ordinates; owned/shared batches |
| `RasterBlock`, borrowed raster view | Contiguous native numeric bands and validity/alpha/grid metadata; no arbitrary strides |
| `Crs`, projection operation | Validated identity/definition and explicit backend/axis/unit/domain; not `u16` EPSG-only |
| `View` | CRS, finite valid bounds, dimensions, aspect policy; explicit minimum span for degenerate auto-fit |
| `MapComposition` | Ordered draw items with resolved styles, fonts and animation bindings; no paths/registry lookups in layer 1 |
| `OperationContext` | Runtime-independent cancellation/deadline, allocation/work budget and bounded diagnostics |
| `PreparedScene` | Opaque immutable scene; public summary only, no public backend command encoding |
| `CpuRenderer`, `GpuRenderer` | Backend execution with no source/style/layout IO; GPU async and runtime independent |
| Reader snapshot and cursor | Immutable metadata/native data access, independent bounded batched queries, terminal error versus EOF |
| Reader composition helpers | Read/query/project/prepare and one-shot image convenience without requiring a server |

Illustrative signatures, **not compilable API declarations**:

```rust
// Layer 1 receives resolved memory resources, never source paths.
fn prepare_scene(
    composition: &MapComposition,
    view: &View,
    time_seconds: f64,
    context: &OperationContext,
) -> Result<PreparedScene>;

fn render_cpu(
    renderer: &mut CpuRenderer,
    scene: &PreparedScene,
    context: &OperationContext,
) -> Result<RgbaImage>;

async fn render_gpu_pixels(
    renderer: &mut GpuRenderer,
    scene: &PreparedScene,
    context: &OperationContext,
) -> Result<RgbaImage>;

async fn render_gpu_target(
    renderer: &mut GpuRenderer,
    scene: &PreparedScene,
    target: &ExternalTarget,
    context: &OperationContext,
) -> Result<()>;

// Layer 2 opens sources and yields native data.
fn open_vector(source: &VectorSource, context: &OperationContext)
    -> Result<VectorSnapshot>;
fn query(snapshot: &VectorSnapshot, query: &VectorQuery)
    -> Result<VectorCursor>;
fn next_batch(cursor: &mut VectorCursor, context: &OperationContext)
    -> Result<Option<VectorBatch>>;
fn read_native_window(snapshot: &RasterSnapshot, window: &NativeWindow,
    context: &OperationContext) -> Result<RasterBlock>;
fn prepare_from_readers(composition: &ReaderComposition, view: &View,
    time_seconds: f64, context: &OperationContext) -> Result<PreparedScene>;
```

Expose explicit operations for projection, raster resampling and color mapping in addition to convenience calls. Do not make a raw raster-window read secretly resample/reproject. CPU and preparation functions stay synchronous; server workers provide their execution context. GPU functions expose async completion without synchronous wrapper APIs or a mandatory Tokio executor.

Pixel output is tight, top-left straight RGBA8, sRGB RGB, alpha-zero RGB zero. GPU targets are same-size single-sample RGBA8Unorm, full replace, with documented premultiplied content, usage/device validation and post-submit invalidation. The scene summary includes dimensions, resource identities, command/label statistics, estimated accounted bytes and bounded diagnostics. It must not expose reader handles or encourage callers to depend on internal Vello scene encoding.

## 2. Feature composition

| Feature | Responsibility | Composition rule |
| --- | --- | --- |
| `default` | CPU and GPU execution | Enables `render-cpu`, `render-gpu`; does not imply server/readers/native PROJ |
| `render-cpu` | Vello CPU backend | Text support included |
| `render-gpu` | Vello GPU/wgpu backend | Text support included |
| `read-geojson` | Layer-2 GeoJSON input | Root-aware streaming implementation required |
| `read-shapefile` | Layer-2 Shapefile/DBF input | Explicit encoding policy |
| `read-geotiff` | Layer-2 TIFF numeric input | Limited to documented supported layouts |
| `derived-store` | FGB companion/index shards and raster blocks | No server registry or response cache |
| `encode-images` | PNG/JPEG encoding utilities | No default image codecs beyond approved formats |
| `server` | Axum/Tokio protocols, registry, scheduling/cache | Includes encoding and derived store; requires at least one selected render backend |
| `server-full` | Complete ordinary server | Server, all three readers, derived store, encoders and CPU/GPU; excludes native PROJ |
| `native-proj` | Optional fixed native PROJ operation | Explicit per-map/operation choice; no automatic fallback |

Text processing belongs to core scene preparation, including backend-free builds. Backend flags control execution, not the presence of text definitions. `server` can be combined with a subset of readers; unavailable source types fail clearly. The binary should use Cargo `required-features = ["server"]`; a server without any backend fails compilation with an explanatory message.

Future verification commands, not evidence that these features exist today:

```text
cargo check --no-default-features --lib
cargo test --no-default-features --features render-cpu --lib
cargo check --no-default-features --features render-gpu --lib
cargo check --no-default-features --features server,render-cpu,read-geojson
cargo check --no-default-features --features server-full
cargo check --no-default-features --features server-full,native-proj
```

## 3. Configuration graph and validation

The public data plane and administration plane are separate. The target administration API is rooted at `/admin/v1` and must be protected by the deployment reverse proxy. It manages published resource records; it does not expose arbitrary filesystem access or a built-in user/tenant authorization system.

### Resource administration API

Resource kinds are `dataset`, `style`, `map`, `animation` and `font`. Every resource uses the same `namespace:local` identifier grammar as public resources and declares `managed` or `external` ownership.

| Method | Target endpoint | Result |
| --- | --- | --- |
| GET | `/admin/v1/resources` | List IDs, kind, ownership, version, status, references and bounded diagnostics |
| GET | `/admin/v1/resources/{kind}/{id}` | Return one resource record and current operation/reference state |
| PUT | `/admin/v1/resources/{kind}/{id}/content` | Stream a managed single file or restricted ZIP into a new version |
| PUT | `/admin/v1/resources/{kind}/{id}/source` | Register an external local path and interpretation metadata without copying it |
| POST | `/admin/v1/resources/{kind}/{id}:unpublish` | Stop new discovery/use while retaining record and content |
| DELETE | `/admin/v1/resources/{kind}/{id}` | Remove a record only; does not delete managed or external bytes |
| DELETE | `/admin/v1/resources/{kind}/{id}/content` | Delete managed content after reference/version checks; external content returns conflict |
| GET | `/admin/v1/operations/{operationId}` | Poll upload/validation/preparation/publication status |

Create uses `If-None-Match: *` (or an equivalent absent-resource precondition). Replace, unpublish, remove and content deletion require the current resource ETag/version. A stale precondition returns `409 Conflict` and leaves the current generation unchanged. A resource referenced by a published map/style/animation/font or an in-flight snapshot also returns `409` with a bounded dependency list. There is no cascading unpublish or delete.

Here the administration contract deliberately reports version conflicts as 409, including a stale `If-Match`; document that distinction from the usual HTTP precondition-failure status in OpenAPI. Resource ETags describe resource records, not the encoded-image ETags used by public HTTP. Recheck the version/reference graph at commit after asynchronous preparation, not just when an upload starts. Content deletion removes the managed content and its registration after the required checks. External content deletion is refused; record removal of an external source remains supported.

The content endpoint accepts a streamed single file for supported GeoJSON, GeoTIFF, TOML and font resources. A Shapefile and sidecars use a restricted ZIP. The server rejects absolute paths, traversal, symbolic links, nested archives, unsafe duplicate targets, excessive request/expanded bytes, file count, member size or compression ratio. It writes to staging and never treats an archive member path as an external destination. Resumable chunk uploads and TAR archives are outside phase 1.

Uploads return `202 Accepted` with an operation ID when asynchronous validation/preparation is required. Operation state is `staging`, `preparing`, `ready`, `failed` or `unpublished`. Only a `ready` operation atomically changes the catalog generation; failures retain the prior complete publication. Operation diagnostics are bounded and do not expose arbitrary host paths.

Operation readiness and publication visibility are distinct. A failed replacement operation does not mark the still-serving old version failed. Unpublish/removal/deletion also change the catalog atomically, but do not introduce content as `ready`. Listing/status must expose the current published version separately from a pending operation. Unpublish preserves in-flight/page lifetimes under the existing retention contract; remove/delete additionally check those references and return conflict until they drain.

Illustrative management requests (schema and numeric limits are target examples, not current runnable endpoints):

```http
PUT /admin/v1/resources/dataset/demo:stations/content
If-None-Match: *
Content-Type: application/geo+json

<streamed GeoJSON bytes>

HTTP/1.1 202 Accepted
Content-Type: application/json

{"operation_id":"op-001","resource":"demo:stations","status":"preparing"}
```

```http
PUT /admin/v1/resources/dataset/demo:roads/content
If-Match: "D1"
Content-Type: application/zip

<restricted roads.zip containing roads.shp, roads.shx, roads.dbf, roads.prj>

PUT /admin/v1/resources/dataset/demo:external-source/source
If-None-Match: *
Content-Type: application/json

{"path":"/srv/geello/roads/roads.shp","format":"shapefile","encoding":"UTF-8"}
```

```http
GET /admin/v1/operations/op-001

{"operation_id":"op-001","status":"ready","resource":"demo:stations",
 "version":"D2","diagnostics":[]}

DELETE /admin/v1/resources/dataset/demo:external-source/content
If-Match: "E1"

HTTP/1.1 409 Conflict
{"type":".../external-content","title":"External content is not owned",
 "status":409,"resource":"demo:external-source"}
```

Style upload and the three distinct removal actions:

```http
PUT /admin/v1/resources/style/demo:stations-style/content
If-Match: "R1"
Content-Type: application/toml

<streamed stations.toml bytes>

POST /admin/v1/resources/map/demo:overview:unpublish
If-Match: "M1"

DELETE /admin/v1/resources/map/demo:overview
If-Match: "M2"

DELETE /admin/v1/resources/dataset/demo:stations/content
If-Match: "D2"
```

Each is an independent example, not an unconditional sequence. First resolve published dependents, unpublish where necessary and let pinned snapshots drain. The final request deletes managed bytes plus registration; record-only removal retains bytes. An example reference conflict uses a bounded response body:

```json
{
  "type": "urn:geello:problem:resource-in-use",
  "title": "Resource has active references",
  "status": 409,
  "resource": "demo:stations-style",
  "references": [{"kind": "map", "id": "demo:overview"}],
  "references_truncated": false
}
```

Source-definition TOML, style/map/animation definitions and font files enter the corresponding resource kind. A dataset definition references an explicitly registered source/content location; it does not authorize copying or taking ownership of arbitrary paths embedded in an upload. Managed package-relative references resolve inside that immutable version; external paths require local registration. A font resource may pair an explicit face definition with managed font bytes; neither form enables system-font scanning.

### Resource directories and upload limits

```toml
[resources]
managed_directory = "data/managed"
external_registry_directory = "data/external"
registry_directory = "data/registry"
staging_directory = "data/staging"
operations_directory = "data/operations"
derived_directory = "data/derived"

[upload]
max_request_bytes = "<temporary profile>"
max_archive_bytes = "<temporary profile>"
max_expanded_bytes = "<temporary profile>"
max_archive_files = "<temporary profile>"
max_member_bytes = "<temporary profile>"
max_compression_ratio = "<temporary profile>"
```

The quoted placeholders are documentation substitutions, not valid numeric settings for a deployable schema. The required limits will use numeric values from a measured profile, with final defaults approved later. All limits are enforced before unbounded extraction or preparation. The registry contains a small top-level catalog, type/namespace-sharded lookup files and complete copy-on-write generations. Requests use the current generation; they do not scan all resource files. The reconciler may report orphans without registering them.

One TOML per resource remains the canonical editable representation. A single giant TOML is unsuitable for frequent local updates and conflict isolation; independent TOMLs without an index are unsuitable for online lookup. SQLite metadata, embedded KV and external object/config services are deferred alternatives, not phase-1 dependencies.

Default startup requires `server.toml`; `--config` selects another file. Only a small documented deployment whitelist can be overridden by environment. No generic recursive merge, missing-file default, implicit global include or arbitrary property URL is supported.

Identifiers are `namespace:local`. Each component begins with an ASCII letter or underscore, followed by ASCII letters/digits/underscore/hyphen/dot. Titles may contain arbitrary text. Dataset and map publication are independent. Required references, duplicate IDs and unknown fields fail configuration validation. Resource paths are relative to the file that declares them.

`schema_version` identifies configuration shape, not publication content. All examples use proposed schema version 1. Resource bytes/interpretation determine runtime versions. The following examples form one illustrative linked graph assuming the named data/font files exist; their assets, budgets and application schema implementation are not delivered here.

### Server file: `server.toml`

```toml
schema_version = 1
catalog = "catalog.toml"

[listen]
address = "127.0.0.1:8000"

[http]
public_base_url = "https://maps.example.test/geello"

[render]
backend = "gpu-preferred"
initialization_fallback = "cpu"

[services]
ogc_features = true
ogc_maps = true
ogc_tiles = true
wms = true
wmts = true
wfs = true
websocket = true

[cache]
response_directory = "var/responses"
storage_engine = "psync"
recompress_responses = false

[storage]
derived_directory = "var/derived"

[tiles]
metatile_columns = 4
metatile_rows = 4
buffer_px = 128

[websocket]
default_fps = 30
max_fps = 60
```

Capacity, queue, deadline, message-size, cursor TTL and disk-budget fields are intentionally not assigned invented production defaults here. A deployable configuration needs the later approved profile. The example is parseable TOML illustrating resource relationships, not a complete deployment configuration. `hello` must report effective configured/backend limits once implemented.

Listen/backend/budget/cache/service-switch changes require restart. Catalog/map/style/animation/font/data changes prepare and publish as immutable graph updates. The explicit external base URL drives discovery/pagination links; forwarded headers do not override it.

### Container variants

The target build is a musl builder followed by a pinned Alpine runtime. The ordinary image tag is the default GPU image and includes both GPU and CPU render features:

```text
geello:<version>        # GPU-preferred Alpine image, CPU initialization fallback
geello:<version>-cpu    # independent CPU-only Alpine image
```

The default GPU image contains the approved Vulkan loader/runtime and requires documented host-device mapping. Its startup reports the actual backend. A GPU initialization failure may select the included CPU backend; a later runtime GPU fault remains an error/suspension until explicit reinitialization. The CPU image has no GPU renderer or Vulkan runtime and starts with the CPU backend directly. Both variants use the same resource volume layout but have separate build, startup, functional, volume, size and performance verification.

```text
/var/lib/geello/managed
/var/lib/geello/external
/var/lib/geello/registry
/var/lib/geello/staging
/var/lib/geello/operations
/var/lib/geello/derived
```

The container runtime user must have only the required write permissions for staging, operations and atomic generation directories. Native PROJ is excluded from both ordinary variants until its fixed dynamic distribution/no-fallback gate closes. GPU image labels and Alpine base tags/digests are not validation evidence by themselves.

Both variants target `x86_64-unknown-linux-musl` and build checked-out source with approved locked dependencies. The default GPU variant uses `server-full` (CPU and GPU); the CPU variant explicitly disables default features and selects `server,render-cpu,read-geojson,read-shapefile,read-geotiff,derived-store,encode-images`. Neither selects `native-proj`. Linux GPU validation is a real Vulkan device test inside the Alpine image, separate from Windows DX12 validation of non-container library/server builds. Verify the Vulkan loader and vendor driver ABI in the musl runtime; static Rust linkage alone does not prove GPU dynamic-loading compatibility. If incompatible, return evidence/options to G-10 without silently replacing Alpine, musl or the GPU default.

Use non-root runtime identities, explicit UID/GID access to GPU devices and configured mounts. External datasets should be mounted read-only; writes are needed for managed content, registry, staging, operations, derived data and response cache. Staging and final publication paths must support the required atomic move/switch on the same filesystem, or reject the deployment layout. No base tag/digest, APK version or host driver is newly approved by this document; those join the dependency/image inventory. Readiness reports selected backend and per-resource preparation/failure without requiring every lazy dataset to prepare before process startup.

### Catalog file: `catalog.toml`

The following hand-authored catalog is a bootstrap/import example for a small installation. The `catalog` startup field above denotes this import input. It must be validated and committed through the same resource-generation path as administration updates; it is not a second live writable registry. In an initialized installation, the generated `resources.registry_directory/catalog.toml` points to the authoritative current generation. Editing/importing definitions never bypasses concurrency, references or atomic publication. This distinction replaces the earlier flat-catalog-only design.

```toml
schema_version = 1

[[datasets]]
id = "demo:stations"
title = "Stations"
source = { kind = "geojson", path = "data/stations.geojson" }
identity = { kind = "attribute", field = "station_id", stable = true }
publish = { ogc_features = true, wfs = true }
queryables = [{ name = "station_class", field = "class", type = "string" }]

[[datasets]]
id = "demo:roads"
title = "Roads"
source = { kind = "shapefile", path = "data/roads.shp", encoding = "UTF-8" }
publish = { ogc_features = true, wfs = true }

[[datasets]]
id = "demo:elevation"
title = "Elevation"
source = { kind = "geotiff", path = "data/elevation.tif", sample_kind = "continuous" }
publish = { ogc_features = false, wfs = false }

[[styles]]
id = "demo:stations-style"
path = "styles/stations.toml"

[[styles]]
id = "demo:roads-style"
path = "styles/roads.toml"

[[styles]]
id = "demo:elevation-style"
path = "styles/elevation.toml"

[[animations]]
id = "demo:pulse"
path = "animations/pulse.toml"

[[fonts]]
id = "demo:label-normal"
path = "fonts/label-normal.ttf"
face_index = 0

[[maps]]
id = "demo:overview"
path = "maps/overview.toml"
publish = { ogc_maps = true, ogc_tiles = true, wms = true, wmts = true, websocket = true }
```

The data CRS uses explicit override before metadata; GeoJSON otherwise defaults to CRS84. Missing Shapefile/GeoTIFF CRS requires configuration. Public data-plane requests never accept file/URL paths. Only protected local registration accepts a local path; it does not fetch a URL or copy/delete external files. Raster datasets do not become feature collections merely because they are registered. Font examples refer to application-supplied assets, not system fonts or implicitly installed test subsets.

### Map file: `maps/overview.toml`

```toml
schema_version = 1
id = "demo:overview"
title = "Overview"
language = "und"
projection_backend = "rust"
output_crs = ["OGC:CRS84", "EPSG:3857"]
background = "#00000000"

[tiles]
matrix_sets = ["WebMercatorQuad", "WorldCRS84Quad"]
min_zoom = 0
max_zoom = 22

[[items]]
id = "terrain"
dataset = "demo:elevation"
style = "demo:elevation-style"

[[items]]
id = "roads"
dataset = "demo:roads"
style = "demo:roads-style"

[[items]]
id = "stations"
dataset = "demo:stations"
style = "demo:stations-style"

[[items.animation_bindings]]
id = "station-pulse"
animation = "demo:pulse"
track = "diameter"
target = { rule = "all-stations", symbol = "marker" }
start = 0.0
speed = 1.0
repeat = "loop"
```

An ordered item identifies the dataset and style. Bindings target stable rule/symbol IDs rather than indexes. Geometry bindings additionally identify stable effective feature IDs and the explicit transform/anchor sequence. A color/style animation does not by itself require stable cross-refresh feature identity unless it selects IDs. Per-map backend here means projection backend; the execution renderer is configured separately by the server.

Example `output_crs` is a requested publication allowlist, not proof that every source/backend/domain combination has passed projection acceptance. Publication must verify those combinations.

### Point and label style: `styles/stations.toml`

```toml
schema_version = 1
id = "demo:stations-style"

[[rules]]
id = "all-stations"

[[rules.symbols]]
id = "marker"
kind = "point"
shape = "circle"
size = { value = 6.0, unit = "px" }
fill = "#2780E3"
opacity = 1.0
angle = 0.0

[[rules.symbols]]
id = "name"
kind = "text"
text = { field = "/name" }
fonts = ["demo:label-normal"]
size = { value = 12.0, unit = "px" }
color = "#000000"
line_height = 1.2
priority = 0
allow_overlap = false
```

No rule filter means match all. An absent `/name` without explicit fallback is a typed evaluation failure, subject only to an explicitly chosen tolerant record policy. Label size 12px/black/line-height 1.2/no halo/no wrap are documented example defaults for schema work, not additional font assets or acceptance evidence. Text angle/layout is not an animation target.

### Line style: `styles/roads.toml`

```toml
schema_version = 1
id = "demo:roads-style"

[[rules]]
id = "all-roads"

[[rules.symbols]]
id = "casing"
kind = "line"
width = { value = 3.0, unit = "px" }
color = "#202020"
cap = "butt"
join = "miter"
miter_limit = 4.0

[[rules.symbols]]
id = "interior"
kind = "line"
width = { value = 1.0, unit = "px" }
color = "#FFFFFF"
cap = "butt"
join = "miter"
miter_limit = 4.0
```

All road casings draw before all road interiors across every batch. Separate polygon example:

```toml
schema_version = 1
id = "demo:area-style"

[[rules]]
id = "land"

[[rules.symbols]]
id = "fill"
kind = "area"
fill = "#808080"
opacity = 1.0

[[rules.symbols]]
id = "boundary"
kind = "line"
rings = "all"
width = { value = 1.0, unit = "px" }
color = "#000000"
```

The polygon example is independent of the linked overview graph. Areas fill even-odd per polygon; outline is a separate symbol. Defaults are point blue circle 6px with no stroke; line black 1px, butt/miter limit 4; area gray fill; opacity 1 and angle 0; transparent map. Circle size is diameter, square size is side, equilateral triangle size is base; center is its bounding-box center. Colors accept only `#RRGGBB`/`#RRGGBBAA` sRGB.

Numeric expressions use explicit units, such as `{ field = "/width", fallback = 1.0, unit = "px" }` or `{ value = 6.0, unit = "map" }`. Map units mean target CRS units, not geodesic meters. Filters support JSON Pointer paths and typed `eq/neq/lt/lte/gt/gte/in/exists/is-null/and/or/not` nodes. The exact tagged TOML filter encoding must be validated with the schema implementation; missing/incompatible comparison yields unknown and final unknown does not match.

### Raster style: `styles/elevation.toml`

```toml
schema_version = 1
id = "demo:elevation-style"

[[rules]]
id = "gray"

[[rules.symbols]]
id = "elevation"
kind = "raster"
mode = "gray"
band = 1
range = { kind = "dataset-minmax" }
resampling = { enlarge = "bilinear", reduce = "area-average" }
opacity = 1.0
```

Example configuration band numbers are one-based and must be translated once at the API boundary. RGB/RGBA mappings name selected channels explicitly; alpha association comes from reliable metadata or explicit memory/source interpretation. Do not equate source alpha with validity or a hidden mask. Automatic ranges belong to DataVersion, not each viewport.

### Animation file: `animations/pulse.toml`

```toml
schema_version = 1
id = "demo:pulse"
duration = 2.0

[[tracks]]
id = "diameter"
parameter = "size"
interpolation = "linear"
unit = "px"

[[tracks.keyframes]]
time = 0.0
value = 6.0

[[tracks.keyframes]]
time = 1.0
value = 12.0

[[tracks.keyframes]]
time = 2.0
value = 6.0
```

Duration/tracks/keyframes are independent of map binding start/speed/repeat. Times must strictly increase and remain within the animation's finite duration. Before binding start, base values apply; once-ended keeps final values; loops wrap binding-local time. The first-stage keyframe boundary behavior must hold the nearest endpoint outside a track's own range within its animation; this is an example-schema convention to test explicitly, not source-history interpolation.

### Resource validation and failure

Validate unknown fields, identity uniqueness, supported enabled formats, CRS consistency, font faces, animation targets/units/ranges and declared queryable types before publication. A config graph can be valid while a lazily prepared source later fails. Any affected replacement graph must remain unpublished on failure; the previous complete version stays usable.

## 4. Protocol and discovery surfaces

All routes below are target routes under `public_base_url`. Resource IDs in examples use `demo:stations` and `demo:overview`; clients must URI-encode path/query values as appropriate. Do not derive a resource from a request path into the filesystem.

| Root | Discovery/data routes | Published object |
| --- | --- | --- |
| `/ogc/features` | `/`, `/conformance`, `/api`, `/api.html`, `/collections`, `/collections/{id}`, `/collections/{id}/items`, `/collections/{id}/items/{featureId}` | Vector datasets |
| `/ogc/maps` | `/`, `/conformance`, `/api`, `/api.html`, `/collections`, `/collections/{id}`, `/collections/{id}/map` | Configured map compositions |
| `/ogc/tiles` | `/`, `/conformance`, `/api`, `/api.html`, `/collections`, `/collections/{id}`, `/tileMatrixSets`, `/tileMatrixSets/{tms}`, `/collections/{id}/map/tiles`, `/collections/{id}/map/tiles/{tms}`, `/collections/{id}/map/tiles/{tms}/{matrix}/{row}/{col}` | Map tile collections and tile sets |
| `/wms` | WMS 1.3.0 KVP Capabilities/GetMap | Published maps |
| `/wmts` | WMTS 1.0.0 KVP Capabilities/GetTile | Published map tile sets |
| `/wfs` | WFS 2.0.0 query subset, KVP GET/XML POST | Published vector datasets |
| `/ws/maps/{mapId}` | Versioned JSON control and image messages | One published map per connection |

Maps/Tiles collection IDs refer to map compositions, not implicitly every constituent feature dataset. The source of truth is links/discovery; clients should not infer arbitrary dataset-level/style routes from these examples. Public rendering uses named published variants and cannot submit arbitrary style definitions per request. Protected administration can upload/register a style, validate it and publish it through the catalog. Exact variant selector spelling and advertised conformance classes are frozen with their protocol contract tests.

Each OGC root provides a landing document with links to data, conformance and API definition. JSON OpenAPI 3.0 and static HTML API documentation describe actual routes/parameters/statuses, including vendor parameters, revalidation and errors. Do not claim the separate data HTML conformance class. OpenAPI JSON Schema dialect and JSON Schema 2020-12 property metadata are distinct outputs, not interchangeable schema versions.

Support PNG/JPEG according to advertised formats. OGC API metadata uses JSON and features use GeoJSON. Feature output is CRS84 XY; Z/M preservation is a library/provenance capability, not a promise to emit arbitrary-dimensional protocol coordinates. Axis normalization handles CRS84 longitude/latitude versus WMS 1.3 EPSG:4326 latitude/longitude.

Illustrative requests:

```http
GET /ogc/features/collections/demo:stations/items?bbox=116,39,117,41&limit=100&station_class=hub
Accept: application/geo+json

GET /ogc/maps/collections/demo:overview/map?bbox=116,39,117,41&width=1024&height=768
Accept: image/png

GET /ogc/tiles/collections/demo:overview/map/tiles/WebMercatorQuad/0/0/0
Accept: image/png

GET /wms?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&LAYERS=demo:overview&STYLES=&CRS=EPSG:4326&BBOX=39,116,41,117&WIDTH=1024&HEIGHT=768&FORMAT=image/png&TRANSPARENT=TRUE

GET /wmts?SERVICE=WMTS&VERSION=1.0.0&REQUEST=GetTile&LAYER=demo:overview&STYLE=default&TILEMATRIXSET=WebMercatorQuad&TILEMATRIX=0&TILEROW=0&TILECOL=0&FORMAT=image/png

GET /wfs?SERVICE=WFS&VERSION=2.0.0&REQUEST=GetFeature&TYPENAMES=demo:stations&STARTINDEX=0&COUNT=100&OUTPUTFORMAT=application/json
```

The WFS `demo` prefix must be bound to the configured published feature namespace in capabilities/schema and XML examples when implemented; it is not permission to resolve arbitrary prefixes. GeoJSON MIME/format aliases must be explicitly advertised. The examples illustrate equivalent normalized views; exact OGC API defaults/parameters remain bounded by the actual implemented conformance classes.

### Query and pagination semantics

Declared scalar equality queryables are top-level properties with unique aliases/types that cannot collide with protocol parameters. Equality uses strict strings, exact integers, finite floats and `true`/`false`. IDs are ORed, then combined with bbox/property filters using AND. No public CQL/FES general expression engine is implied. WFS XML accepts only the documented corresponding query subset with bounded namespace-aware parsing, never arbitrary XML resource loading.

OGC API Features 1.0.1 bbox semantics include null-geometry features as required by Core requirement 23C. Legal empty geometry does not intersect. Pure library spatial queries and WFS BBOX do not inherit that exception unless their protocol requires it.

There are no declared temporal data fields in phase 1. Still parse valid RFC3339 instants/intervals including open ends for `datetime`; timeless features match all valid temporal selections. Do not infer dates from string properties or connect this filter to animation time. The datetime parser implementation/dependency must be approved separately if a new crate is needed.

HTTP animation time defaults to zero through an explicitly declared vendor parameter. Proposed spelling is `geello_time` (seconds), not standard `TIME`/`datetime`. Every adapter must document its accepted spelling/case policy and avoid silently accepting unknown parameters. OGC API unknown/invalid query parameters return 400. Duplicate parameter and legacy case handling must be locked in protocol tests before claims of conformance.

Pages retain source order and output all properties. Cursor links refer to bounded server-side state fixing DataVersion/query/ordinal and absolute creation expiry. New WFS STARTINDEX/COUNT requests select a new snapshot; only returned cursor links guarantee the prior snapshot. Restart/expiry requires query restart, not silent movement to current data. Whole-page success is atomic; exact hits fail if they cannot finish within budget.

Metadata describes optional/null/union property types from the entire version or explicit schema. Queryable declaration validation remains strict. DescribeFeatureType returns XSD with reversible XML-name mapping annotations; separate JSON Schema 2020-12 metadata describes GeoJSON properties. XSD does not validate GeoJSON.

### Map/tile semantics

WMS can compose multiple published maps in request order into one canvas with global labels and one final background. Request view/CRS/time governs the render. Legal no-content results return successful empty images.

WMTS and OGC Tiles use 256px WebMercatorQuad and WorldCRS84Quad, top-left origin and downward rows, z0 matrices 1x1 and 2x1 respectively. Default zoom publication 0–22 may be narrowed; no custom grid or wrapped/out-of-range tile acceptance. Tile metatiles have the fixed agreed layout. Only requested tiles are encoded from a deduplicated area render.

No-version images use encoded-byte ETags and `Cache-Control: no-cache`; only return 304 after verifying current publication. The first stage does not promise immutable version-addressed URLs. Cache identity includes the selected backend, complete render versions and all output-affecting parameters.

## 5. WebSocket state machine

One connection binds one map through `/ws/maps/{mapId}`. Do not repeat a potentially conflicting map ID in `init`. After upgrade the server sends `hello`; before successful init no frames are generated. A successful init defaults to time zero/paused/one frame when playback is omitted, but supports complete explicit restoration. Connection state is not persisted by the server after disconnect.

Use JSON text for application controls/metadata and one raw PNG/JPEG binary message per image. The following schema illustrates the accepted behavior; literal version marker 1 and enum/field spellings are draft forms to freeze with tests. Symbols such as `H`/`V1` are illustrative opaque values, not real hashes or versions.

### Message vocabulary

| Direction | `type` | Contract |
| --- | --- | --- |
| Server to client | `hello` | Protocol version, effective formats/limits, default/max fps, actual backend; no reservation guarantee |
| Client to server | `init` | Initial view, variant, playback and output state; request ID |
| Client to server | `update` | One or more complete state groups; validate all before applying any |
| Client to server | `retry` | Resume frame generation after render failure using current valid state |
| Server to client | `ack` | Accepted request, revision, normalized state/known versions; does not promise render success |
| Server to client | `frame` | Metadata for exactly the next application binary message |
| Server to client | `event` | Resource changes, suspended rendering, unpublication or other declared lifecycle event |
| Server to client | `error` | Rejected control and correlation without altering state |

Client request IDs are bounded strings for correlation, not idempotency keys. Updates are processed in receive order with no `expected_revision` requirement. Before init, update/retry are invalid; repeated init after success is a protocol validation error. Malformed JSON, unknown fields/types, nonfinite/out-of-range values and invalid targets are rejected with controlled details. Binary client uploads are outside the protocol. Transport/message/rate limits and close behavior are bounded and must be finalized in the protocol implementation contract, not left to Axum's large defaults.

### State groups and complete initialization example

| Group | Fields/meaning |
| --- | --- |
| `view` | CRS, bbox, width and height in actual output pixels; no pixel ratio |
| `variant` | Published named style variant; no raw styles or file paths |
| `playback` | Current map time in seconds, positive speed multiplier, playing boolean |
| `output` | PNG/JPEG format, JPEG quality when applicable, requested fps |

```json
{
  "type": "init",
  "request_id": "initial-1",
  "view": {
    "crs": "OGC:CRS84",
    "bbox": [116.0, 39.0, 117.0, 41.0],
    "width": 1024,
    "height": 768
  },
  "variant": { "id": "default" },
  "playback": { "time": 0.0, "speed": 1.0, "playing": false },
  "output": { "format": "png", "fps": 30 }
}
```

An `update` can change output atomically with view/playback. Every supplied group is complete; omitted groups retain state. JPEG example quality is an explicit illustrative value, not an approved default:

```json
{
  "type": "update",
  "request_id": "seek-12",
  "playback": { "time": 12.5, "speed": 1.0, "playing": false },
  "output": { "format": "jpeg", "jpeg_quality": 90, "fps": 15 }
}
```

Validate dimensions against all canvas/device/budget limits, bbox/CRS/variant publication, output format/quality and requested fps. Default fps is 30, server maximum defaults to 60 and is configurable; exceeding the effective maximum is an error, not a clamp. `hello` reports effective backend/configured maxima but reserves no resources or throughput. Numeric capacities and initialization/send timeouts await the approved operational profile.

On success return ack before sending the corresponding new-state frame; an already started old frame pair completes first. Proposed ack form:

```json
{
  "type": "ack",
  "request_id": "seek-12",
  "revision": 2,
  "state": {
    "view": {
      "crs": "OGC:CRS84", "bbox": [116.0, 39.0, 117.0, 41.0],
      "width": 1024, "height": 768
    },
    "variant": { "id": "default" },
    "playback": { "time": 12.5, "speed": 1.0, "playing": false },
    "output": { "format": "jpeg", "jpeg_quality": 90, "fps": 15 }
  },
  "versions": { "render": "V1" }
}
```

Resource versions can still be preparing; ack cannot invent them. A frame reports the versions actually fixed for its execution, even if publication changes after ack. Status revision identifies client control state; refresh does not masquerade as a control update.

### Frame pair and playback behavior

```json
{
  "type": "frame",
  "frame_id": 18,
  "revision": 2,
  "time": 12.5,
  "versions": {
    "render": "V1",
    "data": { "demo:stations": "D1", "demo:roads": "D2", "demo:elevation": "D3" },
    "derived": { "demo:stations": "S1", "demo:roads": "S2", "demo:elevation": "S3" }
  },
  "backend": "gpu",
  "content_type": "image/jpeg",
  "width": 1024,
  "height": 768,
  "byte_length": 48217,
  "incomplete": false,
  "diagnostics": []
}
```

The next application message must be the complete binary JPEG with exactly the announced length. The length above is illustrative; no image bytes are delivered by this document. One writer owns metadata/image pairs and controls, preventing interleaved application messages. Transport ping/pong is normal. A started pair finishes or the connection drops, and an incomplete pair is discarded by the client.

`frame_id` advances per transmitted frame; gaps are legal. `revision` changes on accepted state changes, not every tick. Actual sampled time and resource versions belong to each frame. Clients ignore obsolete revisions and never assume all frames acknowledged for a state will be sent.

Playing uses a server monotonic anchor plus speed, drops frames under load and never slows logical time to maintain fps. A view/output update does not reset time; a complete playback group explicitly sets the new anchor. Pause/update produces a fresh frame. No map-wide duration is inferred; once animations hold their end values while time continues.

Before sending metadata, discard obsolete unstarted results and retain bounded latest work. A render failure emits a bounded event and suspends frame production; playing logical time continues. A valid update or retry resumes attempts. Invalid controls keep state. Resource refresh preserves time, including paused redraw. Unpublish denies new connections and closes current ones after the current started pair and event.

Reconnect has no server session token: the client sends its saved full state against current published resources. Old resource versions are not guaranteed. Ack means state acceptance only; render errors follow as events, not a retroactive negative ack.

## 6. Errors, diagnostics, and compatibility

Expose typed internal categories for validation, unsupported capability, unavailable/preparing source, capacity/deadline/cancellation, IO/corruption, projection and rendering. At the protocol boundary use RFC 9457 JSON problems for OGC/custom HTTP, legacy XML exceptions for WMS/WMTS/WFS and JSON control errors/events for WS. Include request correlation and bounded diagnostic references without exposing arbitrary paths or internals.

Whole successful pages/images are bounded before committing success. An explicitly tolerant render can succeed with an incomplete marker and diagnostic summary; cache entries preserve those fields. Legal empty results are successful, while invalid bbox, unsupported CRS/format, out-of-range tiles and malformed controls fail explicitly.

Before protocol implementation, lock complete error code/status/close-code tables, duplicate/case parameter rules, exact vendor parameter names, published variant selection, and actual conformance URIs in the contract tests. These are schema-level follow-ups under the accepted behavior, not permission to add capabilities or dependencies. New user-facing semantics require a recorded design decision.

Breaking legacy API/URL/RON compatibility is accepted. Migration guidance must map mutable `RenderedGeometry`/RenderOption calls to source data, resolved composition, PreparedScene and execution; request paths to registered IDs; old `/wmts/real-time`/`/wmts/cache` and `/ws/anim` to the new surfaces; and old filesystem/static viewer configuration to explicit resource graphs and independent examples.
