import 'dart:convert';
import 'dart:io' show ZLibCodec, ZLibDecoder;
import 'dart:typed_data';

/// Minimal, dependency-free zip reader/writer.
///
/// Writer: stored or deflate, UTF-8 names. Reader: stored or deflate only;
/// rejects encryption and Zip64, caps entry count/sizes, and verifies sizes
/// and CRC. It never touches the file system.
class ZipFormatException implements Exception {
  const ZipFormatException(this.message);

  final String message;

  @override
  String toString() => 'ZipFormatException: $message';
}

class ZipLimits {
  const ZipLimits({
    this.maxEntries = 2000,
    this.maxEntryBytes = 32 * 1024 * 1024,
    this.maxTotalBytes = 256 * 1024 * 1024,
  });

  final int maxEntries;
  final int maxEntryBytes;
  final int maxTotalBytes;
}

class ZipEntryData {
  const ZipEntryData(this.path, this.bytes);

  final String path;
  final Uint8List bytes;
}

class ZipEntryInfo {
  const ZipEntryInfo({
    required this.path,
    required this.method,
    required this.crc,
    required this.compressedSize,
    required this.uncompressedSize,
    required this.localOffset,
  });

  final String path;
  final int method;
  final int crc;
  final int compressedSize;
  final int uncompressedSize;
  final int localOffset;

  bool get isDirectory => path.endsWith('/');
}

final List<int> _crcTable = List<int>.generate(256, (n) {
  var c = n;
  for (var k = 0; k < 8; k++) {
    c = (c & 1) != 0 ? 0xEDB88320 ^ (c >> 1) : c >> 1;
  }
  return c;
});

int crc32(List<int> data) {
  var c = 0xFFFFFFFF;
  for (final b in data) {
    c = _crcTable[(c ^ b) & 0xFF] ^ (c >> 8);
  }
  return c ^ 0xFFFFFFFF;
}

Uint8List buildZip(List<ZipEntryData> entries) {
  final out = BytesBuilder(copy: false);
  final central = BytesBuilder(copy: false);
  var offset = 0;
  for (final e in entries) {
    final name = utf8.encode(e.path);
    final crc = crc32(e.bytes);
    var method = 0;
    Uint8List data = e.bytes;
    if (e.bytes.length > 64) {
      final deflated = Uint8List.fromList(ZLibCodec(raw: true).encode(e.bytes));
      if (deflated.length < e.bytes.length) {
        method = 8;
        data = deflated;
      }
    }

    final local = ByteData(30)
      ..setUint32(0, 0x04034b50, Endian.little)
      ..setUint16(4, 20, Endian.little)
      ..setUint16(6, 0x0800, Endian.little)
      ..setUint16(8, method, Endian.little)
      ..setUint16(10, 0, Endian.little)
      ..setUint16(12, 0x0021, Endian.little)
      ..setUint32(14, crc, Endian.little)
      ..setUint32(18, data.length, Endian.little)
      ..setUint32(22, e.bytes.length, Endian.little)
      ..setUint16(26, name.length, Endian.little)
      ..setUint16(28, 0, Endian.little);
    out.add(local.buffer.asUint8List());
    out.add(name);
    out.add(data);

    final cd = ByteData(46)
      ..setUint32(0, 0x02014b50, Endian.little)
      ..setUint16(4, 20, Endian.little)
      ..setUint16(6, 20, Endian.little)
      ..setUint16(8, 0x0800, Endian.little)
      ..setUint16(10, method, Endian.little)
      ..setUint16(12, 0, Endian.little)
      ..setUint16(14, 0x0021, Endian.little)
      ..setUint32(16, crc, Endian.little)
      ..setUint32(20, data.length, Endian.little)
      ..setUint32(24, e.bytes.length, Endian.little)
      ..setUint16(28, name.length, Endian.little)
      ..setUint16(30, 0, Endian.little)
      ..setUint16(32, 0, Endian.little)
      ..setUint16(34, 0, Endian.little)
      ..setUint16(36, 0, Endian.little)
      ..setUint32(38, 0, Endian.little)
      ..setUint32(42, offset, Endian.little);
    central.add(cd.buffer.asUint8List());
    central.add(name);

    offset += 30 + name.length + data.length;
  }
  final cdBytes = central.toBytes();
  out.add(cdBytes);
  final end = ByteData(22)
    ..setUint32(0, 0x06054b50, Endian.little)
    ..setUint16(4, 0, Endian.little)
    ..setUint16(6, 0, Endian.little)
    ..setUint16(8, entries.length, Endian.little)
    ..setUint16(10, entries.length, Endian.little)
    ..setUint32(12, cdBytes.length, Endian.little)
    ..setUint32(16, offset, Endian.little)
    ..setUint16(20, 0, Endian.little);
  out.add(end.buffer.asUint8List());
  return out.toBytes();
}

String _decodeName(Uint8List bytes) {
  try {
    return utf8.decode(bytes);
  } on FormatException {
    return latin1.decode(bytes);
  }
}

