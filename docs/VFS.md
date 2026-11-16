# Virtual File System (VFS) Foundation (`fu-vfs`)

## Overview

`fu-vfs` is the platform-independent Virtual File System (VFS) crate for FNF Universal. It provides a multi-mount, priority-based content overlay system that allows mods, base-game assets, and writable user data to be accessed through clean, unified virtual paths.

`fu-vfs` is strictly an infrastructure crate: it owns path normalization, mount arbitration, security validation, cascading asset resolution, and directory union listing. It does not execute scripts, parse charts, or manage graphics pipelines.

---

## 1. Architecture

```text
               +-----------------------------------------+
               |        Consumers & Subsystems           |
               | (Stage Loader, Characters, Audio, etc.) |
               +--------------------+--------------------+
                                    |
                            Unified Virtual Path
                        ("/images/characters/bf.png")
                                    |
                                    v
                       +-------------------------+
                       |           Vfs           |
                       |  (Mount & Priority Mgr) |
                       +------------+------------+
                                    |
       +----------------------------+----------------------------+
       | Priority: 100              | Priority: 50               | Priority: 0
       v                            v                            v
+--------------+             +--------------+             +--------------+
| Mod B Mount  |             | Mod A Mount  |             |  Base Mount  |
|  (Physical)  |             |  (Physical)  |             |  (Physical)  |
+--------------+             +--------------+             +--------------+
       | (NotFound)                 |                            |
       +--------------------------->| (Resolves Asset)           |
                                    +----------------------------+
```

