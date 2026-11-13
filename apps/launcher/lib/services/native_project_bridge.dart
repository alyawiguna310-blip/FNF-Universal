import 'dart:convert';
import 'dart:ffi' as ffi;
import 'dart:io';

import 'package:ffi/ffi.dart';

typedef _JsonNative = ffi.IntPtr Function(
  ffi.Pointer<ffi.Uint8>,
  ffi.UintPtr,
  ffi.Pointer<ffi.Uint8>,
  ffi.UintPtr,
);
typedef _JsonDart = int Function(
  ffi.Pointer<ffi.Uint8>,
  int,
  ffi.Pointer<ffi.Uint8>,
  int,
);

typedef _VersionNative = ffi.Uint32 Function();
typedef _VersionDart = int Function();

/// Dart FFI client for the Rust-owned FNF Universal project model.
///
/// The native library must be built for the current platform and made
/// available to the app package before these methods are called.
///
/// Library lookup order (desktop):
/// 1. The path in the `FU_BRIDGE_LIB` environment variable.
/// 2. Next to the app executable (the packaged location).
/// 3. `target/release` then `target/debug` of the Cargo workspace, searched
///    upward from the executable and the current directory (development).
class NativeProjectBridge {
  NativeProjectBridge._(ffi.DynamicLibrary library)
      : _apiVersion = library.lookupFunction<_VersionNative, _VersionDart>(
          'fu_bridge_api_version',
        ),
        _create = library.lookupFunction<_JsonNative, _JsonDart>(
          'fu_project_create_json',
        ),
        _validate = library.lookupFunction<_JsonNative, _JsonDart>(
          'fu_project_validate_json',
        ),
        _normalize = library.lookupFunction<_JsonNative, _JsonDart>(
          'fu_project_normalize_json',
        );

  static const int supportedApiVersion = 1;
  static const int _maxOutputBytes = 16 * 1024 * 1024;

  final _VersionDart _apiVersion;
  final _JsonDart _create;
  final _JsonDart _validate;
  final _JsonDart _normalize;

  static NativeProjectBridge open() {
    final library = _openPlatformLibrary();
    final NativeProjectBridge bridge;
    try {
      bridge = NativeProjectBridge._(library);
    } catch (error) {
      throw ProjectBridgeException(
        'The native bridge library was loaded but is missing an expected '
        'function ($error). Rebuild it with: cargo build -p fu-bridge --release',
      );
    }
    final version = bridge._apiVersion();
    if (version != supportedApiVersion) {
      throw ProjectBridgeException(
        'Unsupported native bridge API version $version; '
        'expected $supportedApiVersion.',
      );
    }
    return bridge;
  }

  static String _libraryFileName() {
    if (Platform.isWindows) return 'fu_bridge.dll';
    if (Platform.isMacOS) return 'libfu_bridge.dylib';
    if (Platform.isAndroid || Platform.isLinux) return 'libfu_bridge.so';
    throw UnsupportedError(
      'FNF Universal native project bridge is not configured for this platform.',
    );
  }

  static ffi.DynamicLibrary _openPlatformLibrary() {
    final fileName = _libraryFileName();

    // On Android the library is packaged in the APK and found by name.
    if (Platform.isAndroid) {
      try {
        return ffi.DynamicLibrary.open(fileName);
      } catch (error) {
        throw ProjectBridgeException(
          'Could not load $fileName from the Android app package: $error',
        );
      }
    }

    final tried = <String>[];
    for (final path in _candidatePaths(fileName)) {
      if (!File(path).existsSync()) {
        tried.add('$path (not found)');
        continue;
      }
      try {
        return ffi.DynamicLibrary.open(path);
      } catch (error) {
        tried.add('$path (failed to load: $error)');
      }
    }

    throw ProjectBridgeException(
      'Native library $fileName was not found. Build it with '
      '"cargo build -p fu-bridge --release" and rebuild the app, or set '
      'FU_BRIDGE_LIB to its full path. Looked in:\n${tried.join('\n')}',
    );
  }

