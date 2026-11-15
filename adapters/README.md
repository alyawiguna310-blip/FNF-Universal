# Engine adapters

Adapters are engine-specific integration packages. Each one is split into data
import and runtime integration. The V-Slice entry has a read-only data importer
(`fu_import::vslice`, see `docs/VSLICE_ADAPTER.md`); it does not launch a game yet.

Before implementing a runner, pin a source commit/build, document its license and
redistribution permissions, list native dependencies, and verify platform support.
