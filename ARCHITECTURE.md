# Architecture

```text
Flutter UI
   |
   | typed bridge (planned/generated)
   v
Rust shared services
   |-- engine registry + manifest validation
   |-- chart metadata inspection
   |-- asset indexing
   |-- read-only mod import + validation (fu-import, see docs/MOD_IMPORT.md)
   |-- V-Slice data import (fu-import::vslice, see docs/VSLICE_ADAPTER.md)
   |-- config + storage
   |-- benchmarks
   |
Engine adapter contract
   |-- V-Slice (data import implemented, runtime contract next)
   |-- Psych (future)
   |-- Kade (future)
   `-- custom engines (future)

Each engine runtime remains its own build/process/runtime.
```

## Why isolated runtimes?
FNF engines differ in framework, asset layout, chart formats, scripting, native libraries,
and platform support. A single adapter cannot safely assume these are interchangeable.

## Adapter lifecycle
1. Discover a manifest.
2. Validate its schema and supported platform.
3. Resolve an installed runtime path.
4. Check declared capabilities.
5. Launch through a platform-specific runner (not implemented in this foundation).
6. Collect exit status and logs.

## Rust boundary
Rust owns shared file/data services. The engine owns frame timing, note judgment,
rendering, and audio synchronization unless later profiling proves a safe integration path.
