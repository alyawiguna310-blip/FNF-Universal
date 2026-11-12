# Testing

## Rust
- Manifest validation: valid and invalid manifests.
- Registry: duplicate engine IDs rejected.
- Chart metadata: JSON metadata extraction.
- Asset index: recursive file discovery and extension filtering.
- Benchmark: elapsed-time measurement sanity test.

## Integration still required
- Launch a real V-Slice build on Android, Windows, and Linux.
- Verify per-engine save/config isolation.
- Test audio latency and note timing under load.
- Compare baseline FPS, frame-time percentiles, memory, and song load time.
- Validate mod/chart compatibility against each engine's actual version.

No engine compatibility is asserted by this foundation.
