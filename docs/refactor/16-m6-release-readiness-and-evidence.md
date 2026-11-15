# M6 release readiness and evidence

M6 closes the reproducible release checks left after M5. It adds a separate
process liveness endpoint, container health checks, a deterministic
CycloneDX-style Cargo dependency inventory, and a fail-closed release audit.
The audit checks the lockfile/version identity, native source pins, font
manifest hash, generated HTTP documentation, feature boundaries and required
deployment files.

## Implemented contracts

`GET /health/live` returns `{"status":"alive"}` while the server accepts
health requests. It returns `503` after shutdown begins, while
`/health/ready` continues to describe initialized backend and catalog state.
Both GPU-preferred and CPU-only images use the endpoint for Docker
`HEALTHCHECK`.

`tools/generate_sbom.py` writes [m6-sbom.json](m6-sbom.json) from the locked
all-features Cargo metadata. Components are sorted by name/version and the
document records the SHA-256 of `Cargo.lock`; no network or third-party tool is
required beyond Cargo and Python's standard library.

`tools/verify_release.py` is intended to run from a clean checkout. It fails
when generated HTTP documents, dependency/native pins, font fixtures, feature
boundaries, or deployment contracts drift. A release candidate must pass this
audit before tagging or publishing an image.

## Verification record

| Check | Result | Evidence |
| --- | --- | --- |
| Full library and integration suite with native PROJ | Pass | `cargo test --locked --all-features --lib --tests`; all non-GPU tests passed; one real-GPU test ignored by design |
| HTTP contract suite | Pass | `cargo test --locked --all-features --test http_contracts`; 12 passed |
| Generated HTTP docs | Pass | `python tools/generate_http_docs.py --check` |
| SBOM generation | Pass | `python tools/generate_sbom.py`; output is `m6-sbom.json` |
| Release audit | Pass on clean committed checkout | `python tools/verify_release.py` |
| Native absent-distribution guard | Pass | `python tools/verify_native_gate.py` (M5 evidence) |

The native test command uses the previously recorded M5 Windows distribution
at `target/m5-native/install` and `GEELLO_NATIVE_ROOT`; CI rebuilds that
distribution instead. Real Vulkan hardware and Alpine runtime execution remain
environment-dependent M5 gates and are not implied by these source checks.

M6 does not change the approved dependency versions, protocol subset, formal
performance targets, or the owner approval status of G-07. Publishing a tag,
container image, or release signature remains a separate operator action after
the clean-checkout audit succeeds.
