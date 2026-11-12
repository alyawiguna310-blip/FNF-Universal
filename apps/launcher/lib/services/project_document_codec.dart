/// Adapts the editor controls to the canonical Rust project schema.
///
/// This is a UI boundary adapter only. Project validation and canonical
/// serialization remain owned by the Rust project crate.
class ProjectDocumentCodec {
  static Map<String, dynamic> fromEditor({
    required String projectName,
    required String targetEngine,
    required String songName,
    required double bpm,
    required Set<String> notes,
    required String characterName,
    required String characterDescription,
    required List<String> animations,
    required String stageName,
    required List<String> stageObjects,
    required Map<String, bool> layers,
  }) {
    final projectId = _id(projectName, fallback: 'untitled-mod');
    final safeBpm = bpm.isFinite && bpm > 0 ? bpm : 120.0;
    final stepDurationMs = 60000 / safeBpm / 4;
    final sortedNotes = notes.toList()
      ..sort((a, b) {
        final aa = a.split(':').map(int.parse).toList();
        final bb = b.split(':').map(int.parse).toList();
        final byStep = aa[1].compareTo(bb[1]);
        return byStep != 0 ? byStep : aa[0].compareTo(bb[0]);
      });

    return {
      'format': 'fnf-universal-project',
      'formatVersion': 1,
      'project': {
        'id': projectId,
        'name': projectName.trim().isEmpty ? 'Untitled Mod' : projectName.trim(),
        'targetEngine': targetEngine,
      },
      'songs': [
        {
          'id': '${projectId}_song',
          'title': songName.trim().isEmpty ? 'Untitled Song' : songName.trim(),
          'bpm': safeBpm,
          'laneCount': 4,
          'notes': sortedNotes.asMap().entries.map((entry) {
            final parts = entry.value.split(':').map(int.parse).toList();
            return {
              'id': 'note-${entry.key + 1}',
              'gameplay': {
                'lane': parts[0],
                'strumTimeMs': (parts[1] * stepDurationMs).round(),
                'sustainMs': 0,
              },
              'visual': null,
            };
          }).toList(),
        },
      ],
      'characters': [
        {
          'id': _id(characterName, fallback: 'character'),
          'name': characterName.trim().isEmpty ? 'Character' : characterName.trim(),
          'description': characterDescription,
          'animations': List<String>.from(animations),
        },
      ],
      'stages': [
        {
          'id': _id(stageName, fallback: 'stage'),
          'name': stageName.trim().isEmpty ? 'Stage' : stageName.trim(),
          'objects': stageObjects.asMap().entries.map((entry) {
            final objectName = entry.value;
            return {
              'id': '${_id(objectName, fallback: 'object')}-${entry.key + 1}',
              'name': objectName,
              'visible': layers[objectName] ?? true,
              'x': 0.0,
              'y': 0.0,
            };
          }).toList(),
        },
      ],
      'assets': <Map<String, dynamic>>[],
      'modSettings': <Map<String, dynamic>>[],
    };
  }

  /// Migrates the editor prototype's original chart/character/stage layout.
  /// Existing v0.1 local project files therefore remain loadable.
  static Map<String, dynamic> migrateLegacy(Map<String, dynamic> legacy) {
    final project = _asMap(legacy['project']);
    final chart = _asMap(legacy['chart']);
    final character = _asMap(legacy['character']);
    final stage = _asMap(legacy['stage']);
    final notes = <String>{};

    for (final rawNote in _asList(chart['notes'])) {
      final note = _asMap(rawNote);
      final lane = note['lane'];
      final step = note['step'];
      if (lane is int && step is int &&
          lane >= 0 && lane < 4 && step >= 0 && step < 16) {
        notes.add('$lane:$step');
      }
    }

    final layers = <String, bool>{};
    for (final entry in _asMap(stage['layers']).entries) {
      if (entry.value is bool) {
        layers[entry.key] = entry.value as bool;
      }
    }

    final rawBpm = chart['bpm'];
    return fromEditor(
      projectName: project['name']?.toString() ?? 'Untitled Mod',
      targetEngine: project['targetEngine']?.toString() ?? 'Engine-independent',
      songName: chart['song']?.toString() ?? 'Untitled Song',
      bpm: rawBpm is num ? rawBpm.toDouble() : 120.0,
      notes: notes,
      characterName: character['name']?.toString() ?? 'character',
      characterDescription: character['description']?.toString() ?? '',
      animations: _asList(character['animations']).map((value) => value.toString()).toList(),
      stageName: stage['name']?.toString() ?? 'stage',
      stageObjects: _asList(stage['objects']).map((value) => value.toString()).toList(),
      layers: layers,
    );
  }

