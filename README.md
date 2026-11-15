<div align="center">

# Geello

Geello is a geospatial scene preparation and rendering library built around
`geo-types`, Vello CPU/GPU, and explicit resource ownership.

![Geello](assets/logo.png)

</div>

> [!WARNING]
> Geello is an alpha project. The first-stage refactor is being implemented on
> the `refactor` branch.

## Design and implementation status

The [refactor documentation](docs/refactor/README.md) records the agreed
architecture, requirements, dependency decisions, and roadmap. M0 is complete.
M1 now provides the memory-only foundation:

- typed feature IDs, recursive properties, XY geometries, native raster bands,
  masks, grids, CRS, and views;
- resolved point, line, polygon, and image styles;
- immutable `PreparedScene` command packets and bounded diagnostics;
- cooperative cancellation, deadlines, work budgets, and memory accounting;
- synchronous Vello CPU execution and runtime-independent asynchronous Vello
  GPU execution;
- tight top-left straight RGBA8 output with canonical transparent pixels.

M2 now adds bounded GeoJSON, Shapefile/DBF and baseline GeoTIFF readers,
independent vector cursors, exact/bbox spatial query modes, Rust projection,
typed raster validity/calibration/resampling, and strict TOML resource/style
validation. M3 adds immutable vector/raster derived stores, a protected
`/admin/v1` resource lifecycle API, sharded atomic catalog generations,
bounded scheduling/caches and basic Axum map/tile PNG responses. M4 now adds
stateless style/raster and stable-ID source-plane geometry animation,
explicit isolated Unicode font shaping, global/curved labels, fixed-snapshot
Features queries, namespace-aware WFS KVP/XML GeoJSON queries, WMS multi-map
PNG/JPEG and WMTS fixed grids, plus bounded WebSocket playback/update/refresh
and paired images. See the [M4 exit evidence](docs/refactor/12-m4-completion-and-evidence.md)
and [independent protocol demonstration](examples/m4-protocols/README.md).
M5 deployment evidence and M6 release-readiness checks are recorded in
[13-m5-deployment-and-evidence.md](docs/refactor/13-m5-deployment-and-evidence.md)
and [16-m6-release-readiness-and-evidence.md](docs/refactor/16-m6-release-readiness-and-evidence.md).
M7 adds the automated pre-publish audit and container liveness checks documented
in [17-m7-release-automation-and-evidence.md](docs/refactor/17-m7-release-automation-and-evidence.md).
The public contract is served at `/api` (OpenAPI JSON) and `/api.html` (offline
HTML). OGC conformance responses remain explicitly non-certifying; full WFS/GML
and official certification are outside the agreed query subset. The old mutable
`RenderedGeometry`/`RenderOption` and Rocket prototype were removed during the
refactor.

See the [HTTP contract checkpoint](docs/refactor/11-m4-http-contract-and-evidence.md)
for schema regeneration, sanitized errors and client migration notes.

The [M4 animation checkpoint](docs/refactor/10-m4-animation-and-evidence.md)
includes uploadable style/map/animation bodies and image/tile time examples.
The [geometry demonstration](examples/geometry-animation/README.md) includes
independent resource files and requests for moving an offscreen point into view.

## M1 library example

```rust
let scene = geello::prepare_scene(&composition, &view, &context)?;
let image = geello::CpuRenderer::new(0).render(&scene, &context)?;
```

The default Cargo feature set enables both approved Vello adapters. Use
`--no-default-features` for the backend-free memory layer, or select
`render-cpu` and `render-gpu` independently. GPU rendering accepts an explicit
`wgpu` device and queue and does not require Tokio in the library.

## Verification

The current contract suite includes nested property and integer identity
round-trips, source release and scene reuse, geometry compatibility, open and
zero-length lines, polygon holes, view bounds, raster validity, cancellation,
budgets, CPU output, and CPU/GPU parity.

```text
cargo test --all-features --lib --tests
cargo test --test gpu_contracts -- --ignored --nocapture
```

The ignored GPU test requires a real device. The recorded workstation evidence
uses an NVIDIA GeForce RTX 4090 through DX12 and must be repeated for every
supported platform.

## Experimental server and later container work

```text
cargo run --locked --features server-full -- --config examples/server.experimental.toml
```

The profile binds to loopback and selects GPU with initialization-only CPU
fallback. Uploads create managed immutable files; local registration retains
external ownership. Reverse-proxy protection is required for `/admin/v1`.
All numerical settings are experimental pending measured approval.

The target container is a default GPU Alpine image with an independent CPU
`-cpu` variant. Docker, native PROJ and Linux/musl validation remain M5 work.
The [candidate M3 dependency inventory](docs/refactor/08-m3-dependency-closure.md)
is available for review; compatibility tests do not close G-01.

## License

The code is available under the [MIT license](LICENSE).
