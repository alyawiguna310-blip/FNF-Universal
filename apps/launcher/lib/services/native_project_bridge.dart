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
class NativeProjectBridge {
  NativeProjectBridge._(this._library)
      : _apiVersion = _library
            .lookupFunction<_VersionNative, _VersionDart>(
              'fu_bridge_api_version',
            ),
        _create = _library.lookupFunction<_JsonNative, _JsonDart>(
          'fu_project_create_json',
        ),
        _validate = _library.lookupFunction<_JsonNative, _JsonDart>(
          'fu_project_validate_json',
        ),
        _normalize = _library.lookupFunction<_JsonNative, _JsonDart>(
          'fu_project_normalize_json',
        );

  static const int supportedApiVersion = 1;
  static const int _maxOutputBytes = 16 * 1024 * 1024;

  final ffi.DynamicLibrary _library;
  final _VersionDart _apiVersion;
  final _JsonDart _create;
  final _JsonDart _validate;
  final _JsonDart _normalize;

  static NativeProjectBridge open() {
    final library = _openPlatformLibrary();
    final bridge = NativeProjectBridge._(library);
    final version = bridge._apiVersion();
    if (version != supportedApiVersion) {
      throw ProjectBridgeException(
        'Unsupported native bridge API version $version; '
        'expected $supportedApiVersion.',
      );
    }
    return bridge;
  }

  static ffi.DynamicLibrary _openPlatformLibrary() {
    if (Platform.isAndroid || Platform.isLinux) {
      return ffi.DynamicLibrary.open('libfu_bridge.so');
    }
    if (Platform.isWindows) {
      return ffi.DynamicLibrary.open('fu_bridge.dll');
    }
    if (Platform.isMacOS) {
      return ffi.DynamicLibrary.open('libfu_bridge.dylib');
    }
    throw UnsupportedError(
      'FNF Universal native project bridge is not configured for this platform.',
    );
  }

  String createProject(String metadataJson) =>
      _invoke(_create, metadataJson);

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
    final inputPointer = calloc<ffi.Uint8>(inputBytes.length);
    ffi.Pointer<ffi.Uint8> outputPointer = ffi.nullptr;

    try {
      inputPointer.asTypedList(inputBytes.length).setAll(0, inputBytes);
      final required = function(
        inputPointer,
        inputBytes.length,
        ffi.nullptr,
        0,
      );
      if (required <= 0 || required > _maxOutputBytes) {
        throw ProjectBridgeException(
          'Native bridge returned an invalid output size: $required.',
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
