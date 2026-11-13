# Implementation roadmap and verification

This roadmap sequences the **whole first-stage refactor**. M1–M5 are milestones inside that stage, not permission to omit its later capabilities. All implementation work remains on `refactor`; reference branches stay unchanged. Breaking compatibility is allowed with documented migration.

M0 documentation is complete. M1 memory-core and basic execution work is now implemented on `refactor`; later readers, server, protocol and deployment milestones remain pending. See [requirements](02-requirements-and-acceptance.md), [architecture](03-architecture-and-data-flow.md), [interfaces](04-public-apis-and-configuration.md) and [gates](05-decisions-and-dependencies.md).

## 1. Common work and completion rules

Each implementation change must name requirement IDs and corresponding T-scenarios, describe behavior changes, include relevant English API documentation/comments, and report actual verification evidence. Comments should explain invariants, units, ordering, ownership and failure semantics rather than repeat syntax.

Before dependency adoption, close G-01 for the intended feature/platform closure. The owner-approved direct versions are inputs, not permission to accept any resolver output or native build. Newly needed crates, tools, assets, feature expansions or native libraries require the agreed approval path.

Feasibility experiments are narrow implementation tasks tied to G-02 through G-06 and G-08 through G-11. Avoid building broad subsystems on unverified assumptions. A failed experiment returns evidence/options to design; independent work can continue, but the affected requirement remains incomplete. Missing GPU hardware is an evidence gap, never a passing test.

Use meaningful behavior/contract tests and reproducible inputs. Do not create tests merely to mirror a straightforward implementation. After relevant checks pass, broaden only for a concrete unresolved concern. Record failed/unavailable checks distinctly from passing results.

## 2. Milestones

### M1 implementation evidence (refactor)

The M1 memory core is implemented with typed feature identities/properties, native raster bands and grids, explicit CRS/view validation, resolved point/line/polygon/image styles, immutable prepared command scenes, accounting reservations, cooperative cancellation/deadlines, and synchronous CPU plus runtime-independent asynchronous GPU adapters. The old mutable prototype sources were removed.

Verified on the current Windows MSVC toolchain:

- `cargo check --no-default-features --lib` (backend-free memory layer);
- `cargo check --no-default-features --features render-cpu --lib`;
- `cargo check --no-default-features --features render-gpu --lib`;
- `cargo test --all-features --lib --tests`: 17 memory/CPU contract tests pass; the real GPU test is an explicit ignored test;
- `cargo test --test gpu_contracts -- --ignored --nocapture`: NVIDIA GeForce RTX 4090, DX12, driver `32.0.16.1088`; CPU/GPU parity max channel delta 1, mean delta `0.005859375`, and target/cancellation checks pass.

These results implement the memory-only portions of ARC-001–004, DAT-001/003, RAS-001, REN-001–003/005–006 and T-01–T-06/T-17–T-22. Reader, projection, complete image processing, text, and production resource/runtime work remain pending in M2–M5. The GPU test is platform evidence for this workstation, not a cross-platform completion claim.

### M0 — Documentation baseline

**Prerequisites:** completed architecture discussion and read-only assessment of `refactor`/`update`.

**Deliverables:** six English documents, navigation/status definitions, requirement/decision/milestone/test traceability, inline illustrative configuration/API/protocol examples, protected resource lifecycle and storage/index contracts, default GPU Alpine and independent CPU Alpine container contracts, exact approved-version register and README entry point. Label proposed syntax and pending gates explicitly.

**Verification:** local Markdown paths/anchors and fence structure; TOML/JSON syntax using available tooling without adding packages; cross-reference coverage; branch/count/evidence checks; Git diff/whitespace; changed-file allowlist. Parseability is not application-schema validation. Verify no Rust/Cargo/lock/runtime/CI files changed. Do not auto-commit.

**Exit:** all documents present, readable and consistent, documented limits honest, only documentation changes, verification results reported. Runtime APIs/configuration remain clearly unimplemented. M0 completion does not close any runtime acceptance scenario merely by describing it.

### M1 — Memory core and basic execution

**Prerequisites:** M0 and G-01 for core/render/test closure; approved experimental resource limits clearly labeled as temporary.

**Deliverables:** typed memory feature/ID/property/raster models; errors, accounting, cooperative cancellation/deadlines; explicit CRS/view model; immutable opaque PreparedScene and summaries; deterministic basic point/line/polygon/image command preparation; CPU and GPU adapters, external target and RGBA pixel contracts. Separate image encoding utility when its closure is approved. Core text definitions remain available; full text acceptance follows M4.

