import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/export/mod_package.dart';
import 'package:fnf_universal/export/zip_io.dart';
import 'package:fnf_universal/ui/menu_definition.dart';

Uint8List _b(String s) => Uint8List.fromList(utf8.encode(s));

void main() {
  test('safeRelativePath rejects traversal, absolute and reserved paths', () {
    for (final bad in [
      '../x.lua',
      'a/../../x',
      '/etc/passwd',
      r'C:\Windows\x',
      'a/./b',
      'CON.txt',
      'dir/NUL',
      'a:b',
      'x ',
      '',
    ]) {
      expect(safeRelativePath(bad), isNull, reason: bad);
    }
    expect(safeRelativePath('scripts/a/b.lua'), 'scripts/a/b.lua');
    expect(safeRelativePath(r'scripts\a\b.lua'), 'scripts/a/b.lua');
  });

  test('a mod package round trips scripts, project and UI', () async {
    final ui = LauncherUiDefinition.defaults().copyWith(wordmarkMain: 'My Mod');
    final zip = buildModPackage(
      kind: ModPackageKind.mod,
      name: 'My Mod',
      ui: ui,
      projectJson: '{"songs":[]}',
      scripts: {'main.lua': _b('print(1)'), 'sub/x.hx': _b('trace(1);')},
    );
    final report = inspectModPackage(zip);
    expect(report.canImport, isTrue);
    expect(report.scriptCount, 2);
    expect(report.count(ReportStatus.unsupported), 0);
    expect(report.ui!.definition.wordmarkMain, 'My Mod');

    final tmp = Directory.systemTemp.createTempSync('fnfmod_test');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final result = await importModPackage(zip, tmp);
    expect(File('${result.directory.path}/scripts/main.lua').readAsStringSync(), 'print(1)');
    expect(File('${result.directory.path}/scripts/sub/x.hx').readAsStringSync(), 'trace(1);');
    expect(File('${result.directory.path}/launcher_ui.json').existsSync(), isTrue);
  });

  test('unsafe paths and executables are reported and never extracted', () async {
    final zip = buildZip([
      ZipEntryData('fnfmod.json', _b('{"format":1,"kind":"mod","name":"evil","createdUtc":"2026-11-13T00:00:00Z"}')),
      ZipEntryData('../evil.lua', _b('os.execute("bad")')),
      ZipEntryData('scripts/run.exe', _b('MZ')),
      ZipEntryData('scripts/ok.lua', _b('print(2)')),
    ]);
    final report = inspectModPackage(zip);
    expect(report.canImport, isTrue);
    expect(report.items.firstWhere((i) => i.path == '../evil.lua').status, ReportStatus.unsupported);
    expect(report.items.firstWhere((i) => i.path == 'scripts/run.exe').status, ReportStatus.unsupported);
    expect(report.items.firstWhere((i) => i.path == 'scripts/ok.lua').status, ReportStatus.needsAttention);

    final tmp = Directory.systemTemp.createTempSync('fnfmod_test');
    addTearDown(() => tmp.deleteSync(recursive: true));
    final result = await importModPackage(zip, tmp);
    expect(File('${result.directory.path}/scripts/ok.lua').existsSync(), isTrue);
    expect(File('${result.directory.path}/scripts/run.exe').existsSync(), isFalse);
    expect(File('${tmp.path}/evil.lua').existsSync(), isFalse);
    expect(File('${result.directory.path}/../evil.lua').existsSync(), isFalse);
  });

  test('files without a manifest are not packages', () {
    final zip = buildZip([ZipEntryData('readme.txt', _b('hi'))]);
    final report = inspectModPackage(zip);
    expect(report.canImport, isFalse);
    expect(report.errors, isNotEmpty);
  });
}