List<ZipEntryInfo> listZipEntries(
  Uint8List zip, {
  ZipLimits limits = const ZipLimits(),
}) {
  if (zip.length < 22) {
    throw const ZipFormatException('File is too small to be a zip.');
  }
  final data = ByteData.sublistView(zip);
  var eocd = -1;
  final lowest = zip.length - 22 - 65535 < 0 ? 0 : zip.length - 22 - 65535;
  for (var i = zip.length - 22; i >= lowest; i--) {
    if (data.getUint32(i, Endian.little) == 0x06054b50) {
      eocd = i;
      break;
    }
  }
  if (eocd < 0) {
    throw const ZipFormatException('Not a zip file (end record not found).');
  }
  final count = data.getUint16(eocd + 10, Endian.little);
  final cdSize = data.getUint32(eocd + 12, Endian.little);
  final cdOffset = data.getUint32(eocd + 16, Endian.little);
  if (count == 0xFFFF || cdSize == 0xFFFFFFFF || cdOffset == 0xFFFFFFFF) {
    throw const ZipFormatException('Zip64 archives are not supported.');
  }
  if (count > limits.maxEntries) {
    throw ZipFormatException('Too many entries (${limits.maxEntries} allowed).');
  }
  if (cdOffset + cdSize > eocd) {
    throw const ZipFormatException('Zip directory is damaged.');
  }

  final out = <ZipEntryInfo>[];
  var p = cdOffset;
  var total = 0;
  for (var i = 0; i < count; i++) {
    if (p + 46 > zip.length || data.getUint32(p, Endian.little) != 0x02014b50) {
      throw const ZipFormatException('Zip directory is damaged.');
    }
    final flags = data.getUint16(p + 8, Endian.little);
    if ((flags & 1) != 0) {
      throw const ZipFormatException('Encrypted entries are not supported.');
    }
    final method = data.getUint16(p + 10, Endian.little);
    final crc = data.getUint32(p + 16, Endian.little);
    final compressed = data.getUint32(p + 20, Endian.little);
    final uncompressed = data.getUint32(p + 24, Endian.little);
    final nameLen = data.getUint16(p + 28, Endian.little);
    final extraLen = data.getUint16(p + 30, Endian.little);
    final commentLen = data.getUint16(p + 32, Endian.little);
    final localOffset = data.getUint32(p + 42, Endian.little);
    if (p + 46 + nameLen > zip.length) {
      throw const ZipFormatException('Zip directory is damaged.');
    }
    final name = _decodeName(Uint8List.sublistView(zip, p + 46, p + 46 + nameLen));
    if (method != 0 && method != 8) {
      throw ZipFormatException('"$name" uses unsupported compression method $method.');
    }
    if (uncompressed > limits.maxEntryBytes) {
      throw ZipFormatException('"$name" is larger than the allowed entry size.');
    }
    total += uncompressed;
    if (total > limits.maxTotalBytes) {
      throw const ZipFormatException('Package is larger than the allowed total size.');
    }
    out.add(ZipEntryInfo(
      path: name,
      method: method,
      crc: crc,
      compressedSize: compressed,
      uncompressedSize: uncompressed,
      localOffset: localOffset,
    ));
    p += 46 + nameLen + extraLen + commentLen;
  }
  return out;
}

class _LimitedSink implements Sink<List<int>> {
  _LimitedSink(this.limit);

  final int limit;
  final BytesBuilder _bytes = BytesBuilder(copy: false);

  @override
  void add(List<int> data) {
    if (_bytes.length + data.length > limit) {
      throw const ZipFormatException('An entry expands beyond its declared size.');
    }
    _bytes.add(data);
  }

  @override
  void close() {}

  Uint8List takeBytes() => _bytes.takeBytes();
}

Uint8List _inflate(Uint8List slice, int expected, String name) {
  final sink = _LimitedSink(expected);
  try {
    final conv = ZLibDecoder(raw: true).startChunkedConversion(sink);
    conv.add(slice);
    conv.close();
  } on ZipFormatException {
    rethrow;
  } catch (_) {
    throw ZipFormatException('"$name" is corrupt.');
  }
  return sink.takeBytes();
}

Uint8List readZipEntry(Uint8List zip, ZipEntryInfo entry) {
  final data = ByteData.sublistView(zip);
  final p = entry.localOffset;
  if (p + 30 > zip.length || data.getUint32(p, Endian.little) != 0x04034b50) {
    throw ZipFormatException('"${entry.path}" has a damaged header.');
  }
  final nameLen = data.getUint16(p + 26, Endian.little);
  final extraLen = data.getUint16(p + 28, Endian.little);
  final start = p + 30 + nameLen + extraLen;
  final end = start + entry.compressedSize;
  if (end > zip.length) {
    throw ZipFormatException('"${entry.path}" is truncated.');
  }
  final slice = Uint8List.sublistView(zip, start, end);
  final result = entry.method == 0
      ? Uint8List.fromList(slice)
      : _inflate(slice, entry.uncompressedSize, entry.path);
  if (result.length != entry.uncompressedSize) {
    throw ZipFormatException('"${entry.path}" has the wrong size.');
  }
  if (crc32(result) != entry.crc) {
    throw ZipFormatException('"${entry.path}" failed its checksum (damaged file).');
  }
  return result;
}
