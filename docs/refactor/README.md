# Geello refactor design baseline

Design baseline date: **2026-11-12**. Implementation branch: **`refactor`**. Reference branch: **`update`**. Documentation language: English.

Resource administration, indexed catalog storage and container-variant revision: **2026-11-13**. These additions supersede the earlier no-upload target boundary; existing implementation facts remain dated to the original assessment.

This package consolidates the first-stage redesign discussed with the project owner. It is a documentation delivery, not a rendering or server implementation. Earlier experimental code and the reference branch inform the assessment; neither dictates the new architecture.

## Reading order

| Order | Document | Purpose |
| --- | --- | --- |
| 1 | [Current state](01-current-state.md) | Establish what exists, with branch-specific evidence and limitations. |
| 2 | [Requirements and acceptance](02-requirements-and-acceptance.md) | Define the complete first-stage scope and traceable acceptance scenarios. |
| 3 | [Architecture and data flow](03-architecture-and-data-flow.md) | Define ownership, layering, ordering, versions, scheduling, and resource release. |
| 4 | [Public APIs and configuration](04-public-apis-and-configuration.md) | Describe target API contracts, feature composition, configuration examples, and protocol interactions. |
| 5 | [Decisions and dependencies](05-decisions-and-dependencies.md) | Record decisions, exact approved versions, sources, licenses, feasibility evidence, and remaining gates. |
| 6 | [Implementation roadmap](06-implementation-roadmap.md) | Sequence the work and define evidence required to finish each milestone. |

The design also covers the protected [resource lifecycle](04-public-apis-and-configuration.md#resource-administration-api), managed versus external sources, [resource index](03-architecture-and-data-flow.md#resource-files-catalog-generations-and-administration), and [container variants](04-public-apis-and-configuration.md#container-variants). The ordinary image tag is the target GPU Alpine variant; the `-cpu` tag is a CPU Alpine variant requiring independent verification. Neither image is implemented or validated by this revision.

Read the requirements before the examples. Examples illustrate the target and are not runnable against the current prototype. Proposed Rust names, TOML field spelling, and protocol envelopes must be checked against the accepted behavioral requirements when implementation schemas are introduced.

## Status vocabulary

Statuses are independent dimensions, not a single completion checkbox:

| Dimension | Meaning | Required evidence |
| --- | --- | --- |
| **Accepted** | The owner agreed to the requirement, design choice, version, or explicitly scoped exception. | A decision entry in this baseline; later changes need a recorded decision. |
| **Validated** | A particular claim was checked by inspection, an experiment, or a test. | Name the method, version, platform, input, and result. Source inspection is not an end-to-end runtime test. |
| **Implemented** | The target behavior exists in the implementation branch. | Code reference and relevant verification result; accepted design alone is insufficient. |

For the **target redesign**, requirements and decisions are Accepted unless explicitly marked as a proposal or later gate. Target implementation is **No** at this baseline. Runtime acceptance is **Pending**. Existing prototype functionality is described separately and does not count as target completion.

## Traceability and authority

Requirement IDs use `ARC`, `DAT`, `RAS`, `REN`, `STY`, `TXT`, `ANI`, `PRJ`, `STO`, `API`, `ADM`, `OPS`, and `DOC`. Decision IDs use `D-xx`; acceptance scenarios use `T-xx`; gates use `G-01` through `G-11`; implementation milestones use `M0` through `M5`. The requirements matrix links these dimensions. Evidence records must identify the exact requirement/scenario, rather than marking an entire subsystem complete from one passing test.

If descriptions disagree, preserve the explicitly accepted behavior and record the discrepancy. Do not copy a legacy behavior, silently weaken a requirement, upgrade a dependency, or claim an untested capability to resolve it. The project owner can revise this baseline as evidence emerges.

All changes occur on `refactor`. `main` and `update` are read-only references for this work. Breaking API, configuration, and URL changes are allowed; migration documentation is required. Source comments, public API documentation, and maintained examples must be English.

## Completion boundaries

The whole first-stage scope includes both CPU/GPU rendering, three source formats, projection, styles, text, animation, derived storage, HTTP/OGC services, WebSocket, and platform acceptance. Intermediate milestones are implementation sequencing, not exclusions from that scope.

The following gates intentionally follow this documentation delivery:

- Resolve the intended Cargo features, then submit the complete lockfile, transitive dependencies, source/license inventory, and native build inventory for owner approval before adopting them as the implementation baseline.
- Validate the Vello adapters, bounded readers, GeoTIFF layouts, font isolation, derived storage, and native PROJ build. Failures return to a documented design decision.
- Measure reproducible workloads before asking the owner to approve formal CPU/memory/GPU/queue/cache profiles and performance targets. Temporary experiment limits are not production defaults.

Resource uploads and local registrations belong to the server administration plane. Public OGC and WebSocket routes consume only published resource generations. Upload/archive limits, resource counts, image tags and Alpine base digests remain pending operational validation.

The dependency versions already approved in [the decision register](05-decisions-and-dependencies.md) remain fixed. “Use the latest by default” does not authorize upgrading them. This delivery does not alter Cargo files, Rust code, runtime configuration, CI, or Git history.
