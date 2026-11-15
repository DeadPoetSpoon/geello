# Fixed dynamic PROJ binding patch

Origin: `proj-sys 0.27.0`, <https://github.com/georust/proj>, MIT OR Apache-2.0.
The generated `bundled_bindings.rs` is unchanged from the crates.io archive.
The upstream licenses are preserved. `bindings_docs-rs.rs` is unused.

The upstream build script can download/build a static bundled PROJ when system
discovery fails. This patch replaces that script with an exact-version,
target-and-file-hash preflight and dynamic linking only. It removes the bundled
PROJ archive, `libsqlite3-sys`, `link-cplusplus`, CMake, tar and flate2 build
dependencies. It adds no SQLite business catalog and no network feature.
It exports the distribution identity for runtime checking/cache partitioning.
`tools/native-sources.json` and `tools/build_native_proj.py` own the separate,
explicit native source build. A missing distribution always fails the build.

This is a reviewed local fork, not the unchanged registry package; its files
and upstream archive hash are recorded in the M5 dependency inventory.
