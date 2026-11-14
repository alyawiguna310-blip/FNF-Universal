import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:path_provider/path_provider.dart';

import 'content_catalog.dart';

abstract class ContentStore {
  Future<String?> readPath();
  Future<void> writePath(String? path);
}

/// Remembers the chosen folder in `Documents/FNFUniversal/content_source.json`.
class FileContentStore implements ContentStore {
  Future<File> _file() async {
    final docs = await getApplicationDocumentsDirectory();
    final dir = Directory('${docs.path}${Platform.pathSeparator}FNFUniversal');
    await dir.create(recursive: true);
    return File('${dir.path}${Platform.pathSeparator}content_source.json');
  }

  @override
  Future<String?> readPath() async {
    final file = await _file();
    if (!await file.exists()) return null;
    final raw = jsonDecode(await file.readAsString());
    if (raw is Map && raw['path'] is String) return raw['path'] as String;
    return null;
  }

  @override
  Future<void> writePath(String? path) async {
    final file = await _file();
    await file.writeAsString(jsonEncode({'path': path}), flush: true);
  }
}

/// UI state for Story and Freeplay: which folder is used and what was found.
/// It never launches or modifies anything in that folder.
class ContentController extends ChangeNotifier {
  ContentController({
    this.store,
    ContentCatalog? initial,
    Future<ContentCatalog> Function(Directory root)? scanner,
  })  : _catalog = initial ?? ContentCatalog.empty,
        _scanner = scanner ?? scanContentFolder;

  final ContentStore? store;
  final Future<ContentCatalog> Function(Directory root) _scanner;
  ContentCatalog _catalog;
  String? _path;
  bool _scanning = false;
  String? _error;
  bool _disposed = false;

  ContentCatalog get catalog => _catalog;
  String? get path => _path;
  bool get scanning => _scanning;
  String? get error => _error;

  void _notify() {
    if (!_disposed) notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    super.dispose();
  }

  Future<void> load() async {
    try {
      final saved = await store?.readPath();
      if (saved == null || saved.isEmpty) return;
      _path = saved;
      _notify();
      await rescan();
    } catch (_) {
      // No saved folder, or storage unavailable. Stay empty.
    }
  }

  Future<void> setPath(String newPath) async {
    final trimmed = newPath.trim();
    if (trimmed.isEmpty) return;
    _path = trimmed;
    try {
      await store?.writePath(trimmed);
    } catch (_) {
      // Not being able to remember the folder is not fatal.
    }
    await rescan();
  }

  Future<void> rescan() async {
    final p = _path;
    if (p == null) return;
    _scanning = true;
    _error = null;
    _notify();
    try {
      _catalog = await _scanner(Directory(p));
    } catch (e) {
      _error = 'Could not scan that folder: $e';
    }
    _scanning = false;
    _notify();
  }
}