  static List<String> _candidatePaths(String fileName) {
    final paths = <String>[];

    final override = Platform.environment['FU_BRIDGE_LIB'];
    if (override != null && override.isNotEmpty) {
      paths.add(override);
    }

    final exeDir = File(Platform.resolvedExecutable).parent;
    paths.add(_join(exeDir.path, fileName));
    if (Platform.isLinux) {
      paths.add(_join(_join(exeDir.path, 'lib'), fileName));
    }

    // Development fallback: walk up looking for the Cargo target directory.
    for (final start in [exeDir, Directory.current]) {
      var dir = start.absolute;
      for (var i = 0; i < 10; i++) {
        for (final profile in const ['release', 'debug']) {
          paths.add(_join(_join(_join(dir.path, 'target'), profile), fileName));
        }
        final parent = dir.parent;
        if (parent.path == dir.path) break;
        dir = parent;
      }
    }

    // De-duplicate while keeping order.
    return paths.toSet().toList();
  }

  static String _join(String a, String b) =>
      a.endsWith(Platform.pathSeparator) ? '$a$b' : '$a${Platform.pathSeparator}$b';

  String createProject(String metadataJson) => _invoke(_create, metadataJson);

  Map<String, dynamic> validateProject(String projectJson) {
    final decoded = jsonDecode(_invoke(_validate, projectJson));
    if (decoded is! Map<String, dynamic>) {
      throw const ProjectBridgeException(
        'The native bridge returned an invalid validation response.',
      );
    }
    return decoded;
  }

  String normalizeProject(String projectJson) {
    final decoded = jsonDecode(_invoke(_normalize, projectJson));
    if (decoded is! Map<String, dynamic>) {
      throw const ProjectBridgeException(
        'The native bridge returned an invalid project response.',
      );
    }
    final error = decoded['error'];
    if (error != null) {
      final issues = decoded['issues'];
      throw ProjectBridgeException(
        error.toString(),
        issues: issues is List ? issues : const [],
      );
    }
    return const JsonEncoder.withIndent('  ').convert(decoded);
  }

  String _invoke(_JsonDart function, String input) {
    final inputBytes = utf8.encode(input);
    // calloc(0) is avoided so an empty input still gets a valid pointer.
    final inputPointer = calloc<ffi.Uint8>(inputBytes.isEmpty ? 1 : inputBytes.length);
    ffi.Pointer<ffi.Uint8> outputPointer = ffi.nullptr;

    try {
      if (inputBytes.isNotEmpty) {
        inputPointer.asTypedList(inputBytes.length).setAll(0, inputBytes);
      }

      // Size query: with a null output and zero capacity the bridge reports
      // the required length as a negative number (see crate docs).
      final sizeResult = function(
        inputPointer,
        inputBytes.length,
        ffi.nullptr,
        0,
      );
      final required = sizeResult < 0 ? -sizeResult : sizeResult;
      if (required <= 0 || required > _maxOutputBytes) {
        throw ProjectBridgeException(
          'Native bridge returned an invalid output size: $sizeResult.',
        );
      }

      outputPointer = calloc<ffi.Uint8>(required);
      final written = function(
        inputPointer,
        inputBytes.length,
        outputPointer,
        required,
      );
      if (written != required) {
        throw ProjectBridgeException(
          'Native bridge wrote $written bytes; expected $required.',
        );
      }
      return utf8.decode(outputPointer.asTypedList(written));
    } finally {
      calloc.free(inputPointer);
      if (outputPointer != ffi.nullptr) {
        calloc.free(outputPointer);
      }
    }
  }
}

class ProjectBridgeException implements Exception {
  const ProjectBridgeException(this.message, {this.issues = const []});

  final String message;
  final List<dynamic> issues;

  @override
  String toString() => message;
}