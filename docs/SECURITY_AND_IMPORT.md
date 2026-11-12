# Mod Security and Import

## Import pipeline

1. Read archive/folder metadata without executing scripts.
2. Detect source engine and version from manifests and known file patterns.
3. Index assets and chart data.
4. Parse supported script languages into an intermediate representation.
5. Run static security and compatibility analysis.
6. Convert supported data and API calls into the Universal project format.
7. Produce a report for unsupported or ambiguous behavior.
8. Write a separate converted copy; never overwrite the original.

## Compatibility labels

- **Supported:** converted and covered by compatibility tests.
- **Needs attention:** conversion completed with warnings or manual mapping needed.
- **Unsupported:** required behavior cannot currently be represented.

Never silently discard unsupported features. Preserve source files and attribution metadata. Check licenses before bundling or redistributing source code, assets or mods.

## File permissions

A mod may write only to its own designated generated/cache area through approved APIs. It must not delete engine files, other mods, user documents or system files. A self-destruct mechanic may remove its own generated in-game content; it must never delete the game or unrelated files.

## Repair

The repair system should verify installed engine files against trusted release manifests and restore missing or modified engine files from a trusted original package. It must not trust a mod to provide its own repair source.
