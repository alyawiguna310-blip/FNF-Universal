import 'dart:typed_data';

/// Wallpaper image rules shared by the menu, the editor and mod packages.
const int kMaxBackgroundBytes = 8 * 1024 * 1024;
const int kMaxBackgroundNameLength = 80;
const double kMaxBackgroundDim = 0.85;
const Set<String> kBackgroundExtensions = {'png', 'jpg', 'jpeg', 'webp'};

const Set<String> _reserved = {
  'CON', 'PRN', 'AUX', 'NUL',
  'COM1', 'COM2', 'COM3', 'COM4', 'COM5', 'COM6', 'COM7', 'COM8', 'COM9',
  'LPT1', 'LPT2', 'LPT3', 'LPT4', 'LPT5', 'LPT6', 'LPT7', 'LPT8', 'LPT9',
};

class BackgroundImportException implements Exception {
  const BackgroundImportException(this.message);

  final String message;

  @override
  String toString() => message;
}

/// A plain file name (no folders, no tricks) with an image extension, or null.
String? cleanBackgroundName(Object? raw) {
  if (raw is! String) return null;
  final name = raw.trim();
  if (name.isEmpty || name.length > kMaxBackgroundNameLength) return null;
  if (!RegExp(r'^[A-Za-z0-9][A-Za-z0-9._ -]*$').hasMatch(name)) return null;
  if (name.contains('..') || name.endsWith('.') || name.endsWith(' ')) return null;
  final dot = name.lastIndexOf('.');
  if (dot < 0) return null;
  if (!kBackgroundExtensions.contains(name.substring(dot + 1).toLowerCase())) {
    return null;
  }
  if (_reserved.contains(name.substring(0, dot).toUpperCase())) return null;
  return name;
}

/// Makes a picked file name safe, then validates it.
String? sanitizeBackgroundName(String fileName) {
  final safe = fileName
      .replaceAll(RegExp(r'[^A-Za-z0-9._ -]+'), '_')
      .replaceAll(RegExp(r'^[._ -]+'), '');
  return cleanBackgroundName(safe);
}

/// Checks the file header (png, jpg or webp), not just the extension.
bool looksLikeImage(Uint8List b) {
  if (b.length < 12) return false;
  final png = b[0] == 0x89 && b[1] == 0x50 && b[2] == 0x4E && b[3] == 0x47;
  final jpg = b[0] == 0xFF && b[1] == 0xD8 && b[2] == 0xFF;
  final webp = b[0] == 0x52 &&
      b[1] == 0x49 &&
      b[2] == 0x46 &&
      b[3] == 0x46 &&
      b[8] == 0x57 &&
      b[9] == 0x45 &&
      b[10] == 0x42 &&
      b[11] == 0x50;
  return png || jpg || webp;
}
