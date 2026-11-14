import 'dart:convert';
import 'dart:io';

/// Read-only catalog of weeks ("levels") and songs found in a folder the player
/// points at: their own copy of the game or a mod folder. Only names, artist,
/// BPM and difficulty names are read from small text files. No audio, art,
/// charts, scripts or executables are read, copied or run.
///
/// Supported layouts (best effort, not yet verified against real builds, see
/// TESTING.md):
///
/// V-Slice / Funkin' Modding Docs (CC BY-SA 4.0):
///   data/levels/<level>.json               name, songs[], visible
///   data/songs/<id>/<id>-metadata.json     songName, artist, playData, timeChanges
///
/// Codename Engine (per its wiki):
///   data/weeks/weeks/<week>.xml            <week name=".."> <song>id</song> ...
///   data/weeks/weeks.txt                   optional week order
///   songs/<id>/meta.json                   name, displayName, bpm, difficulties
///   songs/<id>/charts/<difficulty>.json    only the file names are listed
///
/// Week XML is matched with a few small patterns, never parsed as full XML, so
/// entities and DOCTYPE tricks are not expanded.
const int _maxFileBytes = 1024 * 1024;
const int _maxVisited = 20000;
const int _maxDepth = 7;
const int _maxSongDirs = 2000;

final RegExp _nameRe = RegExp(r'^[A-Za-z0-9][A-Za-z0-9 _.-]{0,63}$');

/// Ids and file stems: letters, digits, space, dash, underscore, dot. No
/// slashes, no colons, no "..".
bool _nameOk(String s) =>
    _nameRe.hasMatch(s) && !s.contains('..') && !s.endsWith(' ') && !s.endsWith('.');

const Set<String> _skipDirs = {
  'images', 'sounds', 'music', 'videos', 'fonts', 'shaders', '.git', 'node_modules',
};

class SongInfo {
  const SongInfo({
    required this.id,
    required this.title,
    this.artist,
    this.difficulties = const [],
    this.bpm,
    this.origin,
  });

  final String id;
  final String title;
  final String? artist;
  final List<String> difficulties;
  final double? bpm;

  /// Name of the folder (for example the mod) this song came from.
  final String? origin;
}

class LevelInfo {
  const LevelInfo({
    required this.id,
    required this.name,
    required this.songIds,
    this.visible = true,
    this.origin,
  });

  final String id;
  final String name;
  final List<String> songIds;
  final bool visible;
  final String? origin;
}

class ContentCatalog {
  const ContentCatalog({
    required this.levels,
    required this.songs,
    this.warnings = const [],
    this.sourcePath,
  });

  static const ContentCatalog empty = ContentCatalog(levels: [], songs: {});

  final List<LevelInfo> levels;
  final Map<String, SongInfo> songs;
  final List<String> warnings;
  final String? sourcePath;

  bool get isEmpty => levels.isEmpty && songs.isEmpty;

  /// Levels shown in Story. Hidden levels still feed Freeplay.
  List<LevelInfo> get storyLevels => levels.where((l) => l.visible).toList();

