# Build guide

## Requirements
- Rust stable toolchain + Cargo
- Flutter stable SDK
- Android Studio / Android SDK / NDK for Android
- Visual Studio C++ build tools for Windows desktop
- Linux desktop build dependencies required by Flutter

## Rust tests
Run from repository root:
```bash
cargo test --workspace
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## Flutter
The `apps/launcher` folder contains Dart source and a `pubspec.yaml`. Generate native
platform scaffolding with:
```bash
cd apps/launcher
flutter create --platforms=android,windows,linux .
flutter pub get
flutter run
```

## Bridge status
The bridge crate exposes a small C ABI for future native integration. The Flutter
screen is intentionally runnable as a UI prototype before the generated FFI bridge
is connected. Do not claim Rust calls are wired into Flutter until the bridge generation
and integration step is completed and tested on each platform.
