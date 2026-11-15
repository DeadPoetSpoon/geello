# M5 migration guide

M5 keeps the registered resource graph and the frozen HTTP/WebSocket protocol subset from M4. Deployment now builds the checked-out source with locked dependencies; the old registry-install Dockerfile is no longer supported.

## Configuration

Use the strict TOML schema and provide `listen`, an absolute `public_base_url`, an owned `data_directory`, `external_roots`, `backend` (`cpu`, `gpu`, or `gpu-preferred`) and every `[profile]` capacity. Relative data paths resolve from the configuration file. The ordinary image is GPU-preferred with CPU initialization fallback; `Dockerfile.cpu` and `deploy/server.cpu.toml` force CPU and contain no Vulkan package.

Map definitions may set `projection_backend = "rust"` or, when the process was built with `native-proj` and an approved distribution, `projection_backend = "native"`. Backend names are explicit and never fall back. Native operation identity is included in render revisions and response keys, so switching libraries or databases cannot reuse old pixels.

## Build and data migration

Use Rust 1.96.0 and `cargo build --locked`. For native projection, build and verify the fixed dynamic distribution first; do not enable `native-proj` with an unset `GEELLO_NATIVE_ROOT`. Keep `proj.db` and every reviewed grid under the distribution's `share/proj`; network downloads are disabled.

Stop the previous service before copying its data directory. Start the new binary once, inspect operation recovery and catalog reconciliation, then publish a changed source through the administration API with its strong ETag. The service creates a new immutable derived generation and invalidates response identities. Keep the previous directory until the new generation is ready; use the catalog maintenance endpoints for garbage collection and orphan diagnostics.

The old mutable geometry/render-option API, direct source/style paths and experimental `/wmts/real-time` and `/wmts/cache` endpoints are not part of the refactor. Use registered dataset/style/map IDs and the unified OGC/WMS/WMTS/WFS/WebSocket routes documented by generated OpenAPI. Put TLS, proxy authentication and the external base URL in the reverse proxy.

