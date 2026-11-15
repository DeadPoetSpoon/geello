# M7 release automation and evidence

M7 closes the gap between a clean source audit and an automated release. The
SBOM generator now has a `--check` mode, so CI verifies the committed inventory
without modifying the checkout. The release audit also checks SBOM freshness
and requires both container variants to declare the liveness health check.

The acceptance workflow uses the check mode. The tag-based Docker workflow runs
the release audit before logging in, building, pushing, or signing either image.
Container verification exercises both `/health/ready` and `/health/live`, and
the Dockerfiles use an explicit BusyBox health-check command available in the
Alpine runtime.

## Verification

| Check | Result | Evidence |
| --- | --- | --- |
| SBOM freshness without mutation | Pass | `python tools/generate_sbom.py --check` |
| Generated HTTP documents | Pass | `python tools/generate_http_docs.py --check` |
| Source release audit | Pass after commit | `python tools/verify_release.py` |
| Rust formatting and contracts | Pass | M6 full feature and integration evidence; M7 changes are workflow/tooling/container-only |
| Container runtime smoke | Workflow-enforced | `tools/verify_container.py` now checks readiness and liveness before publication/render/restart |

The Docker daemon and Alpine runtime remain unavailable on this development
host, so the container job is intentionally validated by CI rather than claimed
as a local runtime result. Real Vulkan hardware remains a separately dispatched
acceptance job. M7 does not publish a tag or image automatically from this
working session.