**Feasibility first:** G-02 proves approved Vello/wgpu integration, shared scene semantics, u16/device limits, runtime-independent async GPU completion and output conversion. Demonstrate a scene can release input snapshots and render again. Do not reintroduce a Tokio dependency into layer 1 to simplify the server.

**Verification:** T-01–T-06 as applicable, T-17–T-22, memory-only portions of T-11/T-13, and cancellation/admission primitives of T-47. Use open/empty/zero-length line, holes, mixed components, nested properties/full integer identities, transparent pixels, wrong texture contracts and pre/post-submit cancellation cases.

**Exit:** memory-only CPU closed loop passes; GPU adapter evidence is recorded with actual device/platform; feature-disabled builds behave as declared; no source IO/style/layout in backend execution; deterministic order and explicit bounds established. Hardware not yet available remains an explicit gate to M5, not a false parity completion.

### M2 — Readers, Rust projection, raster processing and TOML

**Prerequisites:** M1 data/scene contracts; G-01 for readers/projection/config closure; early G-03 experiments.

**Deliverables:** proper root-aware bounded GeoJSON reader; Shapefile/DBF reader; GeoTIFF metadata/native window access for validated layouts; independent batched cursors; identity/schema/queryable validation; native spatial queries; explicit Rust projection operations; raster validity/calibration/resampling/color stages; separate typed TOML style/map/animation definitions and resource graph validation; resource body schemas and local external-path registration through the administration plane. Reader-composition preparation and one-shot convenience calls use these operations.

**Feasibility first:** prove bounds and lossless properties/IDs; validate primary IFD, WhiteIsZero raw recovery, source alpha and special TIFF layouts before claiming them. Reject unsupported masks explicitly. Test SubIFD decoding independently; use base access if source overview support is unproven. Do not equate tag access with decoded overview access.

**Verification:** T-04–T-15, T-17–T-25 and T-34. Inspect peak decode/sort allocations, malformed-input behavior, strict/tolerant boundaries, exact feature intersections and source-order restoration. Schema/config snippets become schema-tested fixtures only after actual parsers exist.

**Exit:** all three input formats render through common preparation with explicit format limits; CRS/encoding/missing metadata errors are deterministic; typed properties and raw raster contracts survive; basic TOML resolves before rendering; supported CRS/domain combinations have evidence. G-03 failures follow the owner decision rule.

### M3 — Derived storage and basic service closed loop

**Prerequisites:** M2 reader/query/render contracts; G-01 for Axum/cache/storage/codec closure; G-03 derived-format experiments; temporary explicit server profile for experiments.

**Deliverables:** budgeted immutable FGB shards and lossless companions/ID lookup/source ordinals; raster block directory and weighted overviews; DataVersion/DerivedVersion/RenderRevision; atomic resource publication, lazy deduplicated preparation, refresh retention, references/GC; protected `/admin/v1` resource operations; streamed managed uploads, restricted ZIP safety, async operation status, managed/external ownership, reference checks, sharded catalog index and atomic generations; bounded queues and work classes; Moka/Foyer caching and diagnostics; separate metatile-render/tile-encode singleflight; Axum startup and basic map/tile HTTP closed loop.

**Implementation boundary:** basic HTTP/tile delivery is an internal MVP, not first-stage completion or a blanket OGC conformance claim. The server must call library contracts rather than carry duplicate projection/layout logic. Do not implement a business SQLite store.

**Verification:** T-16, T-23, T-36–T-41 as available, T-46–T-50; cold/hot concurrent requests, area-render reuse, requested-only encodes, refresh failure, cache corruption, no-waiter cancellation, disk-full preparation and old-version retention. Verify actual backend in keys and initialization-only fallback.

**Administration verification:** T-55–T-64 cover ownership/kinds, streamed single-file and ZIP upload, external registration, record-only retention, content deletion refusal for external files, dependency pins, stale ETags, operation progress, crash recovery and scale. Reject malicious archives before unbounded extraction. Recheck versions/references at commit and test concurrent updates to different shards without lost changes. Preserve detached managed bytes after record removal.

**Exit:** registered source to consistent HTTP image/tile response works under bounded concurrency; atomic publication/refresh and cache identities are proven; caches do not retain full source snapshots; old live versions are protected; error/preparing/revalidation behavior is documented. Formal numerical performance goals remain open until M5.

**Resource exit:** managed single-file and Shapefile ZIP uploads, external path registration, unpublish/record removal/content deletion, ETag conflicts, reference protection and operation failure rollback have evidence. The catalog generation is indexed at request time, survives restart from the last complete generation, and reports orphaned files without registering them. G-08 and G-09 remain open until archive helper dependencies and scale measurements are approved.

