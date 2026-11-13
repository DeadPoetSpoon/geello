# Requirements and acceptance

This is the complete first-stage target, not a description of existing functionality. Requirements below are **Accepted**, target runtime validation is **Pending**, and target implementation is **No** at the documentation baseline. [Decision IDs](05-decisions-and-dependencies.md), [milestones](06-implementation-roadmap.md), and scenario IDs form the traceability chain.

## 1. Scope and support matrix

| Capability | First-stage target | Explicit boundary |
| --- | --- | --- |
| Packaging | One crate, lib plus feature-gated server binary | No forced server runtime in the in-memory library |
| Rendering | Vello CPU and GPU, basic symbols, raster, Unicode labels | Normal source-over only; no group opacity, arbitrary blend/effect graph, or segmented oversized-scene execution |
| Vector input | Registered local GeoJSON and Shapefile, plus managed upload through the administration plane | No remote fetch, business database, or public FlatGeobuf source reader |
| Raster input | Registered local GeoTIFF; numeric memory rasters | No palette/CMYK/YCbCr/ICC conversion, complex samples, arbitrary packed bit depths, GCP/RPC fitting, or unsupported internal validity masks |
| Projection | Explicit planar/geographic mode, Rust backend, optional native PROJ | No vertical transform, automatic backend fallback, implicit grid downloads, densification, or automatic topology repair |
| Styles | Separate TOML resources and typed expressions | No per-value URL/file IO while rendering |
| Animation | Configured trajectories/transforms and basic symbol values | No vertex morphing, source-history interpolation, or text-layout parameter animation |
| Queries | Metadata, IDs, exact spatial match, declared scalar equality, pagination | No arbitrary CQL/FES, arbitrary sorting, edits, or public null/exists predicates |
| Services | OGC API Features/Maps/Tiles, WMS, WMTS, WFS query subset, WS | Claim only verified conformance classes; no upstream OGC consumption or full WFS 2.0 certification |
| Outputs | PNG/JPEG, library RGBA pixels and constrained GPU textures | No WebP or swapchain/BGRA/existing-target-load contract |
| Operations | Atomic refresh, bounded work/cache, diagnostics, proxy deployment | No built-in users/tenants/TLS/authentication; no server-hosted viewer convention |
| Platforms | Windows x64 MSVC and Linux x86_64 | Real DX12/Vulkan checks required; other platforms are not implied |
| Resource administration | Protected `/admin/v1` upload, local registration, unpublish, record removal and managed-content deletion | No built-in users/tenants/authentication; proxy protects the administration plane |
| Resource storage | Independent versioned files/TOML plus sharded catalog index and atomic generations | No recursive online directory scan, SQLite business catalog, TAR archive, or resumable upload protocol |
| Containers | Default GPU Alpine image and independent CPU Alpine variant, both built from musl | GPU/CPU images require separate verification; image labels do not imply successful hardware validation |

## 2. Traceability matrix

All rows use the baseline statuses stated above. A source-inspected limitation in the decision register does not make the corresponding runtime scenario pass.

