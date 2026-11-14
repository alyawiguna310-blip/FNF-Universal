import 'dart:convert';
import 'dart:io';

/// Read-only catalog of weeks ("levels") and songs found in a folder the player
/// points at: their own copy of the game or a mod folder. Only names, artist,
/// BPM and difficulties are read from small JSON files. No audio, art, charts
/// or scripts are read, copied or run.
///
/// Folder layout follows the Funkin' Modding Docs (CC BY-SA 4.0):
///   data/levels/<level>.json                  name, songs, visible
///   data/songs/<id>/<id>-metadata.json        songName, artist, playData, timeChanges
/// This is not yet verified against real builds (see TESTING.md).
const int _maxJsonBytes = 1024 * 1024;
const int _maxVisited = 20000;
const int _maxDepth = 7;

final RegExp _idPattern = RegExp(r'^[A-Za-z0-9_-]{1,64}$');
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
  });

  final String id;
  final String title;
  final String? artist;
  final List<String> difficulties;
  final double? bpm;
}

class LevelInfo {
  const LevelInfo({
    required this.id,
    required this.name,
    required this.songIds,
    this.visible = true,
  });

  final String id;
  final String name;
  final List<String> songIds;
  final bool visible;
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

String _name(FileSystemEntity e) =>
    e.uri.pathSegments.where((s) => s.isNotEmpty).last;

String? _text(Object? raw, int max) {
  if (raw is! String) return null;
  final cleaned = raw.replaceAll(RegExp(r'[\u0000-\u001F\u007F]'), ' ').trim();
  if (cleaned.isEmpty) return null;
  return cleaned.length > max ? cleaned.substring(0, max) : cleaned;
}

Future<Object?> _readJson(File file, List<String> warnings) async {
  try {
    if (await file.length() > _maxJsonBytes) {
      warnings.add('${_name(file)} is too large and was skipped.');
      return null;
    }
    return jsonDecode(await file.readAsString());
  } on FormatException {
    warnings.add('${_name(file)} is not valid JSON and was skipped.');
    return null;
  } on FileSystemException {
    warnings.add('Could not read ${_name(file)}.');
    return null;
  }
}

Future<List<Directory>> _findDataDirs(Directory root, List<String> warnings) async {
  final sep = Platform.pathSeparator;
  final found = <Directory>[];
  var visited = 0;
  var stopped = false;

  Future<bool> looksLikeData(Directory d) async =>
      await Directory('${d.path}${sep}levels').exists() ||
      await Directory('${d.path}${sep}songs').exists();

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
      if (name == 'data' && await looksLikeData(e)) {
        found.add(e);
        continue;
      }
      await walk(e, depth + 1);
    }
  }

  if (await looksLikeData(root)) {
    found.add(root); // the player pointed straight at a data folder
  } else {
    await walk(root, 0);
  }
  return found;
}

Future<LevelInfo?> _readLevel(File file, List<String> warnings) async {
  final raw = await _readJson(file, warnings);
  if (raw is! Map) return null;
  final stem = _name(file).replaceFirst(RegExp(r'\.json$', caseSensitive: false), '');
  if (!_idPattern.hasMatch(stem)) {
    warnings.add('Level file ${_name(file)} has an unusual name and was skipped.');
    return null;
  }
  final ids = <String>[];
  final songsRaw = raw['songs'];
  if (songsRaw is List) {
    for (final s in songsRaw) {
      if (s is String && _idPattern.hasMatch(s)) ids.add(s);
    }
  }
  return LevelInfo(
    id: stem,
    name: _text(raw['name'], 60) ?? stem,
    songIds: ids,
    visible: raw['visible'] is bool ? raw['visible'] as bool : true,
  );
}

Future<SongInfo> _readSong(Directory dir, String id, List<String> warnings) async {
  final file = File('${dir.path}${Platform.pathSeparator}$id-metadata.json');
  if (!await file.exists()) return SongInfo(id: id, title: id);
  final raw = await _readJson(file, warnings);
  if (raw is! Map) return SongInfo(id: id, title: id);
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
  );
}

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
  final sep = Platform.pathSeparator;
  final levels = <LevelInfo>[];
  final songs = <String, SongInfo>{};
  final seenLevels = <String>{};

  for (final data in await _findDataDirs(root, warnings)) {
    final levelsDir = Directory('${data.path}${sep}levels');
    if (await levelsDir.exists()) {
      final files = <File>[];
      await for (final e in levelsDir.list(followLinks: false)) {
        if (e is File && e.path.toLowerCase().endsWith('.json')) files.add(e);
      }
      files.sort((a, b) => a.path.compareTo(b.path));
      for (final f in files) {
        final level = await _readLevel(f, warnings);
        if (level != null && seenLevels.add(level.id)) levels.add(level);
      }
    }
    final songsDir = Directory('${data.path}${sep}songs');
    if (await songsDir.exists()) {
      final dirs = <Directory>[];
      await for (final e in songsDir.list(followLinks: false)) {
        if (e is Directory) dirs.add(e);
      }
      dirs.sort((a, b) => a.path.compareTo(b.path));
      for (final d in dirs) {
        final id = _name(d);
        if (!_idPattern.hasMatch(id)) {
          warnings.add('Song folder $id has an unusual name and was skipped.');
          continue;
        }
        if (songs.containsKey(id)) continue;
        songs[id] = await _readSong(d, id, warnings);
      }
    }
  }

  for (final level in levels) {
    for (final id in level.songIds) {
      songs.putIfAbsent(id, () => SongInfo(id: id, title: id));
    }
  }
  return ContentCatalog(
    levels: levels,
    songs: songs,
    warnings: warnings,
    sourcePath: root.path,
  );
}
