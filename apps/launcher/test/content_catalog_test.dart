import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/content/content_catalog.dart';

void main() {
  Directory makeData(Directory tmp) {
    final data = Directory('${tmp.path}/assets/data');
    Directory('${data.path}/levels').createSync(recursive: true);
    Directory('${data.path}/songs/alpha').createSync(recursive: true);
    Directory('${data.path}/songs/orphan').createSync(recursive: true);
    return data;
  }

  test('reads levels and song metadata from a levels/songs folder', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final data = makeData(tmp);
    File('${data.path}/levels/week-1.json').writeAsStringSync(jsonEncode({
      'version': '1.0.0',
      'name': 'TEST WEEK',
      'songs': ['alpha', 'beta'],
      'visible': true,
    }));
    File('${data.path}/levels/week-2.json').writeAsStringSync(jsonEncode({
      'name': 'HIDDEN WEEK',
      'songs': ['gamma'],
      'visible': false,
    }));
    File('${data.path}/songs/alpha/alpha-metadata.json').writeAsStringSync(jsonEncode({
      'songName': 'Alpha Song',
      'artist': 'Someone',
      'playData': {
        'difficulties': ['easy', 'hard'],
      },
      'timeChanges': [
        {'t': 0, 'bpm': 120},
      ],
    }));
    File('${data.path}/songs/orphan/orphan-metadata.json')
        .writeAsStringSync(jsonEncode({'songName': 'Orphan Tune'}));

    final catalog = await scanContentFolder(tmp);

    expect(catalog.levels.map((l) => l.id).toList(), ['week-1', 'week-2']);
    expect(catalog.storyLevels.map((l) => l.id).toList(), ['week-1']);
    final alpha = catalog.songs['alpha']!;
    expect(alpha.title, 'Alpha Song');
    expect(alpha.artist, 'Someone');
    expect(alpha.difficulties, ['easy', 'hard']);
    expect(alpha.bpm, 120);
    // Level order first (hidden levels still feed Freeplay), then songs in no level.
    expect(
      catalog.freeplaySongs.map((s) => s.title).toList(),
      ['Alpha Song', 'beta', 'gamma', 'Orphan Tune'],
    );
  });

  test('bad or oversized JSON is skipped with a warning and never crashes', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final data = makeData(tmp);
    File('${data.path}/levels/broken.json').writeAsStringSync('{ not json');
    File('${data.path}/levels/huge.json')
        .writeAsStringSync('x' * (1024 * 1024 + 10));
    File('${data.path}/levels/ok.json')
        .writeAsStringSync(jsonEncode({'name': 'OK', 'songs': []}));

    final catalog = await scanContentFolder(tmp);

    expect(catalog.levels.map((l) => l.id).toList(), ['ok']);
    expect(catalog.warnings.length, greaterThanOrEqualTo(2));
  });

  test('unusual song ids and level names are ignored', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final data = makeData(tmp);
    File('${data.path}/levels/week.json').writeAsStringSync(jsonEncode({
      'name': 'W',
      'songs': ['fine', '../evil', 'a b', 'ok-1'],
    }));
    final catalog = await scanContentFolder(tmp);
    expect(catalog.levels.single.songIds, ['fine', 'ok-1']);
  });

  test('a missing folder gives an empty catalog with a warning', () async {
    final catalog = await scanContentFolder(Directory('${Directory.systemTemp.path}/does_not_exist_fnf'));
    expect(catalog.isEmpty, isTrue);
    expect(catalog.warnings, isNotEmpty);
  });
}