### M4 — Text, complete protocol subset, animation and WebSocket

**Prerequisites:** M3 stable resource/scheduler contracts; G-04 text/font proof; G-06 exact protocol contract and advertised classes; approved helper dependencies if needed.

**Deliverables:** explicit isolated font stacks and approved test subsets; Unicode shaping/bidi/wrap and global collision layout; curved text/metatiles; stateless geometry/style animation; full agreed WMS/WMTS and OGC API Features/Maps/Tiles surfaces; honest WFS query subset with metadata/schema; complete WS state machine and paired image writer; JSON OpenAPI and static HTML API docs; independent demonstrations/request examples without server viewer hosting.

**Protocol contract work:** freeze exact tagged fields, variant selector, vendor time parameter, parameter duplicate/case rules, namespace/ID/schema encoding, RFC3339 parsing, error/status/close-code mapping, conformance URIs and bounded message behavior. The accepted behavior is the constraint; introducing new capabilities or dependencies requires owner approval.

**Verification:** T-09–T-10, T-26–T-33 and T-40–T-50. Include Arabic/Indic/CJK/ligatures, isolated fallback, long clusters/curved labels, bbox/null/timeless datetime, protocol axes, hits/pages during refresh, stale frame disposal, slow client, grouped invalid update, pause/seek/retry/reconnect and unpublish.

**Exit:** all agreed protocol operations match their contracts; conformance output lists only verified classes; no full WFS claim; animation is reproducible at fixed time/resources; metadata/image pairs cannot interleave; full public interface documentation and examples describe actual implementation rather than drafts. Font source/license/hash records and ordinary offline fixtures are complete.

### M5 — Native projection, platforms, migration and measured acceptance

**Prerequisites:** M1–M4 behavior suites; G-05 for optional native stack; realistic recorded platform/device access; G-01 rechecked for final feature closures.

**Deliverables:** fixed dynamically linked PROJ 9.6.2 distribution with approved dependency/db/grid/source/hash/license records and enforced no-fallback build; Windows MSVC/Linux x86_64 feature/CPU CI plus real DX12/Vulkan evidence; default GPU Alpine image built from a musl stage with Vulkan loader/device mapping and CPU initialization fallback; independent CPU-only `-cpu` Alpine image without GPU/Vulkan runtime; local-source deployment build/migration guidance; reproducible performance baseline and owner-confirmed development/small profiles/targets.

**Verification:** T-01 feature matrix across supported targets, T-22 real GPU, T-35 native, T-51–T-54 final evidence, T-64 catalog scale, T-65 default GPU Alpine and T-66 independent CPU Alpine. Re-run only checks affected by final changes or unresolved cross-platform concerns. Separate native-proj from server-full defaults and exercise failure when approved native dependencies are absent.

**Exit:** full correctness/contract suite has named evidence on the supported matrix; unavailable hardware/results are visible and block the associated completion claim; native versions are not silently replaced; migration/docs agree with code; complete dependency/native/font inventory approved; formal performance/profile gate G-07 approved after measurements. No claim of official OGC certification unless separately obtained.

## 3. Verification and evidence format

Use an evidence record per meaningful scenario/group:

| Field | Required information |
| --- | --- |
| Trace | Requirement IDs, T-scenarios, milestone and gate |
| Build identity | Commit, exact lockfile/features, toolchain and compiler/native versions |
| Platform | OS/architecture, hardware and real GPU backend/driver where relevant |
| Input | Fixture/version/hash, source CRS/grid/options, deterministic generator seed/scale |
| Method | Command/harness, temporary or approved profile, comparison/reference and tolerance |
| Result | Pass/fail/unavailable with output/diagnostic artifacts and bounded measurements |
| Status | Which narrow claims are Validated/Implemented; remaining gaps and required decisions |

Do not mark a subsystem validated solely from compilation, source inspection, unit test count, package presence or one cached successful image. Store large synthetic outputs outside tracked fixtures and retain reproducible recipes. Review fixture redistribution and font notices before committing assets.

### Platform/feature matrix

| Configuration | Windows MSVC | Linux x86_64 | Extra evidence |
| --- | --- | --- | --- |
| Backend-free library | Build + memory/scene tests | Build + memory/scene tests | No Tokio/server dependency requirement |
| CPU with each reader and ordinary server | Correctness/contract suites | Correctness/contract suites | Offline deterministic fixtures |
| GPU library/server | Real DX12 | Real Vulkan | Device/driver, parity tolerance, cancellation/readback and target validation |
| `server-full` | Feature closure + service tests | Feature closure + service tests | Native PROJ absent by default |
| `native-proj` combinations | Fixed dynamic native build/test | Fixed dynamic native build/test | Missing-library fail-fast and offline db/grids |
| Default GPU Alpine image | Linux container host only; Windows DX12 remains a separate native test | Musl/Alpine + real mapped Vulkan device | Includes CPU initialization fallback; verify driver ABI, volumes and readiness |
| Independent CPU Alpine image | Linux container environment | Musl/Alpine CPU-only runtime | No GPU/Vulkan closure; own functionality, startup, size and performance evidence |