| Requirement | Accepted behavior | Decisions | Milestone | Acceptance |
| --- | --- | --- | --- | --- |
| ARC-001 | One crate with separated lib/server and composable features | D-01, D-02 | M1 | T-01 |
| ARC-002 | Layer 1 has only in-memory processing; layer 2 owns readers/derived storage; server owns publication/cache | D-01 | M1–M3 | T-01, T-08 |
| ARC-003 | Immutable opaque scene; preparation distinct from execution; summaries expose accounting/diagnostics | D-03 | M1 | T-02 |
| ARC-004 | CPU/preparation synchronous; GPU async without required Tokio runtime | D-04 | M1 | T-03 |
| DAT-001 | Preserve nested properties, full integer types, source identity/ordinal, null/empty distinction, source Z/M | D-06 | M1–M2 | T-04 |
| DAT-002 | Typed effective IDs, uniqueness, reversible public IDs, explicit stable IDs for geometry animation | D-07 | M2 | T-05 |
| DAT-003 | Bounded independent batched cursors; no split feature; cooperative cancellation and typed terminal errors | D-08 | M1–M2 | T-06 |
| DAT-004 | Root-aware bounded GeoJSON parsing; explicit Shapefile encoding and supported shape variants | D-09 | M2 | T-07 |
| DAT-005 | Candidate bbox reads for rendering; exact feature intersection; whole original geometry in query output | D-10 | M2–M4 | T-08 |
| DAT-006 | Declared scalar queryables, strict typing, source-order pagination and exact bounded hits | D-10, D-11 | M3–M4 | T-09 |
| DAT-007 | Version-wide honest property schema; reversible XML names and schema mapping | D-11 | M2–M4 | T-10 |
| RAS-001 | Native numeric blocks, per-band types, common grid, affine rotation/shear and pixel interpretation | D-12 | M1–M2 | T-11 |
| RAS-002 | Supported tiled/striped, planar/interleaved, BigTIFF numeric decoding with bounded allocations | D-13 | M2 | T-12 |
| RAS-003 | Raw NoData before calibration; validity distinct from alpha; nonfinite diagnostics and precision contract | D-14 | M2 | T-13 |
| RAS-004 | Explicit continuous/category policy; nearest/bilinear/area rules and valid-weight normalization | D-15 | M2 | T-14 |
| RAS-005 | Explicit gray/RGB/RGBA mapping and global or explicit ranges; stable stretch across tiles | D-15 | M2 | T-15 |
| RAS-006 | Conservative overview selection and semantic validation; base-first publication | D-16 | M3 | T-16 |
| REN-001 | Stable item/rule/symbol/source order, symbol-major over all batches | D-17 | M1–M2 | T-17 |
| REN-002 | Geometry-compatible symbols, no forced line closure/hulls, even-odd polygons and explicit rings | D-18 | M1–M2 | T-18 |
| REN-003 | Full projected paths/anchors before final clipping; stable dashes, caps and outlines | D-18, D-19 | M1–M2 | T-19 |
| REN-004 | Explicit view mapping, stable pixel conversion, domain/antimeridian rules and conservative query extent | D-19, D-20 | M2 | T-20 |
| REN-005 | Source-over, independent symbol opacity, straight sRGB RGBA8 pixel result | D-05 | M1 | T-21 |
| REN-006 | CPU/GPU semantic parity and bounded dimensions; no backend substitution for external texture requests | D-03, D-04 | M1, M5 | T-22 |
| STY-001 | Versioned separate TOML graph, strict fields/refs, declaring-file-relative paths, no render-time IO | D-21 | M2–M3 | T-23 |
| STY-002 | Ordered rules, JSON Pointer filters with unknown semantics and typed explicit fallbacks | D-22 | M2 | T-24 |
| STY-003 | Explicit px/map units and agreed basic symbol defaults | D-20, D-23 | M2 | T-25 |
| TXT-001 | Shaping/bidi/fallback, explicit font bytes/faces and isolated stacks, explicit language | D-24 | M4 | T-26 |
| TXT-002 | Grapheme-safe wrap, geometry-compatible candidates, global collision pass and deterministic ties | D-25 | M4 | T-27 |
| TXT-003 | Curved text constraints and aligned buffered metatile layout | D-26 | M4 | T-28 |
| TXT-004 | Reproducible, licensed, renamed committed test-font subsets | D-27 | M4 | T-29 |
| ANI-001 | Independent arbitrary-time evaluation; source data and feature services unchanged | D-28 | M4 | T-30 |
| ANI-002 | Stable-ID trajectory and ordered whole-feature transforms before projection | D-29 | M4 | T-31 |
| ANI-003 | Step/linear interpolation, raw-angle rotation, replacing basic symbol values and opacity-only raster | D-30 | M4 | T-32 |
| ANI-004 | Seconds, linear-light premultiplied color interpolation, explicit binding time/repeat and refresh semantics | D-31 | M4 | T-33 |
| PRJ-001 | CRS override > metadata > format default; geographic memory needs CRS; explicit backend | D-32 | M2 | T-34 |
| PRJ-002 | Native PROJ opt-in with fixed versions, offline grids/db identity and no unapproved build fallback | D-33 | M5 | T-35 |
| STO-001 | Immutable sharded FGB plus lossless companion identity/property/ordinal records | D-34 | M3 | T-36 |
| STO-002 | Versioned lossless raster blocks, manifests/checksums and weighted overviews | D-16, D-35 | M3 | T-37 |
| STO-003 | Separate data/derived/render versions; atomic publication and reference-aware retirement | D-36 | M3 | T-38 |
| STO-004 | Moka/Foyer cache responsibilities, full identities, separate render/encode singleflight | D-37 | M3 | T-39 |
| API-001 | Registry IDs, independent dataset/map publication, separate OGC roots and declared discovery | D-38, D-39 | M3–M4 | T-40 |
| API-002 | WMS/WMTS normalization, supported grids, format/background/axis handling | D-40 | M3–M4 | T-41 |
| API-003 | Features Core query semantics, timeless datetime, GeoJSON output and fixed-snapshot links | D-10, D-41 | M4 | T-42 |
| API-004 | Explicit WFS 2.0 query subset, GET/XML POST, XSD/GeoJSON distinction | D-42 | M4 | T-43 |
| API-005 | WS hello/init/atomic updates/ack; distinct revisions/versions; ordered metadata/image pairs | D-43 | M4 | T-44 |
| API-006 | Monotonic playback, bounded latest-state backpressure, failure suspension and client-owned reconnect | D-44 | M4 | T-45 |
| API-007 | Encoded-byte ETags, revalidation, controlled diagnostics and protocol-specific errors | D-45 | M3–M4 | T-46 |
| OPS-001 | Global bounded queues/budgets, fair work classes, explicit cancellation and admission failures | D-46 | M1–M3 | T-47 |
| OPS-002 | Lazy source preparation; startup graph errors fail; refresh preserves previous complete version | D-36, D-47 | M3 | T-48 |
| OPS-003 | Absolute cursor expiry, GC without forced live eviction, derived-disk admission and unpublish lifecycle | D-48 | M3–M4 | T-49 |
| OPS-004 | Proxy authentication/TLS, explicit external base URL, controlled operational exposure | D-49 | M3, M5 | T-50 |
| OPS-005 | Windows/Linux feature acceptance, real GPU evidence and measured owner-approved profiles | D-50 | M5 | T-51, T-52 |
| DOC-001 | English docs/comments, traceable statuses, examples and breaking-change migration guidance | D-51 | M0–M5 | T-53 |
| DOC-002 | Exact approved dependencies; full resolved inventory approval and evidence-driven feasibility gates | D-52 | M0–M5 | T-54 |
| ADM-001 | Dataset, style, map, animation and font resources have managed or external ownership | D-53 | M2–M3 | T-55 |
| ADM-002 | Streamed single-file upload and restricted ZIP package upload with bounded archive safety checks | D-54 | M3 | T-56 |
| ADM-003 | External local paths can be registered without copying or taking ownership of source files | D-53 | M2–M3 | T-57 |
| ADM-004 | Unpublish, record removal and managed-content deletion have distinct non-cascading semantics | D-55 | M3 | T-58, T-59 |
| ADM-005 | Resource mutations use version/ETag optimistic concurrency and atomic publication | D-56 | M3 | T-61 |
| ADM-006 | Referenced resources block removal/deletion until dependent publications and snapshots are released | D-57 | M3 | T-60 |
| ADM-007 | Upload/validation/preparation returns an operation ID with bounded lifecycle status and diagnostics | D-54, D-56 | M3 | T-62 |
| ADM-008 | Independent resource files use sharded indexes and atomic catalog generations | D-58 | M3 | T-63, T-64 |
| OPS-006 | Default container is GPU Alpine, built with musl and containing CPU fallback support | D-59 | M5 | T-65 |
| OPS-007 | CPU Alpine is an independent `-cpu` image with CPU-only runtime and separate validation | D-60 | M5 | T-66 |
| OPS-008 | Resource catalog scale/recovery and managed storage are benchmarked without a fixed unverified capacity claim | D-58, D-61 | M3, M5 | T-64 |

