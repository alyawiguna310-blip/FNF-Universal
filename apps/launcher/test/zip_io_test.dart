import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/export/zip_io.dart';

Uint8List _b(String s) => Uint8List.fromList(utf8.encode(s));

void main() {
  test('round trips stored and deflated entries', () {
    final big = _b('a' * 10000);
    final zip = buildZip([
      ZipEntryData('a.txt', _b('hello')),
      ZipEntryData('dir/\u00fcn\u00ef.lua', big),
      ZipEntryData('empty.txt', Uint8List(0)),
    ]);
    final entries = listZipEntries(zip);
    expect(entries.map((e) => e.path).toList(), ['a.txt', 'dir/\u00fcn\u00ef.lua', 'empty.txt']);
    expect(entries[0].method, 0);
    expect(entries[1].method, 8);
    expect(readZipEntry(zip, entries[0]), _b('hello'));
    expect(readZipEntry(zip, entries[1]), big);
    expect(readZipEntry(zip, entries[2]), isEmpty);
  });

  test('detects a damaged entry through its checksum', () {
    final zip = buildZip([ZipEntryData('a.txt', _b('hello'))]);
    final entries = listZipEntries(zip);
    zip[35] ^= 0xFF; // first data byte: 30 header + 5 name bytes
    expect(
      () => readZipEntry(zip, entries.first),
      throwsA(isA<ZipFormatException>()),
    );
  });

  test('rejects data that is not a zip', () {
    expect(
      () => listZipEntries(Uint8List.fromList(List.filled(100, 7))),
      throwsA(isA<ZipFormatException>()),
    );
  });
}
