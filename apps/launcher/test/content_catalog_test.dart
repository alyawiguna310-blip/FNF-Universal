import 'dart:convert';
import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/content/content_catalog.dart';

void main() {
  Directory makeVSliceData(Directory tmp) {
    final data = Directory('${tmp.path}/assets/data');
    Directory('${data.path}/levels').createSync(recursive: true);
    Directory('${data.path}/songs/alpha').createSync(recursive: true);
    Directory('${data.path}/songs/orphan').createSync(recursive: true);
    return data;
  }

  test('reads levels and song metadata from a V-Slice style folder', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final data = makeVSliceData(tmp);
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
    expect(
      catalog.freeplaySongs.map((s) => s.title).toList(),
      ['Alpha Song', 'beta', 'gamma', 'Orphan Tune'],
    );
  });

  test('reads a Codename Engine mod: weeks xml, weeks.txt order, songs meta.json', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final mod = '${tmp.path}/mods/cool-mod';
    Directory('$mod/data/weeks/weeks').createSync(recursive: true);
    Directory('$mod/songs/alpha/charts').createSync(recursive: true);
    Directory('$mod/songs/free/charts').createSync(recursive: true);
    Directory('$mod/songs/not a song').createSync(recursive: true); // no meta, no charts

    File('$mod/data/weeks/weeks/week-a.xml').writeAsStringSync(
      '<!DOCTYPE codename-engine-week>\n'
      '<!-- <song>commented-out</song> -->\n'
      '<week name="Rock &amp; Roll" chars="dad,bf,gf">\n'
      '  <song>alpha</song>\n'
      '  <song>beta</song>\n'
      '  <difficulty name="easy"/>\n'
      '</week>',
    );
    File('$mod/data/weeks/weeks/week-b.xml')
        .writeAsStringSync('<week name="Second"><song>free</song></week>');
    File('$mod/data/weeks/weeks.txt').writeAsStringSync('week-b\nweek-a\n');
    File('$mod/songs/alpha/meta.json').writeAsStringSync(jsonEncode({
      'name': 'alpha',
      'displayName': 'Alpha Song',
      'bpm': 120,
      'difficulties': ['easy', 'hard'],
    }));
    File('$mod/songs/alpha/charts/easy.json').writeAsStringSync('{}');
    File('$mod/songs/free/meta.json').writeAsStringSync(jsonEncode({'name': 'free', 'bpm': 90}));
    File('$mod/songs/free/charts/normal.json').writeAsStringSync('{}');
    File('$mod/songs/free/charts/hard.json').writeAsStringSync('{}');

    final catalog = await scanContentFolder(tmp);

    // weeks.txt puts week-b first.
    expect(catalog.levels.map((l) => l.id).toList(), ['week-b', 'week-a']);
    final a = catalog.levels.firstWhere((l) => l.id == 'week-a');
    expect(a.name, 'Rock & Roll');
    expect(a.songIds, ['alpha', 'beta']); // the commented-out song is ignored
    expect(a.origin, 'cool-mod');

    final alpha = catalog.songs['alpha']!;
    expect(alpha.title, 'Alpha Song');
    expect(alpha.bpm, 120);
    expect(alpha.difficulties, ['easy', 'hard']); // meta.json wins over chart names
    expect(alpha.origin, 'cool-mod');
    // No difficulties in meta: only the chart file names are listed.
    expect(catalog.songs['free']!.difficulties, ['hard', 'normal']);
    expect(catalog.songs.containsKey('not a song'), isFalse);
    expect(catalog.songs['beta']!.title, 'beta'); // listed by a week, no folder
  });

  test('hostile week xml: entities are never expanded and bad ids are dropped', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final mod = '${tmp.path}/mod';
    Directory('$mod/data/weeks/weeks').createSync(recursive: true);
    File('$mod/data/weeks/weeks/evil.xml').writeAsStringSync(
      '<!DOCTYPE week [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>\n'
      '<week name="Evil">\n'
      '  <song>&xxe;</song>\n'
      '  <song>../../escape</song>\n'
      '  <song>fine-song</song>\n'
      '</week>',
    );
    final catalog = await scanContentFolder(tmp);
    expect(catalog.levels.single.songIds, ['fine-song']);
  });

  test('both layouts in one folder are listed together', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final vs = '${tmp.path}/vs-mod/data';
    Directory('$vs/levels').createSync(recursive: true);
    File('$vs/levels/w.json').writeAsStringSync(jsonEncode({'name': 'VS', 'songs': ['one']}));
    final cn = '${tmp.path}/mods/cn-mod';
    Directory('$cn/songs/two').createSync(recursive: true);
    File('$cn/songs/two/meta.json').writeAsStringSync(jsonEncode({'name': 'two'}));

    final catalog = await scanContentFolder(tmp);
    expect(catalog.songs.keys.toSet(), {'one', 'two'});
    expect(catalog.songs['one']!.origin, 'vs-mod');
    expect(catalog.songs['two']!.origin, 'cn-mod');
  });

  test('bad or oversized files are skipped with a warning and never crash', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final data = makeVSliceData(tmp);
    File('${data.path}/levels/broken.json').writeAsStringSync('{ not json');
    File('${data.path}/levels/huge.json')
        .writeAsStringSync('x' * (1024 * 1024 + 10));
    File('${data.path}/levels/ok.json')
        .writeAsStringSync(jsonEncode({'name': 'OK', 'songs': []}));

    final catalog = await scanContentFolder(tmp);

    expect(catalog.levels.map((l) => l.id).toList(), ['ok']);
    expect(catalog.warnings.length, greaterThanOrEqualTo(2));
  });

  test('unusual song ids are ignored', () async {
    final tmp = Directory.systemTemp.createTempSync('fnfcontent');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final data = makeVSliceData(tmp);
    File('${data.path}/levels/week.json').writeAsStringSync(jsonEncode({
      'name': 'W',
      'songs': ['fine', '../evil', 'a/b', 'a:b', 'ok-1', 'with space'],
    }));
    final catalog = await scanContentFolder(tmp);
    expect(catalog.levels.single.songIds, ['fine', 'ok-1', 'with space']);
  });

  test('a missing folder gives an empty catalog with a warning', () async {
    final catalog = await scanContentFolder(Directory('${Directory.systemTemp.path}/does_not_exist_fnf'));
    expect(catalog.isEmpty, isTrue);
    expect(catalog.warnings, isNotEmpty);
  });
}
