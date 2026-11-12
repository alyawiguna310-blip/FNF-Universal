import 'dart:math' as math;

/// Adapts the editor's current controls to the canonical Rust project schema.
///
/// This is a boundary adapter only; validation and canonical serialization
/// remain owned by the Rust project crate.
class ProjectDocumentCodec {
  static Map<String, dynamic> fromEditor({
    required String projectName,
    required String targetEngine,
    required String songName,
    required double bpm,
    required Set<String> notes,
    required String characterName,
    required List<String> animations,
    required String stageName,
    required List<String> stageObjects,
    required Map<String, bool> layers,
  }) {
    final projectId = _id(projectName, fallback: 'untitled-mod');
    final safeBpm = bpm.isFinite && bpm > 0 ? bpm : 120.0;
    final stepDurationMs = 60000 / safeBpm / 4;

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
          'notes': notes.toList()
            ..sort((a, b) {
              final aa = a.split(':').map(int.parse).toList();
              final bb = b.split(':').map(int.parse).toList();
              final byStep = aa[1].compareTo(bb[1]);
              return byStep != 0 ? byStep : aa[0].compareTo(bb[0]);
            })
            .asMap()
            .entries
            .map((entry) {
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
            })
            .toList(),
        },
      ],
      'characters': [
        {
          'id': _id(characterName, fallback: 'character'),
          'name': characterName.trim().isEmpty ? 'Character' : characterName.trim(),
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

  /// Converts canonical Rust JSON back into the editor's existing control
  /// state shape, allowing old editor widgets to be migrated incrementally.
  static Map<String, dynamic> toEditorState(Map<String, dynamic> document) {
    if (document['format'] != 'fnf-universal-project' ||
        document['formatVersion'] != 1) {
      throw const FormatException('Unsupported FNF Universal project format.');
    }

    final project = _asMap(document['project']);
    final songs = _asList(document['songs']);
    final song = songs.isEmpty ? <String, dynamic>{} : _asMap(songs.first);
    final characters = _asList(document['characters']);
    final character =
        characters.isEmpty ? <String, dynamic>{} : _asMap(characters.first);
    final stages = _asList(document['stages']);
    final stage = stages.isEmpty ? <String, dynamic>{} : _asMap(stages.first);

    final bpmValue = song['bpm'];
    final bpm = bpmValue is num && bpmValue.isFinite && bpmValue > 0
        ? bpmValue.toDouble()
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
      final id = object['id']?.toString() ?? 'object-${stageObjects.length + 1}';
      stageObjects.add(id);
      stageLayers[id] = object['visible'] is bool ? object['visible'] as bool : true;
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
        'description': '',
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
    final result = value.trim().toLowerCase().replaceAll(RegExp(r'[^a-z0-9_-]+'), '-');
    return result.replaceAll(RegExp(r'^-+|-+$'), '').isEmpty
        ? fallback
        : result.replaceAll(RegExp(r'^-+|-+$'), '');
  }

  static Map<String, dynamic> _asMap(dynamic value) =>
      value is Map<String, dynamic> ? value : <String, dynamic>{};

  static List<dynamic> _asList(dynamic value) =>
      value is List ? value : <dynamic>[];
}