### Core Design Principles
1. **Decoupled from Host Filesystems**: Virtual paths are strictly platform-independent. Code consuming assets never handles Windows drive letters (`C:`), Windows backslashes (`\`), or absolute host directory paths.
2. **Explicit Mount Priority**: Resolution order is determined strictly by explicit integer priority values (`priority: i32`) and path specificity, never by filesystem ordering or alphabetical mod folder names.
3. **Cascading Fallback**: If a higher-priority mount does not provide a requested asset, resolution transparently falls through to lower-priority mounts and eventually the base-game assets.
4. **Union Directory Listing**: Calling `read_dir("/data/characters")` aggregates files from all active mounts, deduplicating matching names in priority order so all available assets can be discovered at once.

---

## 2. Virtual Path Specification (`VfsPath`)

All virtual paths in `fu-vfs` adhere to strict invariants:
- **Root Anchor**: All virtual paths start with `/`. The root path is `"/"`.
- **Consistent Separators**: Forward slashes (`/`) are exclusively used. Backslashes (`\`) provided in input strings are normalized to `/`.
- **Component Preservation**: Filename and folder components preserve internal, leading, and trailing whitespace within segments (e.g., `"/songs/ my song / Inst .ogg"`). Empty segments resulting from redundant slashes (`//`) or `.` segments are stripped.
- **Path Traversal Rejection**: Any path segment containing `..` is immediately rejected with `VfsError::PathTraversal`.
- **Host Leaking Prevention**: Any path containing `:` (Windows drive letters, NTFS alternate streams) or null bytes (`\0`) is rejected with `VfsError::InvalidPath`.
- **Validated Serde Deserialization**: `VfsPath` implements `#[serde(try_from = "String")]`. Any configuration or network payload deserialized via serde is strictly validated through the same normalization and traversal rejection pipeline; invalid paths cannot be constructed via deserialization.

---

## 3. Mount System & Prioritization

### Mount Attributes
Every mount registered with the VFS includes:
- `id: String`: Unique mount identifier (e.g. `"base"`, `"mod:vs-var-overhaul"`, `"user_saves"`).
- `mount_point: VfsPath`: Virtual directory prefix where content is rooted (e.g. `"/"`, `"/user"`).
- `priority: i32`: Resolution priority. Higher numbers take precedence.
- `policy: MountPolicy`: `ReadOnly` or `ReadWrite`.
- `case_sensitivity: CaseSensitivity`: `Sensitive` or `Insensitive`.
- `enabled: bool`: Dynamic toggle. Disabled mounts are bypassed during resolution.
- `backend: Arc<dyn VfsBackend>`: The storage backend implementation.

### Resolution Algorithm
When resolving a virtual path `path`:
1. Find all enabled mounts where `path.starts_with(mount.mount_point)`.
2. Compute the relative subpath within the mount (`path.strip_prefix(mount.mount_point)`).
3. Sort candidate mounts by:
   - **Prefix Specificity**: Longest `mount_point` first (e.g. `"/user/saves"` takes precedence over `"/"`).
   - **Priority**: Higher integer `priority` first.
   - **Stable Tie-Breaker**: Lexicographical `id`.
80: 4. Check candidates sequentially for reads/metadata:
81:    - If the candidate contains the file, return it.
82:    - If `NotFound`, continue to the next candidate (cascading fallback).
83:    - If any candidate returns another error (such as `AccessDenied` or `Io`), abort and propagate immediately without continuing fallback.
84:    - If all candidates return `NotFound`, return `VfsError::NotFound(path)`.
85: 5. **Read Provenance (`read_with_provenance`)**:
86:    - Returns `VfsReadOutput` containing `bytes`, the winning `mount_id`, `mount_priority`, and `canonical_relative_path` (preserving exact backend entry casing).
87:    - Identifies the backend that actually provided the file bytes during resolution, preventing race conditions or discrepancy with write targets or metadata-only queries.
88: 6. For write operations (`write`, `create_dir`):
89:    - The top candidate (`candidates[0]`) determines the write policy.
90:    - If candidate 0 has `MountPolicy::ReadOnly`, the operation immediately returns `VfsError::ReadOnlyMount`. Writes do not fall through to lower writable layers, preventing mutation of shadowed state.
91:    - If candidate 0 has `MountPolicy::ReadWrite`, the operation proceeds on candidate 0's backend.
92:    - If no candidate mount exists, returns `VfsError::NoWritableMount(path)`.

---

## 4. Mount Types

| Mount Type | Backend Struct | Description | Use Case |
| :--- | :--- | :--- | :--- |
| **Physical Mount** | `PhysicalMount` | Backs virtual paths with a host filesystem directory (`PathBuf`). | Base assets directory, unpacked mod folders, desktop user data directory. |
| **Memory Mount** | `MemoryMount` | Thread-safe in-memory file tree (`RwLock<BTreeMap>`). | Unit tests, mock environments, generated virtual configs, temporary caches. |
| **Archive Mount** *(Planned)* | `ArchiveMount` | Reads content directly from a `.zip` or `.pak` archive without unpacking. | Packaged community mods, distributed DLC packs. |
| **Android Asset Mount** *(Planned)* | `AndroidAssetMount` | Interfaces with Android `AAssetManager` via JNI/NDK. | Android base-game APK bundled assets in `assets/`. |

---

## 5. Security & Isolation Boundaries

Untrusted community mod content presents security risks if allowed unrestricted filesystem access. `fu-vfs` enforces strict isolation boundaries:
1. **Physical Filesystem Containment & Symlink Defense**:
   - `VfsPath` rejects `..`, drive letters, and null bytes before paths reach backends.
   - For existing paths, `PhysicalMount` canonicalizes host paths and verifies that resolved paths strictly start with the canonical mount root (`canonical.starts_with(&canonical_root)`).
   - For nonexistent nested paths during writes/directory creation, `PhysicalMount` walks up the directory tree to find the nearest existing ancestor, canonicalizes it, and verifies it resides within the mount root. This stops symlink junction escapes targeting nonexistent nested paths (e.g., `/symlink_dir/new_dir/evil.txt`).
   - After `fs::create_dir_all` and `fs::write`, `PhysicalMount` re-verifies the canonical target to ensure no intermediate symlink was introduced.
   - **Never Delete Paths Outside Mount Root**: If a post-write or post-create containment check detects that a path escaped the mount root, the operation immediately aborts with `VfsError::PathTraversal`. The VFS **never** executes cleanup or deletion (`remove_file` / `remove_dir`) on paths outside the mount root, preventing accidental or malicious destruction of host filesystem data.
2. **Write Isolation & Layer Shadowing**:
   - Write operations (`write()`, `create_dir()`) are governed strictly by the top-priority mount matching the path prefix.
   - If the active mount at that prefix is `ReadOnly`, the write is rejected with `VfsError::ReadOnlyMount`. Writes are never permitted to "fall through" to lower-priority writable mounts (which would create silent desynchronization where newly written files remain shadowed on read).
   - Dedicated writable directories (e.g. `"/user"` or `"/saves"`) must be mounted with longer prefix specificity so they cleanly claim write authority over those subtrees.
3. **Backend & Enumeration Error Propagation**:
   - Error handling differentiates between absence (`VfsError::NotFound`) and system failures (`VfsError::AccessDenied`, `VfsError::Io`).
   - Only `NotFound` permits fallback evaluation. Permission errors or hardware I/O errors are immediately surfaced, preventing dangerous fallback masking.
   - Directory enumeration during case-insensitive scanning (`find_case_insensitive_entry`) and directory listing (`read_dir`) explicitly propagates I/O and file type query errors instead of silently swallowing or discarding them.
4. **No Script Execution**:
   - The VFS only serves raw bytes or strings. It does not parse or execute HScript, Lua, or shell commands.

---

## 6. Windows & Android Compatibility Strategy

### Deterministic Case-Insensitive Resolution
- **The Problem**: Windows filesystems (NTFS) are case-insensitive. Many FNF mods authored on Windows have mixed casing (e.g. referencing `Neu.png` in code while the file is named `neu.png`, or `Inst.ogg` vs `inst.ogg`). On Linux and Android (ext4/f2fs), these mismatches fail to open. Furthermore, filesystems with case collisions or differing directory enumeration orders can cause non-deterministic behavior across platforms.
- **The Solution**: `fu-vfs` allows configuring `CaseSensitivity::Insensitive` per mount.
  - On `PhysicalMount`, path resolution first attempts an exact case match. If not found, it performs a directory search comparing entry names with `eq_ignore_ascii_case()`.
  - **Deterministic Collision Tie-Breaking**: When multiple files match the same query case-insensitively (e.g. `asset.png` vs `ASSET.png`), resolution prioritizes an exact case match. If neither is exact, matches are sorted lexicographically (`match.sort()`) and the first entry is deterministically returned.
  - On `MemoryMount`, lookup follows the exact same deterministic rule.
  - This allows existing Codename Engine and Psych Engine mods to run consistently and deterministically across Windows and Android.

### Platform-Specific Integration

```text
+--------------------------------------------------------------+
|                        Unified VFS                           |
+------------------------------+-------------------------------+
                               |
            +------------------+------------------+
            |                                     |
            v                                     v
+-----------------------+             +-----------------------+
|    Windows Desktop    |             |    Android Mobile     |
+-----------------------+             +-----------------------+
| Base:                 |             | Base:                 |
| PhysicalMount         |             | AndroidAssetMount     |
| (D:\FNF\assets)       |             | (APK assets via NDK)  |
|                       |             |                       |
| Mods:                 |             | Mods:                 |
| PhysicalMount         |             | PhysicalMount         |
| (D:\FNF\mods\*)       |             | (/sdcard/FNF/mods/*)  |
|                       |             |                       |
| User Data:            |             | User Data:            |
| PhysicalMount (RW)    |             | PhysicalMount (RW)    |
| (%APPDATA%\FNF)       |             | (/data/data/.../files)|
+-----------------------+             +-----------------------+
```

---

## 7. Known Limitations & Threat Model

1. **Filesystem TOCTOU (Time-of-Check to Time-of-Use) Limitation**:
   - Because standard Rust on Windows and Android lacks ubiquitous access to directory-file-descriptor primitives like Linux `openat2` (`RESOLVE_BENEATH`), userspace path validation followed by `fs::write` or `fs::create_dir_all` inherently contains a theoretical race window. A malicious process running concurrently with sufficient privileges could swap an intermediate directory component with a symlink between the canonical check and file creation.
   - `fu-vfs` mitigates this with multi-layer checks: nearest-ancestor canonicalization before creation and post-creation containment validation. However, applications running completely untrusted multi-threaded code in the same process space should be aware of this OS-level limitation.
2. **Archive Backends Not Yet Implemented**: Currently `PhysicalMount` and `MemoryMount` are fully implemented and tested. Zip/Tar archive backends require pulling in an archive decompression crate (e.g. `zip`).
3. **Android NDK Asset Bridge Not Yet Wired**: `PhysicalMount` supports Android's app-private filesystem (`/data/data/<pkg>/files/`) and external storage, but direct reading from the APK asset pool requires an `AAssetManager` wrapper.
4. **No Dynamic Hot-Reload Watcher**: While files are read dynamically from disk on each `read()`, an active file-system watcher (e.g. `notify`) for mod asset hot-reloading during editing is planned for a future phase.

---

## 8. Future Integration Points

- **Stage Loader**: Will resolve stage XML files (`/data/stages/<stage>.xml`) and referenced sprite sheets (`/images/stages/<stage>/...`) through `Vfs`.
- **Character Loader**: Will resolve character definitions (`/data/characters/<char>.xml`) and Sparrow/Adobe Animate texture atlases.
- **Shader Pipeline**: Will resolve custom fragment shaders (`/shaders/<name>.frag`) across base and mod directories.
- **Audio Engine**: Will stream `Inst.ogg` and `Voices.ogg` from `/songs/<id>/song/` via VFS file handles.
- **Mod Configuration**: Will parse `modpack.ini` and `flags.ini` from `/data/config/` for active mods.
