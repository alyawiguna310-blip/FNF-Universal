# Flutter ↔ Rust Project Bridge

The `fu-bridge` crate exposes project operations from `fu-project` through a
small C ABI. Rust remains the owner of the project schema and validation rules.

## Exported functions

- `fu_core_version() -> u32`: native core ABI version.
- `fu_bridge_api_version() -> u32`: bridge API version.
- `fu_project_create_json(...)`: accepts JSON for `ProjectMetadata` and
  returns a new empty `ProjectDocument`.
- `fu_project_validate_json(...)`: returns a JSON validation response.
- `fu_project_normalize_json(...)`: validates and returns canonical,
  pretty-printed project JSON.

## Buffer contract

Each JSON function accepts:

1. UTF-8 input pointer and byte length.
2. Output pointer and output capacity.

The caller owns both buffers. First call with a null output pointer and zero
capacity to query the required output byte length. Allocate that many bytes and
call again. A successful call returns the number of bytes written. If the
buffer is too small, the function returns the negative required byte length.
A null output pointer with non-zero capacity returns `-1`.

The output is UTF-8 JSON. Error responses have an `error` string. Validation
responses contain `valid` and `issues`; each issue has a `path` and
`message`.

## Safety requirements for FFI callers

- Input must point to at least the declared number of readable bytes.
- Output must point to at least the declared number of writable bytes.
- Input and output buffers must not overlap.
- Input is UTF-8; output length is measured in bytes, not characters.
- Never pass a dangling pointer or a length that exceeds the allocated buffer.

These functions parse data and validate project structure only. They do not load
assets, access files, or execute mod scripts.

## Current scope

This is the first native bridge layer, not yet the complete Flutter integration.
The next step is a Dart FFI wrapper that resolves the native library per
platform and uses these functions for editor create/load/validate/save flows.

## Flutter integration

The launcher now has a Dart FFI client at
`apps/launcher/lib/services/native_project_bridge.dart`. It loads the
platform library and checks `fu_bridge_api_version()` before calling JSON
operations. The caller owns its UTF-8 buffers; the wrapper performs the
required-size query, allocates the output buffer, decodes the response, and
frees both buffers.

The editor uses `ProjectDocumentCodec` as a UI boundary adapter. It maps the
current editor controls into the canonical Rust envelope:

- `songs[].notes[].gameplay` contains lane, strum time and sustain duration.
- `songs[].notes[].visual` remains a separate optional renderer binding.
- Characters, stages, objects, assets and creator settings use the shared
  project model.
- Existing v0.1 editor files using the old `chart`, `character` and
  `stage` layout are migrated when loaded.

Saving and loading pass through Rust normalization/validation. Flutter does
not implement a second set of project validation rules.

## Native library packaging

Build the Rust dynamic library for each target and package it where the
platform loader can find it:

### Windows

From the repository root, with the Rust MSVC toolchain installed:

```powershell
cargo build -p fu-bridge --release
```

Copy `target/release/fu_bridge.dll` beside the generated
`fnf_universal.exe` in the Flutter Windows runner output directory.

### Android (ARM64)

Install the Android NDK, the Rust `aarch64-linux-android` target, and
`cargo-ndk`. Then run from the repository root:

```powershell
rustup target add aarch64-linux-android
cargo install cargo-ndk
cargo ndk -t arm64-v8a -o apps/launcher/android/app/src/main/jniLibs build --release -p fu-bridge
```

The resulting library is packaged as
`android/app/src/main/jniLibs/arm64-v8a/libfu_bridge.so`. Add other Android
ABIs only when they are part of the intended device support matrix.

### Linux

```sh
cargo build -p fu-bridge --release
```

Place `target/release/libfu_bridge.so` beside the Flutter Linux executable
or in a platform loader search path.

The Dart wrapper currently supports Windows, Android, Linux and macOS library
names. iOS packaging is not configured yet. Native binaries are build outputs;
do not commit generated DLL/SO files to the source repository.

## Local Flutter checks

From `apps/launcher`:

```sh
flutter pub get
flutter test
flutter analyze
```

The native library must be built and packaged before running save, load or
project-preview actions. If it is missing or its ABI version differs, the
launcher reports a bridge-loading/version error rather than silently using a
different project schema.

