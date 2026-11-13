import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import '../ui/menu_definition.dart';
import 'zip_io.dart';

/// FNF Universal package: one zip that carries a project, all its scripts and
/// the menu/UI changes. See docs/MOD_PACKAGE_FORMAT.md.
///
/// Safety rules (from docs/SECURITY_AND_IMPORT.md):
/// - Nothing in a package is ever executed or converted during inspection.
/// - Import writes a fresh copy into its own folder. It never overwrites.
/// - Unsafe paths and executable/binary file types are never extracted.
const int kModPackageFormat = 1;

enum ModPackageKind {
  mod('mod'),
  launcherProfile('launcher-profile');

  const ModPackageKind(this.wireName);

  final String wireName;
}

enum ReportStatus { supported, needsAttention, unsupported }

class ReportItem {
  const ReportItem(this.path, this.status, this.message);

  final String path;
  final ReportStatus status;
  final String message;
}

class ModManifest {
  const ModManifest({
    required this.format,
    required this.kind,
    required this.name,
    required this.createdUtc,
    this.generator = 'FNF Universal 0.1.0',
  });

  final int format;
  final ModPackageKind kind;
  final String name;
  final String createdUtc;
  final String generator;

  Map<String, dynamic> toJson() => {
        'format': format,
        'kind': kind.wireName,
        'name': name,
        'createdUtc': createdUtc,
        'generator': generator,
      };

  static ModManifest? tryParse(Object? raw, List<String> errors) {
    if (raw is! Map) {
      errors.add('fnfmod.json is not a JSON object.');
      return null;
    }
    final format = raw['format'];
    if (format is! int) {
      errors.add('fnfmod.json has no format number.');
      return null;
    }
    if (format > kModPackageFormat) {
      errors.add('Package format $format is newer than this app understands ($kModPackageFormat). Update FNF Universal.');
      return null;
    }
    ModPackageKind? kind;
    for (final k in ModPackageKind.values) {
      if (k.wireName == raw['kind']) kind = k;
    }
    if (kind == null) {
      errors.add('Unknown package kind.');
      return null;
    }
    final nameRaw = raw['name'];
    var name = nameRaw is String && nameRaw.trim().isNotEmpty
        ? nameRaw.trim()
        : 'Imported package';
    if (name.length > 60) name = name.substring(0, 60);
    final created = raw['createdUtc'];
    return ModManifest(
      format: format,
      kind: kind,
      name: name,
      createdUtc: created is String ? created : '',
    );
  }
}

class ModPackageReport {
  const ModPackageReport({
    required this.manifest,
    required this.items,
    required this.errors,
    required this.ui,
  });

  final ModManifest? manifest;
  final List<ReportItem> items;
  final List<String> errors;
  final LauncherUiParseResult? ui;

  bool get canImport => manifest != null && errors.isEmpty;

  int count(ReportStatus s) => items.where((i) => i.status == s).length;

  int get scriptCount => items
      .where((i) =>
          i.path.startsWith('scripts/') && i.status != ReportStatus.unsupported)
      .length;
}

// ---------------------------------------------------------------------------
// Path and file-type safety
// ---------------------------------------------------------------------------

const Set<String> _reservedNames = {
  'CON', 'PRN', 'AUX', 'NUL',
  'COM1', 'COM2', 'COM3', 'COM4', 'COM5', 'COM6', 'COM7', 'COM8', 'COM9',
  'LPT1', 'LPT2', 'LPT3', 'LPT4', 'LPT5', 'LPT6', 'LPT7', 'LPT8', 'LPT9',
};

/// Returns a normalized, forward-slash relative path, or null if the path
/// could escape its folder or is not valid on every platform.
String? safeRelativePath(String raw) {
  if (raw.isEmpty || raw.length > 240) return null;
  final normalized = raw.replaceAll('\\', '/');
  if (normalized.startsWith('/') || RegExp(r'^[A-Za-z]:').hasMatch(normalized)) {
    return null;
  }
  final parts = normalized.split('/');
  if (parts.length > 24) return null;
  final cleaned = <String>[];
  for (var i = 0; i < parts.length; i++) {
    final part = parts[i];
    if (part.isEmpty) {
      if (i == parts.length - 1) continue;
      return null;
    }
    if (part == '.' || part == '..') return null;
    if (RegExp(r'[\u0000-\u001F<>:"|?*]').hasMatch(part)) return null;
    if (part.endsWith(' ') || part.endsWith('.')) return null;
    if (_reservedNames.contains(part.split('.').first.toUpperCase())) return null;
    cleaned.add(part);
  }
  if (cleaned.isEmpty) return null;
  return cleaned.join('/');
}