## 3. Acceptance scenarios and required evidence

### Architecture and data

- **T-01 — Feature boundaries:** build/test the feature matrix on supported platforms. A backend-free, no-default library prepares scenes without a server runtime. Server builds require at least one renderer; unavailable readers produce explicit configuration errors. Inspect module/dependency direction for layer-1 IO/cache leakage.
- **T-02 — Scene ownership:** prepare once, release reader/source snapshots, execute repeatedly with the same view/time/resources. Verify immutable summary, stable ordering, resource retention and release, no deferred file reads, and explicit oversized-scene error.
- **T-03 — Execution/cancellation:** run CPU synchronously and GPU through a non-Tokio executor in the library harness. Check pre-submit failure leaves an external texture untouched; post-submit cancellation/fault makes it invalid rather than pretending rollback. Record retained memory at each boundary.
- **T-04 — Feature fidelity:** round-trip nested arrays/objects, null versus missing, i64/u64 extremes, finite float, source IDs, ordinals, empty/null geometry, Z/M and unknown trailing GeoJSON ordinates. Derived records must preserve supported source information without inventing ordinates after transforms.
- **T-05 — Identity:** distinguish string `"42"` from integer `42`; canonicalize nonnegative signed/unsigned numeric identity. Reject duplicates in strict mode; tolerant mode keeps the first source ordinal with diagnostics. Generated IDs change with DataVersion but not DerivedVersion. Verify reversible XML/URL-safe IDs and stable-ID animation rejection where necessary.
- **T-06 — Bounded cursors:** use independent cursors, retain prior batches, hit record/byte/single-feature limits, cancel between batches and during CPU work, and simulate truncation. Account all live batches; distinguish EOF from failure; terminate an unrecoverable cursor without guessed resynchronization.
- **T-07 — Reader correctness:** exercise GeoJSON root member reordering, bbox/extensions before `features`, escaped delimiters, nested properties, depth/coordinate limits and malformed records. Shapefile cases include Z/M variants, NullShape, sidecar mismatch, encoding precedence and undecodable DBF. MultiPatch must be rejected.
- **T-08 — Query semantics:** compare bbox candidates with exact intersections for holes, disjoint multiparts and boundaries; return full uncut features. Verify rendering remains correct when symbol expansion requires full scan. Layer 2 must not require server registry/cache objects.
- **T-09 — Typed filtering/pages:** test exact integer equality beyond 2^53, strict strings, finite floats, booleans, incompatible/null/missing values, AND/ID-OR combinations, explicit tolerant validation, exact bounded hits, deterministic ordinals and refresh during pagination. Never emit a partial successful page on budget failure.
- **T-10 — Schema:** infer optional/null/union properties across a full version; check declared queryable type failures. Encode illegal XML names and reserved-prefix collisions reversibly; retain original GeoJSON names. Verify XSD annotations and JSON Schema 2020-12 metadata describe their distinct encodings.

