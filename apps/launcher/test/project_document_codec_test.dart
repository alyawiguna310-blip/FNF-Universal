import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/services/project_document_codec.dart';

void main() {
  group('ProjectDocumentCodec', () {
    test('creates the canonical Rust project envelope', () {
      final document = ProjectDocumentCodec.fromEditor(
        projectName: 'Demo Mod',
        targetEngine: 'Engine-independent',
        songName: 'Tutorial Remix',
        bpm: 120,
        notes: {'0:0', '2:3'},
        characterName: 'bf-custom',
        characterDescription: 'A custom character.',
        animations: ['idle', 'singLEFT'],
        stageName: 'my-stage',
        stageObjects: ['Background', 'Opponent'],
        layers: {'Background': true, 'Opponent': false},
      );

      expect(document['format'], 'fnf-universal-project');
      expect(document['formatVersion'], 1);
      expect((document['project'] as Map)['id'], 'demo-mod');

      final song = (document['songs'] as List).single as Map;
      expect(song['laneCount'], 4);
      final notes = song['notes'] as List;
      expect(notes, hasLength(2));
      expect((notes.first as Map)['gameplay'], containsPair('lane', 0));
      expect((notes.first as Map)['visual'], isNull);

      final character = (document['characters'] as List).single as Map;
      expect(character['description'], 'A custom character.');

      final stage = (document['stages'] as List).single as Map;
      final objects = stage['objects'] as List;
      expect((objects[1] as Map)['visible'], false);
      expect((objects[1] as Map)['name'], 'Opponent');
    });

    test('round-trips canonical documents to editor state', () {
      final document = ProjectDocumentCodec.fromEditor(
        projectName: 'Demo',
        targetEngine: 'V-Slice',
        songName: 'Song',
        bpm: 120,
        notes: {'1:4'},
        characterName: 'bf',
        characterDescription: 'Playable character',
        animations: ['idle'],
        stageName: 'school',
        stageObjects: ['Background'],
        layers: {'Background': true},
      );

      final state = ProjectDocumentCodec.toEditorState(document);
      expect((state['project'] as Map)['name'], 'Demo');
      expect((state['chart'] as Map)['bpm'], 120.0);
      expect((state['chart'] as Map)['notes'], [
        {'lane': 1, 'step': 4},
      ]);
      expect((state['character'] as Map)['description'], 'Playable character');
      expect((state['stage'] as Map)['objects'], ['Background']);
    });

    test('migrates the original editor prototype format', () {
      final legacy = {
        'format': 'fnf-universal-project',
        'formatVersion': 1,
        'project': {'name': 'Old Mod', 'targetEngine': 'Psych Engine'},
        'chart': {
          'song': 'Old Song',
          'bpm': 100,
          'notes': [
            {'lane': 3, 'step': 2},
          ],
        },
        'character': {
          'name': 'old-bf',
          'description': 'Legacy description',
          'animations': ['idle'],
        },
        'stage': {
          'name': 'old-stage',
          'objects': ['Backdrop'],
          'layers': {'Backdrop': false},
        },
      };

      final migrated = ProjectDocumentCodec.migrateLegacy(legacy);
      expect(migrated['songs'], hasLength(1));
      expect(migrated['characters'], hasLength(1));
      expect(migrated['stages'], hasLength(1));
      expect(
        ((migrated['characters'] as List).single as Map)['description'],
        'Legacy description',
      );
    });
  });
}
