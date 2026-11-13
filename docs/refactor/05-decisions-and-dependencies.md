# Decisions, dependencies, and evidence gates

Baseline date: **2026-11-12**. This register preserves owner-approved decisions. It does not update Cargo, resolve a new lockfile, install native components, or prove runtime compatibility. See [requirements](02-requirements-and-acceptance.md) for the traceability matrix and [roadmap](06-implementation-roadmap.md) for gates.

Accepted revision dated **2026-11-13** adds D-53–D-61 and G-08–G-11 for administration, indexed resource storage and GPU-default/CPU-variant Alpine containers. D-09's former no-upload boundary is superseded by protected management uploads; public rendering requests still cannot select arbitrary paths/styles.

## 1. Decision register

Every `D-xx` entry below is **Accepted**. Target implementation is **No**; runtime validation is **Pending** unless a specific limited source-inspection result is listed later. Example-only schema choices are identified in the interface document and are not silently elevated to accepted new product behavior.

| Decision | Accepted choice and reason/boundary |
| --- | --- |
| D-01 | One crate, lib/server separation and two library layers. Memory processing has no paths/registry/cache/HTTP; layer 2 reads/derives; server publishes/orchestrates. Prevent prototype coupling from determining the redesign. |
| D-02 | Default CPU+GPU, composable readers/server, backend-free preparation library, server requires a renderer, server-full excludes native PROJ. Text belongs to core preparation. |
| D-03 | Opaque immutable backend-independent PreparedScene, stable summary and actual drawing resource ownership; release source snapshots; no lazy IO or oversized-scene segmented execution. |
| D-04 | Synchronous CPU/preparation; runtime-independent async GPU; compatible external wgpu context; constrained full-replace RGBA8Unorm target. Check u16/device/budget limits; post-submit failure cannot roll back. |
| D-05 | Normal source-over only, color alpha times individual symbol opacity, tight straight sRGB RGBA8 pixels with RGB zero at alpha zero. Explicit final JPEG background, no group opacity. |
| D-06 | Lossless typed property tree, XY spatial geometry, original identity/ordinal and source Z/M provenance; missing/null/empty distinctions. No vertical transform or made-up ordinates. |
| D-07 | Typed string/integer effective identity, numeric signed/unsigned canonical equality, reversible safe public encoding, strict uniqueness, explicit tolerant first-record policy, stable IDs for cross-refresh geometry animation. |
| D-08 | Bounded synchronous batched cursors, independent snapshot queries, retained batches accounted, single-record hard limit, terminal error distinct from EOF and cooperative cancellation. Library budget failure returns, not unbounded waiting. |
| D-09 | GeoJSON/Shapefile/GeoTIFF are the supported source formats, available through registered external paths or managed administration uploads. Root-aware bounded GeoJSON parsing; explicit DBF encoding; MultiPatch excluded. No generic remote fetch, upload of arbitrary executable content, or public FlatGeobuf source reader. |
| D-10 | Bbox candidates for rendering versus exact feature intersection; whole feature output, explicitly declared typed scalar equalities, AND selection with OR IDs, stable source-order pages and bounded exact hits. |
| D-11 | Honest version-wide schema inference or explicit schema; queryable types validated separately; optional/null/unions retained; reversible XML names, XSD mapping and separate JSON Schema 2020-12 metadata. |
| D-12 | Numeric contiguous/shared/borrowed memory raster bands, common grid, six-coefficient affine and PixelIsArea/Point; strict contradictory/missing georeference errors, no arbitrary strides/GCP fitting. |
| D-13 | Documented uniform GeoTIFF numeric layouts, explicit primary IFD, bounded tiles/strips/BigTIFF handling. Reject unsupported internal validity masks in phase 1, do not ignore them. Validate special layouts before support claims. |
| D-14 | Raw NoData before scale/offset; validity separate from alpha; selected bands determine validity; raw nonfinite retained but processing invalid; f64 rounding contract above 2^53. |
| D-15 | Explicit category/continuous semantics and resampling policy; valid-only normalization without opacity reduction for partial valid coverage; gray/RGB/RGBA global/explicit ranges; no per-tile stretch or advanced ramps. |
| D-16 | Validate source overview semantics; deterministic conservative level selection; generated weighted means retain weights; base-first publication then atomic DerivedVersion upgrade, no load-driven degradation. |
| D-17 | Explicit stable item/rule/symbol/source/component order; symbol-major across batches; parallel work cannot change order. |
| D-18 | Geometry-compatible drawing, no implicit LineString closure/MultiPoint joining/hulls; full projected paths/anchors before final clip; even-odd polygons and explicit ring strokes; no automatic topology repair. |
| D-19 | View-relative conversion before f32, no densification/simplification, conservative symbol query extent or full scan/error, explicit domain/antimeridian behavior and tested published CRS/domain combinations. |
| D-20 | Library aspect-fit, protocol stretch, explicit degenerate auto-fit span; px/map units, geometric-mean scalar scale under anisotropy, 0.28mm nominal pixels without local geodesic correction. |
| D-21 | Separate strict TOML graph, required references and declaring-file-relative paths, preparation-time resolution only; no LazyValue file/URL IO during render. |
| D-22 | All matching rules execute in order; JSON Pointer typed filter tree with unknown semantics; typed fallback is explicit, no implicit coercion. |
| D-23 | Circle/square/equilateral triangle, agreed basic dimensions/colors/line cap-join defaults; independent fill/stroke. Only hex sRGB colors and transparent default map. |
| D-24 | cosmic-text shaping/bidi/fallback, exact explicit font bytes/faces/style stacks, stack isolation, no system scan/synthetic style; language default und and explicit BCP47 override. |
| D-25 | Plain horizontal text with grapheme-safe wrap, global labels after content, point obstacles, deterministic priority/ties, true polygon containment and component candidates; temporal flicker permitted. |
| D-26 | Curved single-line labels, 150px first/300px repeat, short-line midpoint, 30-degree neighbor/90-degree total bend limits; aligned 4x4 metatiles plus 128px buffer, no cross-area exact-consistency promise. |
| D-27 | Approved Noto source versions and committed renamed subsets with source/license/hash manifests. FontTools 4.68.0 and Python >=3.11 are regeneration tooling only. |
| D-28 | Stateless arbitrary-time animation; geometry before projection; originals and Features/WFS unaffected. No accumulation, history interpolation or vertex morph. |
| D-29 | Stable-ID selection independent of viewport; source-plane trajectory and ordered whole-feature transforms with explicit following anchors; bounded counts/bytes and refresh rebind rules. |
| D-30 | Step/linear tracks, raw-angle rotation, replacing basic symbol size/angle/width/color/opacity; no animated text layout and only raster opacity. Validate units, ranges and target conflicts. |
| D-31 | Nonnegative finite seconds, positive speeds; linear-light premultiplied-alpha color interpolation; binding-owned start/speed/once-loop, pre-start base/end-final; no inferred map endpoint. |
| D-32 | Explicit planar/geographic mode and CRS; override > metadata > format default (GeoJSON CRS84). Rust projection default, native explicit; no automatic backend fallback/vertical conversion. |
| D-33 | Optional dynamically linked PROJ 9.6.2 in a fixed dependency directory, fixed approved native dependencies/db and per-needed-grid approval; no CURL/network or unauthorized bundled-source fallback. |
| D-34 | Immutable auto-budgeted FGB shards and lossless companions/ID locations/ordinals; physical Hilbert order not drawing order; verified FlatBuffers, no public FGB input extension. |
| D-35 | Internal versioned uncompressed raster block directory with type/endian/checksum/validity/weighted overviews; no TIFF writer or SQLite derived store. |
| D-36 | Distinct DataVersion/DerivedVersion/RenderRevision; content+interpretation fingerprints, temporary complete artifacts and atomic graph switch; prior complete version survives refresh failure. |
| D-37 | Moka sync objects and Foyer encoded responses/diagnostics, Psync shared default, no response recompression, full cache identity, separate area-render/tile-encode singleflight; WS avoids disk response cache. |
| D-38 | Registered namespace IDs, independent dataset/map publication, no implicit map from each dataset, explicit published variants and service flags. |
| D-39 | Separate Features/Maps/Tiles roots, JSON/GeoJSON discovery, OpenAPI 3.0 plus static HTML API docs, no data HTML class; declare only tested classes. |
| D-40 | WMS 1.3 multi-map request-order composition/global labels; WMTS 1.0 supported fixed 256px grids, 0–22 zoom may narrow; protocol axis normalization and explicit formats/background. |
| D-41 | Features Core 1.0.1 target: null bbox exception, timeless valid datetime matching, CRS84 XY output, unknown/invalid parameter errors; fixed-snapshot opaque server cursors with absolute expiry. |
| D-42 | WFS 2.0 query subset GET/KVP and POST/XML, GeoJSON only, explicit schema/namespace handling; no full WFS 2.0 claim because GML 3.2 default is mandatory there. |
| D-43 | One map per WS URL, hello/init/update/retry/ack/frame/event/error, complete atomic state groups and receive-order updates; metadata/raw-image pairs; revisions distinct from resource versions. |
| D-44 | Monotonic WS clock, drop frames not time, paused redraw, bounded latest-state backpressure, failure suspension/retry, full client-owned reconnect against current resources; 30fps default/60 configurable maximum, over-limit reject. |
| D-45 | Encoded-byte ETag/no-cache/current-snapshot revalidation; bounded preserved incomplete diagnostics; RFC 9457/legacy XML/WS error boundaries, no immutable version URL phase 1. |
| D-46 | Global bounded fair CPU/memory/GPU work classes; tile-throughput preference, no implicit quality loss; shared data prepare continues while cancelled render work stops cooperatively. |
| D-47 | Required server.toml/--config, small deployment env whitelist, startup graph failure versus lazy data prep; runtime config restart, resources hot update; no metadata guessing. |
| D-48 | Reference-aware GC, no extra history or forced live-cursor eviction, absolute cursor TTL; disk-full rejects replacement; unpublish allows in-flight/pages, WS finishes pair then event/close. |
| D-49 | Reverse proxy owns TLS/auth; no users/tenants; explicit public_base_url without forwarded-header inference; independent demos replace built-in viewer. |
| D-50 | Windows MSVC/Linux x86_64 acceptance, real DX12/Vulkan evidence; fixed-seed synthetic offline measurements then owner-approved profiles/performance targets, not arbitrary hard numbers. |
| D-51 | English source comments/docs/examples, six-document baseline and traceability, separate accepted/validated/implemented status, breaking-change migration guidance. |
| D-52 | Every dependency/version needs approval; direct versions remain exact, full resolved/native/source/license inventory requires later approval. Failed capability experiments return to design with evidence/options, no silent scope reduction. |
| D-53 | Resource administration is a protected `/admin/v1` control plane. Dataset/style/map/animation/font records explicitly distinguish `managed` uploaded content from `external` local-path registration. The proxy supplies access control; the server does not add users/tenants in phase 1. |
| D-54 | Uploads are streamed single requests. Supported single files and restricted ZIP Shapefile packages use bounded staging; absolute paths, traversal, symbolic links, nested archives and excessive expansion are rejected. No resumable chunks or TAR in phase 1. |
| D-55 | Unpublish, record removal and managed-content deletion are separate, non-cascading operations. Record removal does not delete bytes; external content cannot be deleted through Geello. |
| D-56 | Resource mutations use version/ETag optimistic concurrency and asynchronous operations. Only a validated `ready` operation changes the catalog generation; failed replacement retains the previous complete generation. |
| D-57 | Referenced resources and in-flight snapshots block removal/deletion with bounded dependency diagnostics. Callers must resolve dependents first; no automatic cascading unpublish/delete. |
| D-58 | Resource bodies remain independent TOML/files or version directories, while type/namespace-sharded indexes and atomic copy-on-write generations provide online lookup. A single large TOML and unindexed directory scans are rejected for large catalogs; SQLite business catalog, embedded KV and external services are deferred. |
| D-59 | The ordinary image tag is the default GPU Alpine variant built from a musl stage, with Vulkan runtime/device mapping and the CPU backend available for initialization fallback. Actual backend is reported and cached distinctly. |
| D-60 | CPU Alpine is an independent `-cpu` image built from a CPU-only feature set, without GPU renderer or Vulkan runtime. It has independent build/startup/functional/performance verification. |
| D-61 | Resource count, archive, image, storage and runtime limits are measured operational profiles, not invented fixed capacity claims. Large-catalog recovery and orphan reconciliation are explicit acceptance work. |