  /// Converts canonical Rust JSON to the existing editor widget state shape.
  static Map<String, dynamic> toEditorState(Map<String, dynamic> document) {
    if (document['format'] != 'fnf-universal-project' ||
        document['formatVersion'] != 1) {
      throw const FormatException('Unsupported FNF Universal project format.');
    }

    final project = _asMap(document['project']);
    final songs = _asList(document['songs']);
    final song = songs.isEmpty ? <String, dynamic>{} : _asMap(songs.first);
    final characters = _asList(document['characters']);
    final character = characters.isEmpty ? <String, dynamic>{} : _asMap(characters.first);
    final stages = _asList(document['stages']);
    final stage = stages.isEmpty ? <String, dynamic>{} : _asMap(stages.first);

    final rawBpm = song['bpm'];
    final bpm = rawBpm is num && rawBpm.isFinite && rawBpm > 0
        ? rawBpm.toDouble()
        : 120.0;
    final stepDurationMs = 60000 / bpm / 4;
    final loadedNotes = <Map<String, dynamic>>[];

    for (final rawNote in _asList(song['notes'])) {
      final note = _asMap(rawNote);
      final gameplay = _asMap(note['gameplay']);
      final lane = gameplay['lane'];
      final time = gameplay['strumTimeMs'];
      if (lane is int && time is num && lane >= 0 && lane < 4) {
        final step = (time / stepDurationMs).round();
        if (step >= 0 && step < 16) {
          loadedNotes.add({'lane': lane, 'step': step});
        }
      }
    }

    final stageObjects = <String>[];
    final stageLayers = <String, bool>{};
    for (final rawObject in _asList(stage['objects'])) {
      final object = _asMap(rawObject);
      final name = object['name']?.toString() ?? object['id']?.toString() ?? 'Object ${stageObjects.length + 1}';
      stageObjects.add(name);
      stageLayers[name] = object['visible'] is bool ? object['visible'] as bool : true;
    }

    return {
      'project': {
        'id': project['id']?.toString() ?? 'untitled-mod',
        'name': project['name']?.toString() ?? 'Untitled Mod',
        'targetEngine': project['targetEngine']?.toString() ?? 'Engine-independent',
      },
      'chart': {
        'song': song['title']?.toString() ?? 'Untitled Song',
        'bpm': bpm,
        'notes': loadedNotes,
      },
      'character': {
        'name': character['name']?.toString() ?? 'character',
        'description': character['description']?.toString() ?? '',
        'animations': _asList(character['animations']).map((value) => value.toString()).toList(),
      },
      'stage': {
        'name': stage['name']?.toString() ?? 'stage',
        'objects': stageObjects,
        'layers': stageLayers,
      },
    };
  }

  static String _id(String value, {required String fallback}) {
    final cleaned = value
        .trim()
        .toLowerCase()
        .replaceAll(RegExp(r'[^a-z0-9_-]+'), '-')
        .replaceAll(RegExp(r'^-+|-+$'), '');
    return cleaned.isEmpty ? fallback : cleaned;
  }

  static Map<String, dynamic> _asMap(dynamic value) =>
      value is Map<String, dynamic> ? value : <String, dynamic>{};

  static List<dynamic> _asList(dynamic value) =>
      value is List ? value : <dynamic>[];
}
