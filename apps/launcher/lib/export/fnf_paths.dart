import 'dart:io';

import 'package:path_provider/path_provider.dart';

/// Folders FNF Universal uses under <Documents>/FNFUniversal. The Projects
/// folder is the same one the Studio saves to.
///
/// Note: on Android, Documents is app-private. A real file picker / share
/// sheet (Storage Access Framework) is needed before mobile users can move
/// zips in and out. That is not built yet.
class FnfPaths {
  static Future<Directory> _sub(String name) async {
    final docs = await getApplicationDocumentsDirectory();
    final sep = Platform.pathSeparator;
    final dir = Directory('${docs.path}${sep}FNFUniversal$sep$name');
    await dir.create(recursive: true);
    return dir;
  }

  static Future<Directory> projects() => _sub('Projects');
  static Future<Directory> scripts() => _sub('Scripts');
  static Future<Directory> exports() => _sub('Exports');
  static Future<Directory> imports() => _sub('Imports');
}
