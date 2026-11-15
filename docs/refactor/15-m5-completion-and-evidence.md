# M5 completion and evidence

M5 implementation work is complete for the reproducible source, native, deployment, CI and measurement deliverables. The evidence boundary is explicit: Windows MSVC/DX12, Windows native PROJ, Linux x86_64 CPU/native and fixed-seed measurements are validated; a real Linux Vulkan device and Alpine runtime execution remain unavailable on this host and therefore are not claimed complete.

The exact dependency/native records are [m5-dependency-closure.json](m5-dependency-closure.json), [m5-native-windows.json](m5-native-windows.json), [m5-native-linux.json](m5-native-linux.json), `tools/native-sources.json`, the local `third_party/proj-sys` patch and the existing font manifest. CI covers Windows/Linux CPU/full/native combinations, absent-native fail-fast, native build/test, HTTP documentation freshness and independent CPU/GPU image smoke tests. A manual self-hosted GPU job requires the actual DX12/Vulkan device and records adapter information.

The only remaining approval gate is G-07: owner confirmation of development/small limits and formal performance targets after reviewing the measured artifacts. No OGC certification or cross-platform GPU claim is made.