### Raster and rendering

- **T-11 — Raster model:** mixed numeric bands in memory, shared dimensions/grid, owned/shared/borrowed views, rotated/sheared affine transforms, edge/center conventions and PixelIsPoint/Area. Reject contradictory, incomplete or singular georeferencing; do not fit GCPs.
- **T-12 — TIFF formats/limits:** integer and float sample matrix, f16 promotion, strips/tiles, planar/interleaved, BigTIFF and multi-primary selection. Test actual decode allocations for a large strip. Reject unsupported photometry, bit depth, complex/mixed file sample layouts and internal masks. Validate WhiteIsZero raw recovery and associated alpha before claiming support. Record SubIFD capability independently; base access may bypass unvalidated overviews, never validity masks.
- **T-13 — Validity/precision:** apply raw NoData before scale/offset, selected-band validity, memory masks distinct from alpha, nonfinite processing invalidity and counters. Nearest raw sampling preserves integer types; f64 processing of i64/u64 beyond 2^53 follows the documented rounding warning contract.
- **T-14 — Resampling:** category-nearest, continuous enlargement-bilinear/downsampling-area, invalid combinations rejected, rotated grids and outside pixels. Valid weights normalize color; partial valid coverage alone does not reduce opacity. Verify premultiplied alpha processing occurs exactly once.
- **T-15 — Color ranges:** adjacent tiles use identical version-wide ranges; test explicit ranges, u8 color channels, constant-band midpoint with diagnostics, all-invalid transparent output, and unselected invalid bands. JPEG flattens once against the explicit final background.
- **T-16 — Overviews:** verify generation semantics, grid/bands/validity and conservative level choice under anisotropy/rotation. Compare weighted multilevel aggregation with a reference that avoids mean-of-means. Publication starts with base; complete overview upgrades change DerivedVersion and render identity, not feature identity.
- **T-17 — Ordering:** use line casings/interiors across multiple batches and parallel preparation, mixed map items and several matching rules. Output must be symbol-major and deterministic rather than batch- or hash-order dependent.
- **T-18 — Geometry:** open/empty/zero-length lines, point/multipoint, polygon holes/ring modes, multipolygons and recursive collections. No hull fills, artificial joins, implicit closure or topology repair; incompatible components produce the agreed diagnostics/statistics.
- **T-19 — Full geometry before clipping:** compare cropped full-view output with direct viewport render for dash phase, caps, polygon outlines and label anchors. Avoid artificial cut-edge strokes and clip-dependent centroids.
- **T-20 — Views/domains:** aspect-fit letterboxing versus service stretch, degenerate auto-fit requiring explicit span, large coordinates rebased before f32, conservative symbol bounds, wrapped bbox split/deduplication, domain clipping and out-of-range tile rejection. Do not assert shortest antimeridian edges or repeated worlds.
- **T-21 — Pixel contract:** source-over reference cases, color alpha times symbol opacity, overlapping features without group opacity, transparent output with RGB zero, top-left tight straight RGBA8 sRGB and single final JPEG flatten.
- **T-22 — CPU/GPU parity:** compare shared geometry/layout/ordering and tolerant pixel references on real Windows DX12 and Linux Vulkan devices. Reject dimensions exceeding u16, backend or budget limits including metatile buffer; reject wrong external texture size/format/usage/device contract.
- **T-23 — Configuration:** parse valid linked resources; reject unknown fields, absent required files, duplicate IDs and broken targets; verify declaring-file-relative paths and no rendering-time reads. Runtime-only configuration changes require restart; resource updates publish atomically.
- **T-24 — Filters:** JSON Pointer escapes, nested values, missing/null distinction, comparisons, membership, existence and three-valued boolean behavior. Final unknown does not match. Type errors require explicit fallback or an error, not implicit coercion.
- **T-25 — Units/symbols:** circle/square/triangle dimensions and centers, line cap/join/miter defaults, polygon rings, px/map unit conversion and geometric-mean scale under anisotropic views. Verify no geodesic ground-meter promise.

