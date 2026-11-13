import 'dart:async';
import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import 'background_image.dart';
import 'menu_definition.dart';

abstract class LauncherUiStorage {
  Future<String?> read();
  Future<void> write(String json);
  Future<List<String>> listBackgrounds();
  Future<Uint8List?> readBackground(String name);

  /// Saves a wallpaper and returns the final file name (a number is added if
  /// that name is already taken, so nothing is overwritten).
  Future<String> writeBackground(String name, Uint8List bytes);
}

/// Saves under `Documents/FNFUniversal`: launcher_ui.json and Backgrounds/.
class FileLauncherUiStorage implements LauncherUiStorage {
  Future<Directory> _root() async {
    final base = await getApplicationDocumentsDirectory();
    final dir = Directory('${base.path}${Platform.pathSeparator}FNFUniversal');
    await dir.create(recursive: true);
    return dir;
  }

  Future<Directory> _backgroundsDir() async {
    final root = await _root();
    final dir = Directory('${root.path}${Platform.pathSeparator}Backgrounds');
    await dir.create(recursive: true);
    return dir;
  }

  Future<File> _file() async {
    final root = await _root();
    return File('${root.path}${Platform.pathSeparator}launcher_ui.json');
  }

  @override
  Future<String?> read() async {
    final file = await _file();
    if (!await file.exists()) return null;
    return file.readAsString();
  }

  @override
  Future<void> write(String json) async {
    final file = await _file();
    await file.writeAsString(json, flush: true);
  }

  @override
  Future<List<String>> listBackgrounds() async {
    final dir = await _backgroundsDir();
    final names = <String>[];
    await for (final e in dir.list(followLinks: false)) {
      if (e is! File) continue;
      final name = e.uri.pathSegments.last;
      if (cleanBackgroundName(name) != null) names.add(name);
    }
    names.sort();
    return names;
  }

  @override
  Future<Uint8List?> readBackground(String name) async {
    final clean = cleanBackgroundName(name);
    if (clean == null) return null;
    final dir = await _backgroundsDir();
    final file = File('${dir.path}${Platform.pathSeparator}$clean');
    if (!await file.exists()) return null;
    if (await file.length() > kMaxBackgroundBytes) return null;
    final bytes = await file.readAsBytes();
    return looksLikeImage(bytes) ? bytes : null;
  }

  @override
  Future<String> writeBackground(String name, Uint8List bytes) async {
    final clean = cleanBackgroundName(name);
    if (clean == null) {
      throw const BackgroundImportException('That file name is not allowed.');
    }
    final dir = await _backgroundsDir();
    final sep = Platform.pathSeparator;
    final dot = clean.lastIndexOf('.');
    var target = File('${dir.path}$sep$clean');
    var n = 2;
    while (await target.exists()) {
      target = File('${dir.path}$sep${clean.substring(0, dot)}-$n${clean.substring(dot)}');
      n++;
    }
    await target.writeAsBytes(bytes, flush: true);
    return target.uri.pathSegments.last;
  }
}

String _encode(LauncherUiDefinition d) =>
    const JsonEncoder.withIndent('  ').convert(d.toJson());

/// UI state only. Holds the launcher look, notifies listeners on edits, and
/// saves/loads it. It never touches engine or project data.
class LauncherUiController extends ChangeNotifier {
  LauncherUiController({LauncherUiDefinition? initial, this.storage})
      : _definition = initial ?? LauncherUiDefinition.defaults() {
    _savedJson = _encode(_definition);
  }

  final LauncherUiStorage? storage;
  LauncherUiDefinition _definition;
  late String _savedJson;
  int _revision = 0;
  List<String> _warnings = const [];
  Uint8List? _backgroundBytes;
  String? _backgroundName;
  bool _disposed = false;

  LauncherUiDefinition get definition => _definition;

  /// Bumps when the whole definition is replaced (load, reset, apply from a
  /// package) so text fields can rebuild with the new values.
  int get revision => _revision;

  List<String> get warnings => _warnings;

  /// Decoded-ready bytes of the current wallpaper, or null (none, missing or
  /// still loading).
  Uint8List? get backgroundBytes => _backgroundBytes;

  bool get hasUnsavedChanges => _encode(_definition) != _savedJson;

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }

  Future<void> _syncBackground() async {
    final name = _definition.backgroundImage;
    if (name == _backgroundName) return;
    _backgroundName = name;
    final s = storage;
    if (name == null || s == null) {
      if (_backgroundBytes != null) {
        _backgroundBytes = null;
        _notify();
      }
      return;
    }
    Uint8List? bytes;
    try {
      bytes = await s.readBackground(name);
    } catch (_) {
      bytes = null;
    }
    if (_backgroundName != name) return; // changed while loading
    _backgroundBytes = bytes;
    _notify();
  }

  Future<void> load() async {
    final s = storage;
    if (s == null) return;
    try {
      final text = await s.read();
      if (text == null) return;
      final result = LauncherUiDefinition.parse(jsonDecode(text));
      _definition = result.definition;
      _savedJson = _encode(_definition);
      _warnings = result.warnings;
      _revision++;
      _notify();
      unawaited(_syncBackground());
    } catch (_) {
      // Keep the defaults if the saved file is unreadable.
    }
  }

  /// Live edit. Not saved until [save].
  void update(LauncherUiDefinition next) {
    _definition = next;
    _notify();
    unawaited(_syncBackground());
  }

  /// Swap in a whole new definition (for example from a mod package).
  void replace(LauncherUiDefinition next, {List<String> warnings = const []}) {
    _definition = next;
    _warnings = warnings;
    _revision++;
    _notify();
    unawaited(_syncBackground());
  }

  void resetToDefaults() => replace(LauncherUiDefinition.defaults());

  Future<void> save() async {
    final json = _encode(_definition);
    await storage?.write(json);
    _savedJson = json;
    _notify();
  }

  Future<List<String>> listBackgrounds() async =>
      await storage?.listBackgrounds() ?? const <String>[];

  /// Copies an image the player points at into the Backgrounds folder.
  /// Returns the saved name, or null if there is no storage (tests).
  Future<String?> importBackgroundFile(String path) async {
    final s = storage;
    if (s == null) return null;
    final file = File(path);
    if (!await file.exists()) {
      throw const BackgroundImportException('No file at that path.');
    }
    if (await file.length() > kMaxBackgroundBytes) {
      throw const BackgroundImportException('That image is larger than 8 MB.');
    }
    final name = sanitizeBackgroundName(path.split(RegExp(r'[\\/]')).last);
    if (name == null) {
      throw const BackgroundImportException('Use a png, jpg or webp file with a simple name.');
    }
    final bytes = await file.readAsBytes();
    if (!looksLikeImage(bytes)) {
      throw const BackgroundImportException('That file is not a real png, jpg or webp image.');
    }
    return s.writeBackground(name, bytes);
  }

  /// Installs wallpaper bytes that came from a package. Returns the saved
  /// name, or null if it was refused or there is no storage.
  Future<String?> installBackground(String name, Uint8List bytes) async {
    final s = storage;
    final clean = cleanBackgroundName(name);
    if (s == null || clean == null) return null;
    if (bytes.length > kMaxBackgroundBytes || !looksLikeImage(bytes)) return null;
    return s.writeBackground(clean, bytes);
  }
}