Shared CI may lack GPU devices; use dedicated/manual hardware runs with recorded evidence. CPU success is not a substitute for a GPU texture requirement.

## 4. Performance and operational acceptance

Tile throughput is the optimization priority under bounded fairness for animation and background jobs. Preserve style/geometry/data quality and conservative overview choice under load. Cache weights and external allocation estimates do not promise exact process RSS limits; record that distinction.

First build fixed-seed synthetic vector/raster workloads covering dense points, long lines, complex polygons/holes, large properties, multilingual labels, large strips/tiles, continuous/category samples and concurrent publication. Ordinary tests stay offline and compact; large inputs are generated reproducibly, not committed as giant binaries. Later business datasets are a separate evaluation, and synthetic results do not claim business performance.

Measure cold source prepare, derived generation, uncached render, warm object/response cache, refresh and contention. Record parse/query/projection/style/layout/scene/execute/readback/encode/storage times; request latency percentiles and throughput; RSS and estimated GPU residency; temporary/retired disk growth; queued work; cancellation release and recovery. Compare backend/platform separately with exact profiles and hardware.

Only after that evidence propose CPU concurrency, memory/GPU limits, queue capacities/fairness, deadlines, message/send limits, cursor absolute TTL/count, cache budgets and development/small deployment profiles. Ask the owner to confirm the concrete profiles and formal throughput/latency/resource targets. Until approval, label all experiment values temporary and keep G-07 open. Do not insert convenient numbers into default config and later treat them as approved requirements.

## 5. Migration and release preparation

| Existing behavior | Target migration |
| --- | --- |
| Mutable RenderedGeometry + RenderOption | Typed source feature/raster data, resolved ordered composition, immutable PreparedScene and explicit execution |
| RON/MagicValue/file-or-URL value fetching | Strict separate TOML resource graph resolved during preparation |
| Request source/style paths | Registered namespace dataset/map/style IDs and explicit publication |
| `/wmts/real-time`, `/wmts/cache` | One protocol surface with transparent server caching and correct identities |
| `/ws/anim` cumulative radius demo | One-map WS state machine with explicit time and independent evaluation |
| `/map`/static data-directory viewer | Independent demos and API documentation |
| Hardcoded projection assumptions | Explicit CRS/axis/backend/domain contracts and optional offline native stack |
| Registry-installed container binary | Build the verified checked-out source with approved lock/features/native inputs |
| No resource management API | Protected `/admin/v1` managed upload, external registration, unpublish, record removal, managed-content deletion and operation polling |
| Direct resource-directory layout | Independent immutable resource files/TOML, sharded catalog indexes and atomic generation pointers |
| Existing Dockerfile's registry install | Musl builder plus pinned Alpine runtime; ordinary tag GPU-preferred, `-cpu` tag CPU-only |

Do not publish new usage commands as operationally verified until implemented and tested. Update README/service/config/API examples together as milestones deliver real capabilities. Document intentional unsupported formats/classes and error behavior. Release version, publishing, deployment and Git commits are separate actions, not automatically authorized by this documentation plan.

## 6. M0 delivery checklist and handoff

- Six English body documents and one navigation page; root README points to them.
- Resource administration, managed/external ownership, restricted ZIP staging, sharded catalog generations and reference-protected deletion are described and traceable.
- The ordinary image tag is explicitly the default GPU Alpine image; the `-cpu` tag is an independent CPU-only Alpine variant with separate validation.
- Requirements, decisions, milestones, scenarios and evidence limitations agree.
- Exact approved direct versions and selected native/font/transitive decisions are retained; unresolved closure/hash/compatibility tasks remain explicit.
- Embedded TOML/JSON is syntactically valid under available parsers; semantic validation is not claimed before implementation.
- Local links resolve, fences/tables are well formed, no misleading target completion percentages or inherited reference checkmarks.
- Only README/docs changed; no Rust, Cargo files/lock, runtime configs, fixture downloads, CI, installation or auto-commit.

Handoff should provide the navigation link, summarize the contents and verification performed, and list G-01 through G-11 as applicable future work. The next implementation step is the approved dependency-closure review plus M1 feasibility/core work, not another unbounded architecture discussion or a wholesale merge of `update`.