### Cross-cutting renderer initialization clarification

Default compiled CPU+GPU support is distinct from server execution preference. Server default is **GPU-preferred with CPU fallback only when GPU initialization fails**. Log/report actual backend and include it in response/cache identity. GPU runtime faults suspend/fail affected work; do not silently rerender with CPU. An explicit reinitialization/restart can establish a new execution context.

## 2. Approved direct Rust versions

These exact version strings are owner-approved. The implementation should use exact Cargo requirements (for example `=0.4.0`), not broad caret upgrades. TOML build metadata in a version string still needs the resolved package/checksum in the lockfile; it is not a substitute for lockfile pinning.

Each package link leads to the exact crates.io release, which identifies its published repository and distribution. Licenses below were read from release metadata on the baseline date. They are **declared licenses**, not a completed legal/source-file audit. The full transitive inventory remains pending. The project itself is MIT.

| Package / exact version | Purpose / intended feature discipline | Published license |
| --- | --- | --- |
| [vello_gpu 0.4.0](https://crates.io/crates/vello_gpu/0.4.0) | GPU execution; no defaults; std, wgpu, text, full_image_support; do not enable wgpu_default/all_shader_features indiscriminately | Apache-2.0 OR MIT |
| [vello_cpu 0.4.0](https://crates.io/crates/vello_cpu/0.4.0) | CPU execution; no defaults; std, text, u8_pipeline, multithreading; encoding handled separately | Apache-2.0 OR MIT |
| [vello_common 0.4.0](https://crates.io/crates/vello_common/0.4.0) | Shared scene/color/path adapter types; no defaults, std | Apache-2.0 OR MIT |
| [wgpu 30.0.1](https://crates.io/crates/wgpu/30.0.1) | Explicit GPU API; no defaults; std, wgsl, dx12, vulkan for target platforms | MIT OR Apache-2.0 |
| [geo 0.33.1](https://crates.io/crates/geo/0.33.1) | Geometry algorithms; do not accidentally enable native PROJ by default | MIT OR Apache-2.0 |
| [geo-types 0.7.20](https://crates.io/crates/geo-types/0.7.20) | Public XY memory geometry; compatible serde/parallel support as required by audited feature graph | MIT OR Apache-2.0 |
| [serde 1.0.229](https://crates.io/crates/serde/1.0.229) | Typed model/config serialization; derive | MIT OR Apache-2.0 |
| [serde_json 1.0.154](https://crates.io/crates/serde_json/1.0.154) | Bounded root-aware GeoJSON and JSON protocols; preserve integer distinctions | MIT OR Apache-2.0 |
| [toml 1.2.1+spec-1.1.0](https://crates.io/crates/toml/1.2.1%2Bspec-1.1.0) | TOML resources; parse/serialization features audited before resolution | MIT OR Apache-2.0 |
| [thiserror 2.0.21](https://crates.io/crates/thiserror/2.0.21) | Typed error implementation | MIT OR Apache-2.0 |
| [rayon 1.12.0](https://crates.io/crates/rayon/1.12.0) | Bounded CPU parallel execution within caller/server accounting | MIT OR Apache-2.0 |
| [cosmic-text 0.19.0](https://crates.io/crates/cosmic-text/0.19.0) | Shaping/bidi/fallback; no defaults, std; no swash/editor/fontconfig/system scan | MIT OR Apache-2.0 |
| [geojson 1.0.0](https://crates.io/crates/geojson/1.0.0) | GeoJSON types/conversions; do not use first-array deprecated stream shortcut | MIT/Apache-2.0 (published expression) |
| [shapefile 0.9.0](https://crates.io/crates/shapefile/0.9.0) | Reader; geo-types integration and encoding_rs-backed DBF decoding including GBK | MIT |
| [tiff 0.11.3](https://crates.io/crates/tiff/0.11.3) | Rust business-image decoder; deflate/fax/jpeg/lzw; no zstd/webp | MIT |
| [proj4rs 0.3.0](https://crates.io/crates/proj4rs/0.3.0) | Default Rust projection backend | MIT OR Apache-2.0 |
| [proj4wkt 0.2.0](https://crates.io/crates/proj4wkt/0.2.0) | WKT conversion for supported Rust operations | MIT OR Apache-2.0 |
| [crs-definitions 0.6.0](https://crates.io/crates/crs-definitions/0.6.0) | Versioned CRS definitions | CC0-1.0 |
| [proj 0.31.0](https://crates.io/crates/proj/0.31.0) | Optional explicit native projection; no bundled/network defaults | MIT OR Apache-2.0 |
| [flatgeobuf 6.0.1](https://crates.io/crates/flatgeobuf/6.0.1) | Derived vector shards; default-features=false, no HTTP/TLS | BSD-2-Clause |
| [axum 0.8.9](https://crates.io/crates/axum/0.8.9) | HTTP/WS server; http1/json/query/ws/tracing and bounded raw-body streaming for administration uploads; multipart/form/http2/TLS features not selected | MIT |
| [tokio 1.54.1](https://crates.io/crates/tokio/1.54.1) | Server runtime only; rt-multi-thread/net/sync/time/signal/macros as needed | MIT |
| [tower 0.5.3](https://crates.io/crates/tower/0.5.3) | Service utilities; util, no blanket full | MIT |
| [tower-http 0.7.1](https://crates.io/crates/tower-http/0.7.1) | trace/cors/limit; avoid unreviewed request-id UUID expansion | MIT |
| [tokio-util 0.7.20](https://crates.io/crates/tokio-util/0.7.20) | Server cancellation/runtime adapters; audit minimal required features | MIT |
| [futures-util 0.3.34](https://crates.io/crates/futures-util/0.3.34) | Stream/Sink orchestration, including WS; minimal std/async/sink feature selection | MIT OR Apache-2.0 |
| [tracing 0.1.44](https://crates.io/crates/tracing/0.1.44) | Structured diagnostics/spans | MIT |
| [tracing-subscriber 0.3.23](https://crates.io/crates/tracing-subscriber/0.3.23) | Server text/JSON logs and filtering; audit fmt/json/env-filter graph | MIT |
| [image 0.25.10](https://crates.io/crates/image/0.25.10) | PNG/JPEG encoding, default-features=false, png/jpeg only | MIT OR Apache-2.0 |
| [quick-xml 0.42.0](https://crates.io/crates/quick-xml/0.42.0) | Bounded namespace-aware legacy XML, no generic XML IO resolution | MIT |
| [notify 8.2.2](https://crates.io/crates/notify/8.2.2) | Windows/Linux resource watching; default-features=false | CC0-1.0 |
| [moka 0.12.16](https://crates.io/crates/moka/0.12.16) | Sync object cache, sync feature; no unnecessary async cache | (MIT OR Apache-2.0) AND Apache-2.0 |
| [rstar 0.14.0](https://crates.io/crates/rstar/0.14.0) | Bbox candidate indexes with own wrapper; avoid introducing a second geo-types contract | MIT OR Apache-2.0 |
| [blake3 1.8.7](https://crates.io/crates/blake3/1.8.7) | Content/identity hashing, pure implementation; build-dependency inventory still required | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception |
| [tempfile 3.27.0](https://crates.io/crates/tempfile/3.27.0) | Bounded staging and atomic publish support | MIT OR Apache-2.0 |
| [foyer 0.22.6](https://crates.io/crates/foyer/0.22.6) | Encoded response hybrid cache, runtime-tokio, Psync, no response recompression | Apache-2.0 |

Feature descriptions are an intended minimal graph, not a finalized manifest. Inspect exact optional/implicit features and unified transitive features before adoption. `cosmic-text/std` can pull locale support transitively; application language must still be explicitly `und`/configured and no system-font scan may occur. Transitive availability does not authorize exposing a capability.

Use Vello/FlatGeobuf reexports where appropriate rather than adding unapproved direct peniko/kurbo/geozero/skrifa/harfrust dependencies. Do not force all versions of shared font crates to unify: cosmic-text's harfrust/fontdb/skrifa chain and Vello/glifo's newer skrifa/peniko chain can legitimately coexist. Adapters must bridge actual glyph IDs/fonts correctly. Archived rustybuzz is not an approved replacement.

Not approved for the redesign merely because present in old code or convenient: Rocket, rocket_ws, RON, figment, reqwest, GDAL, SQLite business storage/rusqlite, a frontend package or a new datetime crate. Existing package approvals also do not preapprove arbitrary extras or HTTP/TLS feature graphs.

Restricted ZIP handling needs a separately proposed exact archive dependency/codec feature set and its full normal/build/native closure. No ZIP crate is selected or approved here. Raw-body single-request upload does not require enabling Axum multipart by implication. Base Rust/Alpine image versions/digests, APK Vulkan/driver packages, musl C/C++ build tools and Foyer codec linking also join G-01/G-10/G-11. Both container variants build checked-out locked source, never a registry `cargo install`. Keep native PROJ excluded by default. A GPU Alpine build must prove vendor loader/driver ABI compatibility, not just musl binary compilation.

## 3. Selected transitive/native components

The following candidate chains/versions were explicitly accepted during design, but are not a complete resolved inventory:

| Component | Accepted version | Role / evidence boundary |
| --- | --- | --- |
| `lz4` | 1.28.1 | Foyer storage transitive native-codec wrapper |
| `lz4-sys` | 1.11.1+lz4-1.10.0 | Bundled native LZ4 1.10.0; include build tools/source/license in inventory |
| Native LZ4 | 1.10.0 | Built even though encoded responses are not recompressed |
| `zstd` | 0.13.4 | Compatible Foyer `^0.13` chain; do not force unrelated latest 0.14.3 |
| `zstd-safe` | 7.4.0 | Accepted candidate wrapper chain |
| `zstd-sys` | 2.1.2+zstd.1.5.7 | Bundled native Zstd 1.5.7 |
| Native Zstd | 1.5.7 | Native build/source/license inventory required |
| `proj-sys` | 0.27.0, inspected candidate | Native PROJ wrapper/build-script evidence; exact resolved closure still needs approval |

Release sources: [lz4](https://crates.io/crates/lz4/1.28.1), [lz4-sys](https://crates.io/crates/lz4-sys/1.11.1%2Blz4-1.10.0), [zstd](https://crates.io/crates/zstd/0.13.4), [zstd-safe](https://crates.io/crates/zstd-safe/7.4.0), [zstd-sys](https://crates.io/crates/zstd-sys/2.1.2%2Bzstd.1.5.7), [proj-sys](https://crates.io/crates/proj-sys/0.27.0). Preserve each release's own license expression and bundled notices in the eventual resolved inventory; wrapper license alone does not describe embedded C sources.

### Optional native PROJ distribution

| Native component | Exact approved version | Source of truth | License/inventory action |
| --- | --- | --- | --- |
| PROJ | **9.6.2** | [OSGeo PROJ 9.6.2](https://github.com/OSGeo/PROJ/releases/tag/9.6.2) | Capture release COPYING/source hash; expected MIT-style terms require actual notice retention |
| SQLite | **3.54.1** | [SQLite source/downloads](https://www.sqlite.org/download.html) | Record exact archive/source ID/hash; retain public-domain dedication and any distributed notices |
| libtiff | **4.7.2** | [libtiff releases](https://libtiff.gitlab.io/libtiff/releases.html) | Capture release's complete copyright/license notices and hash, not a guessed single wrapper license |
| zlib | **1.4** | [zlib official source](https://zlib.net/) | Capture exact archive/hash and zlib license notice |
| proj.db | Built from approved PROJ 9.6.2 source | Same PROJ build | Record database hash/schema and generation inputs; not an independently moving system database |
| Needed grid files | Per actual CRS requirement | [PROJ data distribution](https://cdn.proj.org/) | Each file needs version/hash/source/license approval; no blanket PROJ-data download approval |

These are approvals of requested versions, not claims that their archives have been downloaded, hashed, built together, or verified on either platform. Confirm availability and compile compatibility at G-05; discrepancies return to design without substituting a newer/older release.

Use a fixed dependency directory and dynamic linking. Rust TIFF decodes application imagery; native libtiff is only a PROJ grid dependency. SQLite serves proj.db only. Disable CURL/implicit network. Keep only needed libtiff grid codecs (deflate/internal) and disable other external codec dependencies unless explicitly approved.

`proj-sys 0.27.0` source inspection shows minimum PROJ 9.6.2 and source-build fallback if pkg-config detection fails, even without deliberately choosing a bundled build flag. Therefore “disable bundled feature” alone does not satisfy the contract. Before enabling native-proj, define/test an enforced build preflight/environment that resolves the exact approved library/database and fails before any unauthorized fallback. The exact enforcement mechanism is a G-05 output, not already implemented behavior.

Research found pkg-config under the local MSYS2 installation but no discoverable PROJ package. CMake/compiler commands were not in the inspected PATH; that does not establish that the MSVC toolchain is absent. Do not turn this environment observation into a cross-platform feasibility result.

## 4. Approved text fixture sources/tooling

| Resource | Exact version | Approved source |
| --- | --- | --- |
| Noto Sans | 2.015 | [NotoSans-v2.015](https://github.com/notofonts/latin-greek-cyrillic/releases/tag/NotoSans-v2.015) |
| Noto Sans CJK SC | 2.004 | [Sans2.004](https://github.com/notofonts/noto-cjk/releases/tag/Sans2.004) |
| Noto Naskh Arabic | 2.021 | [NotoNaskhArabic-v2.021](https://github.com/notofonts/arabic/releases/tag/NotoNaskhArabic-v2.021) |
| Noto Sans Devanagari | 2.007 | [NotoSansDevanagari-v2.007](https://github.com/notofonts/devanagari/releases/tag/NotoSansDevanagari-v2.007) |
| FontTools | **4.68.0**, no extras | [FontTools 4.68.0](https://pypi.org/project/fonttools/4.68.0/) |
| Python | >=3.11 for regeneration | [Python](https://www.python.org/) |

Font sources are SIL OFL 1.1. Choose actual static source files and record face/name/source SHA-256, source license and retained notices. Rename modified subsets with `Geello Test ...` family/style names as required for the chosen assets, retain shaping GSUB/GPOS closures, and commit compact subsets plus a reproducible maintenance recipe. Do not use an archived convenience repository as a different version source. Exact asset hashes are pending selection/download, not fabricated here.

FontTools is maintenance-only, with its actual release license/source recorded when tooling is installed. Ordinary runtime and tests must not require FontTools, Python font generation, network access or system fonts. Test fixtures do not grant approval to redistribute arbitrary application fonts or the old reference font.

## 5. Evidence ledger

| Evidence | Method and result | Claim limits |
| --- | --- | --- |
| E-01 Branch/code inventory | Git/ref/source inspection at the commits in [current state](01-current-state.md) | Validates recorded structure/counts, not behavior or test success |
| E-02 Approved release metadata | Read exact direct-package crates.io release metadata on 2026-11-12; recorded versions, published repositories and declared licenses | Does not resolve a lockfile, verify all embedded notices, or prove compatibility |
| E-03 Vello 0.4 source | Inspected CPU/GPU/common source: u16 dimensions, target configuration, text/shared types and backend surfaces | Adapters, pixel conversion, external target lifecycle and real GPU parity still need runtime validation |
| E-04 GeoJSON 1.0.0 stream source | Deprecated feature iterator identifies first array before features, unsafe as a general root-aware parser | Requires new bounded root parser; no reader implemented yet |
| E-05 TIFF 0.11.3 source | TransparencyMask/RGBPalette colortype rejected; WhiteIsZero normalization and ExtraSamples handling inspected; high-level image seeking follows main IFD chain | Internal masks explicitly excluded. WhiteIsZero raw recovery/associated alpha/SubIFD decoding need separate tests; do not infer support from tag inspection |
| E-06 FlatGeobuf 6.0.1 source | Indexed writing sorts by Hilbert order; offset/index state is O(N); reexported geozero available | Requires budgeted shards and companion/order restoration experiments; no full implementation proof |
| E-07 proj-sys 0.27.0 build script | Minimum 9.6.2 and pkg-config failure fallback inspected | Need enforced fixed native resolution and full Windows/Linux build evidence |
| E-08 Axum 0.8.9 WS source | Text/Binary/Sink/Stream and explicit size limit APIs inspected; transport Ping/Pong supported | Application frame pairing/backpressure/state machine still needs implementation tests |
| E-09 Standards inspection | Features null bbox/timeless datetime rules and WFS GML default obligation checked | Actual conformance classes/routes/error details require contract tests; no official certification claimed |
| E-10 Offline baseline build attempt | Existing offline locked check could not obtain uncached packages | Neither a successful build nor a code compile failure; not repeated as part of docs delivery |

Package archives can be inspected without changing the repository via `https://static.crates.io/crates/{name}/{name}-{version}.crate`. The final inventory must preserve actual checksums and provenance; a mutable repository branch URL alone is insufficient.

Useful specification references:

- [OGC API Features Part 1 Core 1.0.1](https://docs.ogc.org/is/17-069r3/17-069r3.html): bbox/null requirement 23C, datetime/timeless matching, discovery, unknown parameters and separate HTML/GeoJSON/OpenAPI classes.
- [WFS 2.0](https://docs.ogc.org/is/09-025r2/09-025r2.html): GML default obligation, feature types/schema and query/paging contracts.
- [GeoTIFF 1.1](https://docs.ogc.org/is/19-008r4/19-008r4.html): raster space, georeferencing and CRS metadata.
- [OGC API Maps reference OpenAPI](https://github.com/opengeospatial/ogcapi-maps/blob/master/openapi/ogcapi-maps-1.yaml) and [OGC API Tiles reference OpenAPI](https://github.com/opengeospatial/ogcapi-tiles/blob/master/openapi/ogcapi-tiles-1.yaml): candidate route/parameter references, not instructions to claim every listed extension.
- [RFC 9457](https://www.rfc-editor.org/rfc/rfc9457.html), [RFC 7946](https://www.rfc-editor.org/rfc/rfc7946.html), [JSON Schema 2020-12](https://json-schema.org/draft/2020-12).

Freeze exact specification revisions and relevant clauses in implementation evidence. Moving upstream OpenAPI examples are research references, not an immutable acceptance artifact.

## 6. Remaining gates and decision handling

| Gate | Required output | Owner/acceptance rule |
| --- | --- | --- |
| G-01 Dependency closure | Exact manifest features, resolved Cargo.lock, all normal/build/target dependencies and checksums, repositories, licenses/notices, native tool/runtime inputs and platform differences | Owner approves complete baseline before dependency adoption; no new direct/helper dependency inferred from existing approvals |
| G-02 Rendering | CPU/GPU adapter prototype, runtime-independent async harness, pixel/target/cancel/size evidence | Preserve all accepted execution requirements; report platform/device gaps |
| G-03 Reader/storage | Bounded GeoJSON, Shapefile fidelity, TIFF decode/georeference/alpha/layout matrix, FGB companions/shards and raster-overview semantics | Unsupported internal mask rejection is already an accepted exclusion; unvalidated source overviews can fall back to base. Other capability failures require a decision |
| G-04 Text | Stack isolation, shaping/bidi/fallback/curved layout and reproducible approved font subsets | No silent system-font fallback or replacement engine |
| G-05 Native PROJ | Fixed-version source/hash/build inventory, dynamic deployment, offline db/grids, prevention of bundled fallback, Windows/Linux compile and coordinates | Optional feature remains incomplete until validated; never substitute native versions or download grids automatically |
| G-06 Protocol contract | Exact schemas/parameter rules/error and close codes, actual conformance URIs, OpenAPI and discovery fixtures | Mechanical schema details must obey accepted semantics; new behavior/scope needs owner decision |
| G-07 Performance/profile | Measured cold/hot/refresh traces and resource usage on reproducible generated workloads; proposed development/small profiles and targets | Owner confirms formal numbers after baseline measurement; no performance-complete claim before that |
| G-08 Resource lifecycle/archive | Resource ownership, restricted ZIP safety, stream limits, reference protection, optimistic conflicts and atomic operation publication | Add any archive/stream helper dependency to G-01; do not silently accept unsafe extraction or ownership ambiguity |
| G-09 Resource catalog scale | Thousands of resource definitions, shard lookup/update/rebuild, restart recovery, orphan reporting and concurrent generation updates | Report measured scale/latency; do not claim a fixed maximum without evidence |
| G-10 GPU Alpine image | Musl-built default GPU Alpine image, Vulkan loader/device mapping, startup/readiness, GPU initialization fallback and volume recovery | Ordinary image tag denotes target variant only; hardware evidence and pinned base digest remain required |
| G-11 CPU Alpine image | Independent CPU-only Alpine image without GPU/Vulkan runtime, feature closure, startup/resource/API behavior and performance | CPU success cannot substitute for GPU validation or prove GPU-image fallback |

For any failure, record requirement/scenario, exact dependency/features/platform/input, observed result, violated assumption and options. Stop calling the affected requirement complete. Continue independent work only where it does not assume the failed capability. Return the evidence/options to the owner; do not silently drop a feature, change a version/backend, ignore validity, or lower quality.

This register is the starting inventory. Subsequent approval records should identify exact artifacts/checksums and state which pending gates they close. Accepted, validated and implemented remain separate throughout.
