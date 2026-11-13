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
validation. The protected `/admin/v1` API, OGC services, WebSocket animation,
derived stores and server runtime remain later milestones. The old mutable
`RenderedGeometry`/`RenderOption` and Rocket prototype were removed during the
refactor.

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

## Later server and container work

The target design defines a protected managed/external resource API, sharded
catalog generations, and a default GPU Alpine image with an independent CPU
`-cpu` variant. Docker, server and administration are intentionally
not implemented in M2; their requirements and validation gates remain in the
[roadmap](docs/refactor/06-implementation-roadmap.md).

## License

The code is available under the [MIT license](LICENSE).