const Set<String> _blockedExtensions = {
  'exe', 'dll', 'bat', 'cmd', 'ps1', 'psm1', 'sh', 'apk', 'aab', 'so',
  'dylib', 'jar', 'msi', 'com', 'scr', 'vbs', 'lnk', 'app',
};

/// Executables and native libraries are never packaged or imported.
bool isBlockedScriptPath(String path) {
  final name = path.split('/').last.toLowerCase();
  final dot = name.lastIndexOf('.');
  if (dot < 0) return false;
  return _blockedExtensions.contains(name.substring(dot + 1));
}

String safeFileStem(String name) {
  final cleaned = name
      .trim()
      .replaceAll(RegExp(r'[^A-Za-z0-9._-]+'), '_')
      .replaceAll(RegExp(r'^[._]+'), '');
  if (cleaned.isEmpty) return 'package';
  return cleaned.length > 60 ? cleaned.substring(0, 60) : cleaned;
}

ReportItem _classify(String safePath) {
  if (safePath == 'fnfmod.json') {
    return ReportItem(safePath, ReportStatus.supported, 'Package manifest.');
  }
  if (safePath == 'launcher_ui.json') {
    return ReportItem(safePath, ReportStatus.supported, 'Menu/UI definition (applied only when you choose).');
  }
  if (safePath.startsWith('project/') && safePath.endsWith('.fnfup.json')) {
    return ReportItem(safePath, ReportStatus.supported, 'Editor project, copied as-is.');
  }
  if (safePath.startsWith('scripts/')) {
    if (isBlockedScriptPath(safePath)) {
      return ReportItem(safePath, ReportStatus.unsupported, 'Blocked for safety: executables and native libraries are never imported.');
    }
    return ReportItem(
      safePath,
      ReportStatus.needsAttention,
      'Script file. Copied as plain text, never executed, not converted for other loaders.',
    );
  }
  return ReportItem(safePath, ReportStatus.unsupported, 'Not part of the FNF Universal package format. Ignored.');
}

// ---------------------------------------------------------------------------
// Build
// ---------------------------------------------------------------------------

Uint8List _pretty(Object json) =>
    Uint8List.fromList(utf8.encode(const JsonEncoder.withIndent('  ').convert(json)));

Uint8List buildModPackage({
  required ModPackageKind kind,
  required String name,
  required LauncherUiDefinition ui,
  String? projectJson,
  Map<String, Uint8List> scripts = const {},
  DateTime? nowUtc,
}) {
  final manifest = ModManifest(
    format: kModPackageFormat,
    kind: kind,
    name: name,
    createdUtc: (nowUtc ?? DateTime.now().toUtc()).toIso8601String(),
  );
  final entries = <ZipEntryData>[
    ZipEntryData('fnfmod.json', _pretty(manifest.toJson())),
    ZipEntryData('launcher_ui.json', _pretty(ui.toJson())),
  ];
  if (kind == ModPackageKind.mod) {
    if (projectJson != null) {
      entries.add(ZipEntryData(
        'project/${safeFileStem(name)}.fnfup.json',
        Uint8List.fromList(utf8.encode(projectJson)),
      ));
    }
    for (final item in scripts.entries) {
      final safe = safeRelativePath(item.key);
      if (safe == null || isBlockedScriptPath(safe)) {
        throw ArgumentError('Script path is not allowed in a package: ${item.key}');
      }
      entries.add(ZipEntryData('scripts/$safe', item.value));
    }
  }
  return buildZip(entries);
}

class ScriptCollection {
  const ScriptCollection(this.files, this.skipped);

  final Map<String, Uint8List> files;
  final List<String> skipped;
}

/// Reads script files from [dir] (recursively) for packaging. Blocked file
/// types, unsafe names and anything over the size/count caps are skipped and
/// listed in `skipped`. Nothing is executed.
Future<ScriptCollection> collectScriptFiles(
  Directory dir, {
  int maxFiles = 500,
  int maxFileBytes = 20 * 1024 * 1024,
  int maxTotalBytes = 100 * 1024 * 1024,
}) async {
  final files = <String, Uint8List>{};
  final skipped = <String>[];
  if (!await dir.exists()) return ScriptCollection(files, skipped);
  final rootPath = dir.absolute.path;
  var total = 0;
  await for (final entity in dir.list(recursive: true, followLinks: false)) {
    if (entity is! File) continue;
    var rel = entity.absolute.path.substring(rootPath.length).replaceAll('\\', '/');
    while (rel.startsWith('/')) {
      rel = rel.substring(1);
    }
    final safe = safeRelativePath(rel);
    if (safe == null || isBlockedScriptPath(safe)) {
      skipped.add(rel);
      continue;
    }
    final length = await entity.length();
    if (files.length >= maxFiles ||
        length > maxFileBytes ||
        total + length > maxTotalBytes) {
      skipped.add(rel);
      continue;
    }
    files[safe] = await entity.readAsBytes();
    total += length;
  }
  return ScriptCollection(files, skipped);
}

