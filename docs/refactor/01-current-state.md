# Current state and reference-branch assessment

Assessment date: **2026-11-12**. This document records repository facts, not target implementation claims. See [the target requirements](02-requirements-and-acceptance.md) for the redesign.

## 1. Branch baseline and reproducibility

| Ref | Commit | Relationship | Role |
| --- | --- | --- | --- |
| `main` | `841b0df833bfce86c308e777cec157b40b0c0de0` | Same starting commit as `refactor` | Published v0.2.0 baseline |
| `refactor` | `841b0df833bfce86c308e777cec157b40b0c0de0` | Clean before this documentation delivery | All redesign work |
| `update` | `66592cb15ddb21ba39573b0462c841ad3bd9d748` | 21 commits ahead of `main`, zero commits behind at assessment | Read-only design reference |

The `update` manifest declares package version `0.1.0`, while `main/refactor` declare `0.2.0`; package version alone cannot identify the newer architecture. Ref names can move later, so the full commit IDs above are the evidence anchors.

Inventory was obtained with `git ls-tree`, `git show`, and static source inspection. Line counts include blank/comment lines and count tracked `.rs` files. Test counts count literal `#[test]` attributes, not executed tests.

| Metric | `refactor` baseline | `update` reference |
| --- | ---: | ---: |
| Rust files | 11 | 30 |
| Rust source lines | 2,624 | 3,523 |
| `#[test]` attributes | 0 | 42 |
| Tracked files under `assets/` or `data/` | 4 | 19 |
| Tracked GeoJSON examples | 0 | 4 |
| Shapefile/GeoTIFF acceptance fixtures | None identified | None identified |

The four `update` examples are `data/test/china.geo.json`, `data/test/line.geojson`, `data/test/point.geojson`, and `data/test/polygon.geojson`. Their existence is verified; suitability, licensing, and expected outputs for the new acceptance suite are not verified. Other reference assets, including a font and an output PNG, are not automatically approved fixtures.

Useful read-only reproduction commands:

```powershell
git status --short --branch
git rev-parse main refactor update
git rev-list --left-right --count main...update
git ls-tree -r --name-only refactor
git ls-tree -r --name-only update
git show update:src/core/geojson.rs
git grep -n '#\[test\]' update -- '*.rs'
```

Local research used Windows x64 MSVC with Rust/Cargo 1.96.0. A previous `cargo check --offline --locked --lib` was blocked by uncached dependencies. This is neither a successful build nor evidence of compilation failure in the code. No new rendering, native PROJ, performance, or cross-platform execution is claimed by this documentation delivery.

## 2. Existing `refactor` prototype

Paths in this section refer to baseline commit `841b0df`. Prototype files removed
during M1 are linked to that immutable commit so historical evidence remains
readable after the refactor.