  /// Songs in level order, then any song that is in no level, A to Z.
  List<SongInfo> get freeplaySongs {
    final out = <SongInfo>[];
    final used = <String>{};
    for (final level in levels) {
      for (final id in level.songIds) {
        final song = songs[id];
        if (song != null && used.add(id)) out.add(song);
      }
    }
    final rest = songs.values.where((s) => !used.contains(s.id)).toList()
      ..sort((a, b) => a.title.toLowerCase().compareTo(b.title.toLowerCase()));
    return [...out, ...rest];
  }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

String _name(FileSystemEntity e) {
  final segs = e.uri.pathSegments.where((s) => s.isNotEmpty);
  return segs.isEmpty ? '' : segs.last;
}

String _originOf(Directory d) {
  final n = _name(d);
  return n.isEmpty ? 'folder' : n;
}

String _stem(File f, String ext) =>
    _name(f).replaceFirst(RegExp('\\.$ext\$', caseSensitive: false), '');

String? _text(Object? raw, int max) {
  if (raw is! String) return null;
  final cleaned = raw.replaceAll(RegExp(r'[\u0000-\u001F\u007F]'), ' ').trim();
  if (cleaned.isEmpty) return null;
  return cleaned.length > max ? cleaned.substring(0, max) : cleaned;
}

Future<String?> _readText(File file, List<String> warnings) async {
  try {
    if (await file.length() > _maxFileBytes) {
      warnings.add('${_name(file)} is too large and was skipped.');
      return null;
    }
    return await file.readAsString();
  } on FormatException {
    warnings.add('${_name(file)} is not valid text and was skipped.');
    return null;
  } on FileSystemException {
    warnings.add('Could not read ${_name(file)}.');
    return null;
  }
}

Future<Object?> _readJson(File file, List<String> warnings) async {
  final text = await _readText(file, warnings);
  if (text == null) return null;
  try {
    return jsonDecode(text);
  } on FormatException {
    warnings.add('${_name(file)} is not valid JSON and was skipped.');
    return null;
  }
}

String _decodeEntities(String s) => s
    .replaceAll('&lt;', '<')
    .replaceAll('&gt;', '>')
    .replaceAll('&quot;', '"')
    .replaceAll('&apos;', "'")
    .replaceAll('&amp;', '&');

String? _xmlAttr(String attrs, String key) {
  final m = RegExp('$key\\s*=\\s*(?:"([^"]*)"|\'([^\']*)\')', caseSensitive: false)
      .firstMatch(attrs);
  if (m == null) return null;
  return _decodeEntities(m.group(1) ?? m.group(2) ?? '');
}

// ---------------------------------------------------------------------------
// Finding what to read
// ---------------------------------------------------------------------------

class _Roots {
  final List<Directory> vslice = [];
  final List<Directory> codename = [];
  bool get isEmpty => vslice.isEmpty && codename.isEmpty;
}

Future<bool> _looksLikeVSliceData(Directory d) async {
  final sep = Platform.pathSeparator;
  return await Directory('${d.path}${sep}levels').exists() ||
      await Directory('${d.path}${sep}songs').exists();
}

/// A Codename mod (or the base assets folder): has data/weeks, or a songs
/// folder whose children carry a meta.json.
Future<bool> _looksLikeCodenameRoot(Directory d) async {
  final sep = Platform.pathSeparator;
  if (await Directory('${d.path}${sep}data${sep}weeks').exists()) return true;
  final songs = Directory('${d.path}${sep}songs');
  if (!await songs.exists()) return false;
  var checked = 0;
  await for (final e in songs.list(followLinks: false)) {
    if (e is! Directory) continue;
    if (await File('${e.path}${sep}meta.json').exists()) return true;
    if (++checked >= 50) break;
  }
  return false;
}

Future<_Roots> _findRoots(Directory root, List<String> warnings) async {
  final roots = _Roots();
  var visited = 0;
  var stopped = false;

  Future<void> walk(Directory dir, int depth) async {
    if (depth > _maxDepth || stopped) return;
    await for (final e in dir.list(followLinks: false)) {
      visited++;
      if (visited > _maxVisited) {
        stopped = true;
        warnings.add('The folder is very large. The scan stopped early.');
        return;
      }
      if (e is! Directory) continue;
      final name = _name(e).toLowerCase();
      if (_skipDirs.contains(name)) continue;
      if (name == 'data' && await _looksLikeVSliceData(e)) {
        roots.vslice.add(e);
        continue;
      }
      if (await _looksLikeCodenameRoot(e)) {
        roots.codename.add(e);
        continue;
      }
      await walk(e, depth + 1);
    }
  }

  // The player may have pointed straight at a data folder or a mod folder.
  if (await _looksLikeVSliceData(root)) roots.vslice.add(root);
  if (await _looksLikeCodenameRoot(root)) roots.codename.add(root);
  if (roots.isEmpty) await walk(root, 0);
  return roots;
}

// ---------------------------------------------------------------------------
// V-Slice
// ---------------------------------------------------------------------------

Future<LevelInfo?> _readVSliceLevel(File file, String origin, List<String> warnings) async {
  final raw = await _readJson(file, warnings);
  if (raw is! Map) return null;
  final stem = _stem(file, 'json');
  if (!_nameOk(stem)) {
    warnings.add('Level file ${_name(file)} has an unusual name and was skipped.');
    return null;
  }
  final ids = <String>[];
  final songsRaw = raw['songs'];
  if (songsRaw is List) {
    for (final s in songsRaw) {
      if (s is String && _nameOk(s)) ids.add(s);
    }
  }
  return LevelInfo(
    id: stem,
    name: _text(raw['name'], 60) ?? stem,
    songIds: ids,
    visible: raw['visible'] is bool ? raw['visible'] as bool : true,
    origin: origin,
  );
}

Future<SongInfo> _readVSliceSong(
  Directory dir,
  String id,
  String origin,
  List<String> warnings,
) async {
  final file = File('${dir.path}${Platform.pathSeparator}$id-metadata.json');
  if (!await file.exists()) return SongInfo(id: id, title: id, origin: origin);
  final raw = await _readJson(file, warnings);
  if (raw is! Map) return SongInfo(id: id, title: id, origin: origin);
  final diffs = <String>[];
  final play = raw['playData'];
  if (play is Map && play['difficulties'] is List) {
    for (final d in play['difficulties'] as List) {
      final t = _text(d, 20);
      if (t != null && diffs.length < 8) diffs.add(t);
    }
  }
  double? bpm;
  final changes = raw['timeChanges'];
  if (changes is List && changes.isNotEmpty && changes.first is Map) {
    final b = (changes.first as Map)['bpm'];
    if (b is num) bpm = b.toDouble();
  }
  return SongInfo(
    id: id,
    title: _text(raw['songName'], 60) ?? id,
    artist: _text(raw['artist'], 60),
    difficulties: diffs,
    bpm: bpm,
    origin: origin,
  );
}

Future<void> _readVSliceData(
  Directory data,
  List<LevelInfo> levels,
  Map<String, SongInfo> songs,
  Set<String> seenLevels,
  List<String> warnings,
) async {
  final sep = Platform.pathSeparator;
  final origin = _originOf(data.parent);
  final levelsDir = Directory('${data.path}${sep}levels');
  if (await levelsDir.exists()) {
    final files = <File>[];
    await for (final e in levelsDir.list(followLinks: false)) {
      if (e is File && e.path.toLowerCase().endsWith('.json')) files.add(e);
    }
    files.sort((a, b) => a.path.compareTo(b.path));
    for (final f in files) {
      final level = await _readVSliceLevel(f, origin, warnings);
      if (level != null && seenLevels.add('$origin/${level.id}')) levels.add(level);
    }
  }
  final songsDir = Directory('${data.path}${sep}songs');
  if (await songsDir.exists()) {
    final dirs = <Directory>[];
    await for (final e in songsDir.list(followLinks: false)) {
      if (e is Directory) dirs.add(e);
      if (dirs.length >= _maxSongDirs) break;
    }
    dirs.sort((a, b) => a.path.compareTo(b.path));
    for (final d in dirs) {
      final id = _name(d);
      if (!_nameOk(id)) {
        warnings.add('Song folder $id has an unusual name and was skipped.');
        continue;
      }
      if (songs.containsKey(id)) continue;
      songs[id] = await _readVSliceSong(d, id, origin, warnings);
    }
  }
}

// ---------------------------------------------------------------------------
// Codename Engine
// ---------------------------------------------------------------------------

Future<List<String>> _readOrder(File file) async {
  try {
    if (!await file.exists() || await file.length() > _maxFileBytes) return const [];
    final lines = (await file.readAsString()).split(RegExp(r'\r?\n'));
    return [
      for (final l in lines)
        if (l.trim().isNotEmpty) l.trim().replaceFirst(RegExp(r'\.xml$', caseSensitive: false), ''),
    ];
  } on FormatException {
    return const [];
  } on FileSystemException {
    return const [];
  }
}

Future<LevelInfo?> _readCodenameWeek(File file, String origin, List<String> warnings) async {
  final text = await _readText(file, warnings);
  if (text == null) return null;
  final stem = _stem(file, 'xml');
  if (!_nameOk(stem)) {
    warnings.add('Week file ${_name(file)} has an unusual name and was skipped.');
    return null;
  }
  final xml = text.replaceAll(RegExp(r'<!--.*?-->', dotAll: true), '');
  final weekTag = RegExp(r'<week\b([^>]*)>', caseSensitive: false).firstMatch(xml);
  if (weekTag == null) {
    warnings.add('${_name(file)} has no <week> tag and was skipped.');
    return null;
  }
  final name = _text(_xmlAttr(weekTag.group(1) ?? '', 'name'), 60) ?? stem;
  final ids = <String>[];
  for (final m in RegExp(r'<song\b[^>]*>([^<]*)</song>', caseSensitive: false).allMatches(xml)) {
    final id = _decodeEntities(m.group(1) ?? '').trim();
    if (_nameOk(id) && !ids.contains(id)) ids.add(id);
  }
  return LevelInfo(id: stem, name: name, songIds: ids, origin: origin);
}

Future<List<String>> _chartNames(Directory songDir) async {
  final dir = Directory('${songDir.path}${Platform.pathSeparator}charts');
  if (!await dir.exists()) return const [];
  final names = <String>[];
  await for (final e in dir.list(followLinks: false)) {
    if (e is! File || !e.path.toLowerCase().endsWith('.json')) continue;
    final n = _text(_stem(e, 'json'), 20);
    if (n != null) names.add(n);
    if (names.length >= 8) break;
  }
  names.sort();
  return names;
}

Future<void> _readCodenameRoot(
  Directory root,
  List<LevelInfo> levels,
  Map<String, SongInfo> songs,
  Set<String> seenLevels,
  List<String> warnings,
) async {
  final sep = Platform.pathSeparator;
  final origin = _originOf(root);

  final weeksDir = Directory('${root.path}${sep}data${sep}weeks${sep}weeks');
  if (await weeksDir.exists()) {
    final files = <File>[];
    await for (final e in weeksDir.list(followLinks: false)) {
      if (e is File && e.path.toLowerCase().endsWith('.xml')) files.add(e);
    }
    final order = await _readOrder(File('${root.path}${sep}data${sep}weeks${sep}weeks.txt'));
    int rank(File f) {
      final i = order.indexOf(_stem(f, 'xml'));
      return i < 0 ? order.length : i;
    }

    files.sort((a, b) {
      final byRank = rank(a).compareTo(rank(b));
      return byRank != 0 ? byRank : a.path.compareTo(b.path);
    });
    for (final f in files) {
      final level = await _readCodenameWeek(f, origin, warnings);
      if (level != null && seenLevels.add('$origin/${level.id}')) levels.add(level);
    }
  }

  final songsDir = Directory('${root.path}${sep}songs');
  if (await songsDir.exists()) {
    final dirs = <Directory>[];
    await for (final e in songsDir.list(followLinks: false)) {
      if (e is Directory) dirs.add(e);
      if (dirs.length >= _maxSongDirs) break;
    }
    dirs.sort((a, b) => a.path.compareTo(b.path));
    for (final d in dirs) {
      final id = _name(d);
      if (songs.containsKey(id)) continue;
      final metaFile = File('${d.path}${sep}meta.json');
      final charts = await _chartNames(d);
      final hasMeta = await metaFile.exists();
      if (!hasMeta && charts.isEmpty) continue; // not a song folder
      if (!_nameOk(id)) {
        warnings.add('Song folder $id has an unusual name and was skipped.');
        continue;
      }
      Map<Object?, Object?>? meta;
      if (hasMeta) {
        final raw = await _readJson(metaFile, warnings);
        if (raw is Map) meta = raw;
      }
      final diffs = <String>[];
      final rawDiffs = meta?['difficulties'];
      if (rawDiffs is List) {
        for (final x in rawDiffs) {
          final t = _text(x, 20);
          if (t != null && diffs.length < 8) diffs.add(t);
        }
      }
      final bpmRaw = meta?['bpm'];
      songs[id] = SongInfo(
        id: id,
        title: _text(meta?['displayName'], 60) ?? _text(meta?['name'], 60) ?? id,
        difficulties: diffs.isNotEmpty ? diffs : charts,
        bpm: bpmRaw is num ? bpmRaw.toDouble() : null,
        origin: origin,
      );
    }
  }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

Future<ContentCatalog> scanContentFolder(Directory root) async {
  final warnings = <String>[];
  if (!await root.exists()) {
    return ContentCatalog(
      levels: const [],
      songs: const {},
      warnings: const ['That folder does not exist.'],
      sourcePath: root.path,
    );
  }
  final levels = <LevelInfo>[];
  final songs = <String, SongInfo>{};
  final seenLevels = <String>{};

  final roots = await _findRoots(root, warnings);
  for (final data in roots.vslice) {
    await _readVSliceData(data, levels, songs, seenLevels, warnings);
  }
  for (final r in roots.codename) {
    await _readCodenameRoot(r, levels, songs, seenLevels, warnings);
  }

  for (final level in levels) {
    for (final id in level.songIds) {
      songs.putIfAbsent(id, () => SongInfo(id: id, title: id, origin: level.origin));
    }
  }
  return ContentCatalog(
    levels: levels,
    songs: songs,
    warnings: warnings,
    sourcePath: root.path,
  );
}