// ---------------------------------------------------------------------------
// Inspect (read-only) and import (writes a new folder)
// ---------------------------------------------------------------------------

ModPackageReport inspectModPackage(Uint8List zip) {
  final List<ZipEntryInfo> entries;
  try {
    entries = listZipEntries(zip);
  } on ZipFormatException catch (e) {
    return ModPackageReport(
      manifest: null,
      items: const [],
      errors: ['Not a readable zip: ${e.message}'],
      ui: null,
    );
  }

  final errors = <String>[];
  final items = <ReportItem>[];
  ModManifest? manifest;
  ZipEntryInfo? manifestEntry;
  for (final e in entries) {
    if (e.path == 'fnfmod.json') manifestEntry = e;
  }
  if (manifestEntry == null) {
    errors.add('fnfmod.json is missing. This is not an FNF Universal package.');
  } else {
    try {
      final text = utf8.decode(readZipEntry(zip, manifestEntry));
      manifest = ModManifest.tryParse(jsonDecode(text), errors);
    } on ZipFormatException catch (e) {
      errors.add(e.message);
    } on FormatException {
      errors.add('fnfmod.json is not valid JSON.');
    }
  }

  LauncherUiParseResult? ui;
  for (final entry in entries) {
    if (entry.isDirectory) continue;
    final safe = safeRelativePath(entry.path);
    if (safe == null) {
      items.add(ReportItem(entry.path, ReportStatus.unsupported, 'Unsafe path. Skipped.'));
      continue;
    }
    var item = _classify(safe);
    if (safe == 'launcher_ui.json') {
      try {
        final text = utf8.decode(readZipEntry(zip, entry));
        ui = LauncherUiDefinition.parse(jsonDecode(text));
        if (ui.warnings.isNotEmpty) {
          item = ReportItem(
            safe,
            ReportStatus.needsAttention,
            '${ui.warnings.length} adjustment(s) made: ${ui.warnings.first}',
          );
        }
      } on ZipFormatException catch (e) {
        item = ReportItem(safe, ReportStatus.unsupported, e.message);
      } on FormatException {
        item = ReportItem(safe, ReportStatus.unsupported, 'Not valid JSON. Ignored.');
      }
    }
    items.add(item);
  }
  return ModPackageReport(manifest: manifest, items: items, errors: errors, ui: ui);
}

class ImportResult {
  const ImportResult(this.directory, this.written, this.skipped);

  final Directory directory;
  final int written;
  final int skipped;
}

class ModPackageException implements Exception {
  const ModPackageException(this.message);

  final String message;

  @override
  String toString() => message;
}

String _two(int n) => n.toString().padLeft(2, '0');

/// Extracts the safe, supported files of a package into a brand-new folder
/// under [importRoot]. Never overwrites anything that exists.
Future<ImportResult> importModPackage(
  Uint8List zip,
  Directory importRoot, {
  DateTime? nowUtc,
}) async {
  final report = inspectModPackage(zip);
  final manifest = report.manifest;
  if (!report.canImport || manifest == null) {
    throw ModPackageException(report.errors.join(' '));
  }
  final now = nowUtc ?? DateTime.now().toUtc();
  final stamp =
      '${now.year}${_two(now.month)}${_two(now.day)}-${_two(now.hour)}${_two(now.minute)}${_two(now.second)}';
  final base = '${importRoot.path}${Platform.pathSeparator}${safeFileStem(manifest.name)}-$stamp';
  var target = Directory(base);
  var n = 2;
  while (await target.exists()) {
    target = Directory('$base-$n');
    n++;
  }
  await target.create(recursive: true);

  var written = 0;
  var skipped = 0;
  for (final entry in listZipEntries(zip)) {
    if (entry.isDirectory) continue;
    final safe = safeRelativePath(entry.path);
    if (safe == null || _classify(safe).status == ReportStatus.unsupported) {
      skipped++;
      continue;
    }
    final file = File('${target.path}/$safe');
    await file.parent.create(recursive: true);
    await file.writeAsBytes(readZipEntry(zip, entry), flush: true);
    written++;
  }
  return ImportResult(target, written, skipped);
}