### Text, animation, and projection

- **T-26 — Shaping/fonts:** Latin ligatures, Arabic bidi/shaping, Indic clusters, CJK fallback and missing glyphs. Prove stack isolation, explicit face/weight/style, `und` language default and BCP47 overrides, no system-font scanning or synthetic style.
- **T-27 — Labels:** grapheme-safe long-word wrapping, polygon containment excluding holes, component candidates, point obstacles, priority and later-drawn tie-breaking; deterministic per-frame results. Numbers must not implicitly become text. Test explicit overlap permission.
- **T-28 — Curved/metatile:** repeat distances 150/300px, short-line midpoint, cluster bending/total-angle limits and upright layout; reject wrapping/newlines for curved labels. Test aligned 4x4+128px buffered layout and its documented cross-metatile consistency limit.
- **T-29 — Fonts as fixtures:** verify source versions/licenses/hashes, renamed subset names and retained shaping closures. Ordinary offline tests use committed subsets; regeneration requires approved FontTools/Python tooling, not a runtime dependency.
- **T-30 — Stateless time:** render times out of order, pause/seek/resume, skip frames and repeat an earlier time. Fixed resources/time/view must yield equivalent scene semantics; original feature services remain unmodified.
- **T-31 — Geometry animation:** stable-ID selection outside the viewport, trajectory distance/timestamps, ordered/repeated transforms, explicit anchors, source-plane interpolation and bounded feature counts. Refresh rebinds IDs/recomputes anchors without resetting time; missing IDs diagnose, incompatible coordinates reject publication.
- **T-32 — Tracks:** strictly increasing keyframe times, step/linear boundaries, replacing field-driven base values, same-target conflicts, finite/range/unit checks, 350-to-370-degree and multi-turn rotations. Reject text-layout targets and raster targets other than opacity.
- **T-33 — Animation time/color:** seconds with fractional values; pre-start base, once-final and binding loops; positive speeds. Check linear-light premultiplied interpolation (black/white midpoint about `#BCBCBC`; transparent red to opaque blue gives half-alpha blue), alpha-zero canonicalization and separate symbol opacity.
- **T-34 — CRS:** consistent explicit override across all root/source types, GeoJSON CRS84 default, missing CRS errors elsewhere, Rust backend selection, protocol axis normalization and no vertical transform. Publish only tested CRS/domain combinations; unsupported conversion paths fail explicitly.
- **T-35 — Native projection:** fixed dynamic PROJ/database/grid identities, no network, approved native build sources, missing-library failure without automatic bundled fallback, Windows/Linux loading and reference coordinate checks. Missing build tools or grids must be reported rather than silently changing the operation.