| Area | Evidence | Current responsibility |
| --- | --- | --- |
| Library entry | [src/lib.rs](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/src/lib.rs) | Mutable geometry/style input, Vello scene creation, GPU texture output, blocking GPU readback |
| Geometry | [src/rendered_geometry.rs](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/src/rendered_geometry.rs) | Original geometry plus cached line/area/anchor conversions and properties |
| Configuration | [src/render_option.rs](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/src/render_option.rs), [src/magic_value.rs](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/src/magic_value.rs) | RON-era options, dynamic values, fetching/conversion, viewport and output settings |
| Symbols | [src/renderer](https://github.com/DeadPoetSpoon/geello/tree/841b0df833bfce86c308e777cec157b40b0c0de0/src/renderer) | Point/line/area preparation and drawing coupled together |
| Coordinates | [src/utils.rs](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/src/utils.rs) | Hardcoded 4326/3857 transforms and tile bounds |
| Server | [src/server.rs](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/src/server.rs), [src/main.rs](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/src/main.rs) | Rocket routes, data/style loading, caches, GPU management, image encoding, WS loop |
| Dependencies | [Cargo.toml](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/Cargo.toml) | Old Vello 0.5.0/Rocket design; server feature; runtime coupled into library dependencies |

Existing capabilities are experimental GPU vector drawing, image output, WMS-like and WMTS-like endpoints, local/URL loading, and a message-driven WebSocket demo. They are not a complete standards implementation. Shapefile reading, GeoTIFF processing, lossless feature queries, a CPU backend, Unicode shaping, and the new snapshot lifecycle are absent.

### Rendering and geometry findings

- `line_renderer::to_shape` obtains the first vertex with `unwrap()` and closes the path. Open LineStrings therefore acquire closing-edge semantics, and empty input can panic.
- `RenderedGeometry::get_areas_from_geom` builds convex hulls for LineString, MultiPoint, and MultiLineString inputs. This changes geometry meaning instead of ignoring incompatible symbol components.
- Mutable geometry caches, resolved style values, fetching, and drawing share objects. A caller cannot treat prepared drawing as an immutable reusable product with separately accounted resources.
- Anchors may depend on clipped geometry or default fallbacks. The redesign requires anchors and path distances from the complete projected geometry.
- Scene construction and backend execution occur in the same call. There is no backend-independent scene contract or shared CPU/GPU layout.
- GPU readback uses a Tokio oneshot and `block_on_wgpu`; ownership, mapped-buffer cleanup, and early-return behavior need replacement with explicit resource and cancellation contracts. Texture-pool acquisition/release is manually coordinated and has no proven all-error-path release guarantee.

### Server and data findings

- `get_geom_from_geojson` converts sources into geometry vectors; the render path does not retain the full feature identity/property model needed for styling, animation, or WFS queries.
- Synchronous filesystem operations, GPU waits, renderer setup, and encoding run inside async request paths. `thread::sleep` is also imported/used by server orchestration. Async route syntax does not make this work nonblocking.
- Loading is performed while holding cache write access in some paths. Cache-miss handling lacks the intended per-identity singleflight and can serialize unrelated work or duplicate preparation.
- The disk tile path is derived from sanitized data/style strings plus z/x/y and a `.png` suffix. It lacks content versions, backend, full style/font/projection identity, and reliable collision isolation.
- Cache output is written without the new atomic publication and task deduplication contracts; concurrent work and partial writes are concerns, not measured failure-rate claims.
- Request values name paths/URLs, rather than published registry resources. The redesign removes this request-level source selection.
- `/ws/anim` reads each inbound message, changes point radii cumulatively, and then renders. It is not arbitrary-time evaluation, scheduled playback, or bounded latest-state delivery.
- `/map` serves files under the configured data location. It is not service discovery, API documentation, or a stable viewer product contract.
- No administration resource lifecycle exists: the prototype has no managed upload, external local-path registration, unpublish, record removal, content deletion, operation status, ownership model or atomic catalog generation.

## 3. `update` reference design

These paths exist on `update`, not in the current checkout: `src/core/{layer,geojson,lazy,projection,cache}.rs`, `src/core/style/`, `src/render/`, and `src/server/`.

Useful ideas are the move toward `core/render/server`, TOML, explicit layer IDs, error types, a separate render context, and more unit tests. They provide reference material, not an approved implementation to merge wholesale.

| Observation | Consequence for the redesign |
| --- | --- |
| `WorkspaceRegistry`, physical sources, and cache live in `core` | Do not copy them into the in-memory layer. Separate reader facilities from server publication/cache ownership. |
| `LazyValue` permits per-value file/URL loading | Resolve resource graphs before rendering; no arbitrary IO from style evaluation. |
| `convert_property` converts numbers to `f64` and arrays/objects to Null | Preserve signed/unsigned integers and nested values; distinguish missing and null. |
| Null geometry is converted to an empty GeometryCollection | Retain the distinction for metadata and protocol-specific query semantics. |
| CRS identifiers use `u16`; bare-feature parsing can bypass source override | Use a wider validated CRS representation and one consistent override/metadata/default policy. |
| Hash maps are used in registries/style-related structures | Use explicit ordered lists for drawing and stable ordinals; lookup containers cannot define drawing order. |
| `layout_glyphs` iterates `.chars()` and maps charmap entries to advances | Replace with shaping/bidi/fallback; this is not Unicode layout. |
| `/wmts/cache` returns a formatted configuration string | The Axum route is a placeholder, not a working WMTS implementation. |
| `/map` reads `index.html` | Replace the page convention with independent examples and proper protocol discovery. |
| Default feature enables `full`, including server and remote-source placeholders | Introduce the approved composable feature model and backend-free library build. |
| GeoTIFF configuration/dependency exists without full reader/rendering path | Dependency presence is not raster capability. Shapefile is also missing. |

The 42 test attributes are static evidence only. They do not establish coverage of the new feature/ID model, raster semantics, Unicode text, service standards, resource budgets, GPU behavior, or version lifecycle.

## 4. Build, deployment, and documentation findings

The baseline [Dockerfile](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/Dockerfile) installs the registry crate with `cargo install -F server geello` rather than building the checked-out redesign. It uses a moving Rust base and installs Vulkan support, but has no musl builder plus pinned Alpine runtime, no default-GPU/independent-CPU image variants, and no resource volume/staging contract. It cannot validate the new local source or the approved dependency graph.

The tracked [Docker publishing workflow](https://github.com/DeadPoetSpoon/geello/blob/841b0df833bfce86c308e777cec157b40b0c0de0/.github/workflows/docker-publish.yml) builds/publishes/signs an image on version tags. No general feature-matrix/test CI was identified. A publish workflow is not a substitute for Windows/Linux and real-GPU acceptance.

The existing root README describes experimental URLs, direct path parameters, old mutable APIs, and GPU deployment assumptions. This delivery adds a design entry point while retaining those prototype instructions as historical/current-code documentation. Migration guidance must later replace the old operational instructions when the new implementation exists.

The reference `doc/requirements.md` and `doc/plan.md` include completion checkmarks and approximate percentages. They also describe requirements now rejected or changed: core caches, generic LazyValue IO, remote/Samba sources, default full features, static viewer serving, and synchronous GPU wrappers. Do not carry their completion claims into this baseline.

## 5. Reuse and replacement boundary

Reuse concepts or narrowly verified utilities where they satisfy the new contract. Reimplement the mutable render pipeline, feature representation, protocol adaptation, source publication, caches, and animation lifecycle around the accepted architecture. There is no compatibility obligation to preserve old APIs, RON, or experimental URLs.

No wholesale merge from `update` is planned. Each implementation milestone must cite the new requirement and test evidence. Current-state findings are **Validated by static inspection** within the limits above; the target remains **not implemented**.
