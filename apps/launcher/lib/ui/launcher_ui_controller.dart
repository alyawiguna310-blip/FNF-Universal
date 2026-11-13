import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import 'menu_definition.dart';

abstract class LauncherUiStorage {
  Future<String?> read();
  Future<void> write(String json);
}

/// Saves to <Documents>/FNFUniversal/launcher_ui.json.
class FileLauncherUiStorage implements LauncherUiStorage {
  Future<File> _file() async {
    final base = await getApplicationDocumentsDirectory();
    final dir = Directory('${base.path}${Platform.pathSeparator}FNFUniversal');
    await dir.create(recursive: true);
    return File('${dir.path}${Platform.pathSeparator}launcher_ui.json');
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

  LauncherUiDefinition get definition => _definition;

  /// Bumps when the whole definition is replaced (load, reset, apply from a
  /// package) so text fields can rebuild with the new values.
  int get revision => _revision;

  List<String> get warnings => _warnings;

  bool get hasUnsavedChanges => _encode(_definition) != _savedJson;

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
      notifyListeners();
    } catch (_) {
      // Keep the defaults if the saved file is unreadable.
    }
  }

  /// Live edit. Not saved until [save].
  void update(LauncherUiDefinition next) {
    _definition = next;
    notifyListeners();
  }

  /// Swap in a whole new definition (for example from a mod package).
  void replace(LauncherUiDefinition next, {List<String> warnings = const []}) {
    _definition = next;
    _warnings = warnings;
    _revision++;
    notifyListeners();
  }

  void resetToDefaults() => replace(LauncherUiDefinition.defaults());

  Future<void> save() async {
    final json = _encode(_definition);
    await storage?.write(json);
    _savedJson = json;
    notifyListeners();
  }
}