### Storage, protocols, and operations

- **T-36 — FGB:** Hilbert physical order versus restored source ordinal, shard memory bounds, typed-ID lookup, null/empty companion records, nested values, Z/M and mixed geometry fidelity. Reject corrupt FlatBuffers; rebuild incompatible derived versions without changing source identity.
- **T-37 — Raster store:** block endian/type/dimension/checksum validation, validity/weights and complete overview manifests; interrupted writes leave no published partial directory. Source files are never rewritten.
- **T-38 — Publication:** concurrent reads during successful/failed refresh, content/interpretation changes versus derived upgrades, multi-resource atomic manifest switch, old reference lifetimes and scene snapshot release. Cache entries must not accidentally pin full source snapshots.
- **T-39 — Cache/singleflight:** full identity collision checks, concurrent identical jobs, one metatile render with only requested tile encodes, actual backend separation, diagnostic retention and cache corruption/bypass. WS intermediate frames do not enter disk response cache.
- **T-40 — Discovery:** three independent roots, namespace IDs, dataset/map publication differences, collection/tileset/TMS links, actual conformance declarations and OpenAPI parameters/statuses. Metadata preparation must not invent values or silently omit a registered broken dataset.
- **T-41 — Maps/tiles:** WMS 1.3 axis cases, request-order multi-map layering with global labels/background, WMTS 1.0 grids/row direction/limits, matching OGC API map/tile output and legal empty 200 responses. Verify 256px WebMercatorQuad 1x1 and WorldCRS84Quad 2x1 at z0.
- **T-42 — Features:** CRS84 XY only, reversible IDs, bbox handling of null geometry required by Features Core, empty-geometry exclusion, valid timeless datetime matching and invalid datetime errors, unknown parameter rejection and fixed-snapshot pagination expiry/restart behavior.
- **T-43 — WFS subset:** capabilities/DescribeFeatureType/GetFeature GET and bounded namespace-aware XML POST, declared equality/BBOX/IDs and paging. GeoJSON-only output must be advertised honestly; XSD is not presented as validating GeoJSON or proving full WFS compliance.
- **T-44 — WS contract:** hello before init; grouped update atomicity; receive-order updates; unique frame sequence and revision/version distinction; ack before render outcome; exact metadata/image pairing with no intervening application message. WebSocket control ping/pong is not prohibited.
- **T-45 — WS lifecycle:** monotonic playing clock, frame drops without time slowdown, paused redraw on update/refresh, latest-state queues, render-error suspension/retry, complete-state reconnect to current versions, unpublish after a started frame pair and bounded slow-client handling.
- **T-46 — HTTP/errors:** ETag from actual encoded bytes, `Cache-Control: no-cache`, current-version verification before 304, RFC 9457 versus legacy XML errors, request correlation, incomplete-result summaries and cached diagnostic preservation. No immutable version URL promise.
- **T-47 — Admission/release:** concurrent HTTP/animation/background work with bounded memory/CPU/GPU queues, tile-throughput preference plus fairness, cancelled waiters, shared preparation continuation, submitted GPU work disposal and all error-path resource release. Third-party syscall/decode interruption is not promised.
- **T-48 — Startup/refresh:** syntax/graph/unknown-field errors fail startup; source prepare is lazy/deduplicated with brief wait and retryable preparing response. Refresh failures keep the prior full version and report status; all-waiter cancellation does not abort bounded source preparation.
- **T-49 — Retention:** absolute cursor lifetime without extension, page in flight completes, old snapshots retire only when uncurrent/unreferenced, no extra history, unpublish prevents new use but permits existing pages until original expiry. Full disk after GC rejects new preparation, cleans temporary data and preserves current/live versions.
- **T-50 — Deployment:** explicit external base URL including proxy prefix, no forwarded-header inference, no built-in identity-dependent response, proxy-controlled HTTP and WS access, no old static viewer route. Diagnostic examples must not disclose sensitive filesystem details.
- **T-51 — Platform evidence:** feature checks/CPU suites on Windows MSVC and Linux x86_64, separate real DX12/Vulkan tests, and explicit unverified status when hardware is unavailable. Optional native PROJ is separately tested.
- **T-52 — Performance:** fixed-seed generated offline datasets; cold/hot/refresh, tile throughput, latency percentiles, phase timing, RSS, GPU estimates and cancellation recovery. Record hardware/config/software/source seed; owner approval of formal profiles follows measurements. Synthetic results are not business-data guarantees.
- **T-53 — Documentation:** English docs/comments/examples, valid links, honest statuses, complete traceability, old-to-new API/config/URL migration guidance and clear unsupported capabilities. Documentation-only delivery must leave Rust/manifests/lockfiles unchanged.
- **T-54 — Dependency gates:** exact direct pins, resolved transitive/features/license/source/native inventory review, font inventory, no implicit upgrade/download/bundled build. A failed capability gate records evidence/options and returns to the owner for design resolution.
- **T-55 — Resource ownership and administration:** create managed dataset/style/map/animation/font records through `/admin/v1`, register an external local source, list/detail resource status, and verify public routes only see published generations.
- **T-56 — Upload safety:** stream a supported single file and a Shapefile ZIP through bounded staging. Reject absolute paths, traversal, symbolic links, nested archives, duplicate/invalid package members and request/expanded-size/file-count/compression-ratio violations.
- **T-57 — External registration:** register a local GeoJSON, Shapefile or GeoTIFF path without copying it; validate path/source interpretation and report unavailable or changed external content without claiming ownership.
- **T-58 — Unpublish and record removal:** unpublish blocks new discovery while preserving the record/content; remove a record does not delete managed or external bytes and leaves no published reference after dependent checks.
- **T-59 — Managed deletion:** delete managed content only with the current version/ETag; remove staged/derived references according to retention rules. Attempting to delete external content returns `409` and leaves its file untouched.
- **T-60 — Reference protection:** map/style/animation/font and in-flight snapshot references block removal/deletion with a bounded dependency list; no cascading unpublish or delete occurs.
- **T-61 — Atomic optimistic mutation:** concurrent replacement/removal with a stale version returns `409`; successful validation publishes a complete new generation, while failure retains the previous complete generation.
- **T-62 — Async operation lifecycle:** upload returns an operation ID and progresses through staging/preparing/ready or failed/unpublished; polling exposes bounded diagnostics, and public discovery changes only after ready publication.
- **T-63 — Index recovery:** restart reads catalog/generation indexes without recursive request-time scans; interrupted copy-on-write updates leave the prior generation usable and recovery identifies orphaned files without treating them as registered.
- **T-64 — Large catalog:** benchmark thousands of resource definitions, concurrent shard updates, lookup latency, restart, rebuild and orphan cleanup. Report measured limits rather than asserting a fixed maximum.
- **T-65 — Default GPU Alpine:** build the musl-based default GPU image, start it with mapped Vulkan device/loader, report GPU backend, exercise initialization-failure CPU fallback, mounted resource volumes and readiness while resources prepare.
- **T-66 — Independent CPU Alpine:** build and start the `-cpu` musl/Alpine variant without GPU/Vulkan runtime, verify CPU-only feature closure, resource administration and public rendering independently of GPU-image fallback tests.

No numerical throughput, latency, memory, queue, TTL, or cache-capacity acceptance threshold is invented here. Those values are a deliberate post-measurement approval gate. Correctness, boundedness, explicit failure, and evidence requirements apply before that gate.
