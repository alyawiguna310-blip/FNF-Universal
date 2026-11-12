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
