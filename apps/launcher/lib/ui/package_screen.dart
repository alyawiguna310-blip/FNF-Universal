import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../export/fnf_paths.dart';
import '../export/mod_package.dart';
import 'fnf_tokens.dart';
import 'launcher_ui_controller.dart';
import 'menu_definition.dart';

enum _PackageMode { export, import }

const _warnColor = Color(0xFFFFC53D);
const _errorColor = Color(0xFFFF5D5D);
const _boxBorder = OutlineInputBorder(borderRadius: BorderRadius.zero);

/// Developer Mode tool: export a mod (project + scripts + menu/UI + wallpaper)
/// or a launcher profile as one zip, and inspect/import zips safely.
class PackageScreen extends StatefulWidget {
  const PackageScreen({super.key, required this.ui});

  final LauncherUiController ui;

  @override
  State<PackageScreen> createState() => _PackageScreenState();
}

class _PackageScreenState extends State<PackageScreen> {
  _PackageMode _mode = _PackageMode.export;
  ModPackageKind _kind = ModPackageKind.mod;
  final TextEditingController _profileName = TextEditingController(text: 'My Launcher');
  final TextEditingController _zipPath = TextEditingController();

  List<File> _projects = const [];
  File? _project;
  Directory? _scriptsDir;
  ScriptCollection? _scripts;
  List<File> _zips = const [];
  Directory? _exportsDir;
  Directory? _importsDir;

  String _status = '';
  bool _statusIsError = false;
  bool _busy = false;

  ModPackageReport? _report;
  Uint8List? _zipBytes;
  String? _zipLabel;
  ImportResult? _imported;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void dispose() {
    _profileName.dispose();
    _zipPath.dispose();
    super.dispose();
  }

  void _setStatus(String message, {bool error = false}) {
    if (!mounted) return;
    setState(() {
      _status = message;
      _statusIsError = error;
    });
  }

  String _stem(File f) =>
      f.uri.pathSegments.last.replaceAll(RegExp(r'\.fnfup\.json$'), '');

  Future<void> _refresh() async {
    try {
      final projectsDir = await FnfPaths.projects();
      final exportsDir = await FnfPaths.exports();
      final importsDir = await FnfPaths.imports();
      final projects = projectsDir
          .listSync()
          .whereType<File>()
          .where((f) => f.path.endsWith('.fnfup.json'))
          .toList()
        ..sort((a, b) => a.path.compareTo(b.path));
      final zips = <File>[
        for (final dir in [importsDir, exportsDir])
          ...dir
              .listSync()
              .whereType<File>()
              .where((f) => f.path.toLowerCase().endsWith('.zip')),
      ];
      File? selected;
      for (final f in projects) {
        if (f.path == _project?.path) selected = f;
      }
      if (!mounted) return;
      setState(() {
        _projects = projects;
        _project = selected ?? (projects.isNotEmpty ? projects.first : null);
        _zips = zips;
        _exportsDir = exportsDir;
        _importsDir = importsDir;
      });
      await _loadScripts();
    } catch (e) {
      _setStatus('Could not read the FNF Universal folders: $e', error: true);
    }
  }

  Future<void> _loadScripts() async {
    final project = _project;
    if (project == null) {
      if (mounted) {
        setState(() {
          _scriptsDir = null;
          _scripts = null;
        });
      }
      return;
    }
    final root = await FnfPaths.scripts();
    final dir = Directory('${root.path}${Platform.pathSeparator}${safeFileStem(_stem(project))}');
    final collected = await collectScriptFiles(dir);
    if (!mounted) return;
    setState(() {
      _scriptsDir = dir;
      _scripts = collected;
    });
  }

  Future<File> _uniqueFile(Directory dir, String fileName) async {
    final sep = Platform.pathSeparator;
    var file = File('${dir.path}$sep$fileName');
    var n = 2;
    while (await file.exists()) {
      final dot = fileName.indexOf('.');
      final name = dot < 0 ? '$fileName-$n' : '${fileName.substring(0, dot)}-$n${fileName.substring(dot)}';
      file = File('${dir.path}$sep$name');
      n++;
    }
    return file;
  }

  Future<void> _export() async {
    setState(() => _busy = true);
    try {
      final exports = await FnfPaths.exports();
      final Uint8List bytes;
      final String fileName;
      if (_kind == ModPackageKind.mod) {
        final project = _project;
        if (project == null) {
          _setStatus('Save a project in Studio first, then pick it here.', error: true);
          return;
        }
        final stem = _stem(project);
        final scripts = _scripts?.files ?? const <String, Uint8List>{};
        bytes = buildModPackage(
          kind: ModPackageKind.mod,
          name: stem,
          ui: widget.ui.definition,
          projectJson: await project.readAsString(),
          scripts: scripts,
          backgroundBytes: widget.ui.backgroundBytes,
        );
        fileName = '${safeFileStem(stem)}.fnfmod.zip';
      } else {
        final raw = _profileName.text.trim();
        final name = raw.isEmpty ? 'My Launcher' : raw;
        bytes = buildModPackage(
          kind: ModPackageKind.launcherProfile,
          name: name,
          ui: widget.ui.definition,
          backgroundBytes: widget.ui.backgroundBytes,
        );
        fileName = '${safeFileStem(name)}.fnflauncher.zip';
      }
      final file = await _uniqueFile(exports, fileName);
      await file.writeAsBytes(bytes, flush: true);
      final unsaved = widget.ui.hasUnsavedChanges ? ' It includes your unsaved menu/UI edits.' : '';
      final def = widget.ui.definition;
      final noWallpaper = def.background == MenuBackgroundStyle.image && widget.ui.backgroundBytes == null
          ? ' The wallpaper image could not be read, so it is not in the zip.'
          : '';
      _setStatus('Exported ${bytes.length} bytes to ${file.path}.$unsaved$noWallpaper');
      await _refresh();
    } catch (e) {
      _setStatus('Export failed: $e', error: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _inspectFile(File file) async {
    try {
      if (await file.length() > 300 * 1024 * 1024) {
        _setStatus('That file is too large to inspect.', error: true);
        return;
      }
      final bytes = await file.readAsBytes();
      final report = inspectModPackage(bytes);
      if (!mounted) return;
      setState(() {
        _zipBytes = bytes;
        _zipLabel = file.path;
        _report = report;
        _imported = null;
      });
      _setStatus(report.canImport ? 'Inspected ${file.uri.pathSegments.last}. Nothing was run or converted.' : report.errors.join(' '), error: !report.canImport);
    } catch (e) {
      _setStatus('Could not read that file: $e', error: true);
    }
  }

  Future<void> _inspectTyped() async {
    final path = _zipPath.text.trim();
    if (path.isEmpty) return;
    final file = File(path);
    if (!await file.exists()) {
      _setStatus('No file at that path.', error: true);
      return;
    }
    await _inspectFile(file);
  }

  Future<void> _importCopy() async {
    final bytes = _zipBytes;
    if (bytes == null) return;
    setState(() => _busy = true);
    try {
      final result = await importModPackage(bytes, await FnfPaths.imports());
      if (!mounted) return;
      setState(() => _imported = result);
      _setStatus('Imported a copy: ${result.written} file(s) written, ${result.skipped} skipped, into ${result.directory.path}');
    } catch (e) {
      _setStatus('Import failed: $e', error: true);
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _applyUi() async {
    final report = _report;
    final parsed = report?.ui;
    if (report == null || parsed == null) return;
    var def = parsed.definition;
    final notes = [...parsed.warnings];
    if (def.background == MenuBackgroundStyle.image) {
      String? installed;
      final name = report.backgroundName;
      final bytes = report.backgroundBytes;
      if (name != null && bytes != null) {
        installed = await widget.ui.installBackground(name, bytes);
      }
      if (installed != null) {
        def = def.copyWith(backgroundImage: installed);
      } else {
        def = def.copyWith(
          background: MenuBackgroundStyle.stripes,
          clearBackgroundImage: true,
        );
        notes.add('The wallpaper could not be installed, so stripes are used.');
      }
    }
    widget.ui.replace(def, warnings: notes);
    _setStatus('Applied the package menu/UI. Review it in the MENU / UI tab, then press Save to keep it.');
  }

  Future<void> _openFolder(Directory? dir) async {
    if (dir == null || !Platform.isWindows) return;
    try {
      await Process.start('explorer.exe', [dir.path]);
    } catch (e) {
      _setStatus('Could not open the folder: $e', error: true);
    }
  }

  Widget _pathRow(String label, Directory? dir) {
    final path = dir?.path ?? '...';
    return Padding(
      padding: const EdgeInsets.only(bottom: FnfSpace.sm),
      child: Wrap(
        crossAxisAlignment: WrapCrossAlignment.center,
        spacing: FnfSpace.sm,
        children: [
          Text(label, style: FnfType.eyebrow),
          SelectableText(path, style: FnfType.hint),
          IconButton(
            tooltip: 'Copy path',
            onPressed: dir == null ? null : () => Clipboard.setData(ClipboardData(text: path)),
            icon: const Icon(Icons.copy, size: 18),
          ),
          if (Platform.isWindows)
            IconButton(
              tooltip: 'Open folder',
              onPressed: dir == null ? null : () => _openFolder(dir),
              icon: const Icon(Icons.folder_open, size: 18),
            ),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return Material(
      color: FnfColors.background,
      child: ListView(
        padding: const EdgeInsets.all(FnfSpace.lg),
        children: [
          const Text('MOD PACKAGE', style: FnfType.eyebrow),
          const SizedBox(height: FnfSpace.sm),
          SegmentedButton<_PackageMode>(
            segments: const [
              ButtonSegment(value: _PackageMode.export, label: Text('EXPORT')),
              ButtonSegment(value: _PackageMode.import, label: Text('IMPORT')),
            ],
            selected: {_mode},
            onSelectionChanged: (s) => setState(() => _mode = s.first),
          ),
          const SizedBox(height: FnfSpace.lg),
          if (_mode == _PackageMode.export) ..._exportSection() else ..._importSection(),
          if (_status.isNotEmpty) ...[
            const SizedBox(height: FnfSpace.lg),
            SelectableText(
              _status,
              style: FnfType.body.copyWith(
                color: _statusIsError ? _errorColor : widget.ui.definition.accent,
              ),
            ),
          ],
          const SizedBox(height: FnfSpace.xl),
        ],
      ),
    );
  }

  List<Widget> _exportSection() {
    final scripts = _scripts;
    return [
      const Text('WHAT TO EXPORT', style: FnfType.eyebrow),
      const SizedBox(height: FnfSpace.sm),
      SegmentedButton<ModPackageKind>(
        segments: const [
          ButtonSegment(value: ModPackageKind.mod, label: Text('MOD')),
          ButtonSegment(value: ModPackageKind.launcherProfile, label: Text('LAUNCHER PROFILE')),
        ],
        selected: {_kind},
        onSelectionChanged: (s) => setState(() => _kind = s.first),
      ),
      const SizedBox(height: FnfSpace.sm),
      Text(
        _kind == ModPackageKind.mod
            ? 'One zip with your saved project, every script file and the current menu/UI (wallpaper included). Scripts are copied as files, not run and not converted.'
            : 'Only the menu/UI (wallpaper included) and name. Import it on another FNF Universal install to rebrand it. A standalone APK/EXE build is a later step.',
        style: FnfType.body.copyWith(color: FnfColors.textMuted),
      ),
      const SizedBox(height: FnfSpace.lg),
      if (_kind == ModPackageKind.mod) ...[
        if (_projects.isEmpty)
          Text(
            'No saved projects yet. Open Studio, save a project, then press refresh.',
            style: FnfType.body.copyWith(color: _warnColor),
          )
        else
          DropdownButtonFormField<File>(
            key: ValueKey(_project?.path),
            initialValue: _project,
            decoration: const InputDecoration(labelText: 'Project', border: _boxBorder),
            items: [
              for (final f in _projects) DropdownMenuItem(value: f, child: Text(_stem(f))),
            ],
            onChanged: (f) {
              setState(() => _project = f);
              _loadScripts();
            },
          ),
        const SizedBox(height: FnfSpace.md),
        _pathRow('SCRIPTS FOLDER', _scriptsDir),
        Text(
          scripts == null
              ? 'Pick a project to see its scripts.'
              : '${scripts.files.length} script file(s) found'
                  '${scripts.skipped.isEmpty ? '' : ', ${scripts.skipped.length} skipped (blocked type, unsafe name or too large)'}.'
                  '${scripts.files.isEmpty ? ' Put .lua / .hx / .json files in that folder to include them.' : ''}',
          style: FnfType.hint,
        ),
      ] else
        TextField(
          controller: _profileName,
          decoration: const InputDecoration(labelText: 'Launcher name', border: _boxBorder),
        ),
      const SizedBox(height: FnfSpace.lg),
      Wrap(
        spacing: FnfSpace.sm,
        runSpacing: FnfSpace.sm,
        children: [
          FilledButton.icon(
            key: const ValueKey('package-export'),
            style: FilledButton.styleFrom(minimumSize: const Size(48, 48), shape: const RoundedRectangleBorder()),
            onPressed: _busy ? null : _export,
            icon: const Icon(Icons.archive_outlined),
            label: const Text('EXPORT ZIP'),
          ),
          OutlinedButton.icon(
            style: OutlinedButton.styleFrom(minimumSize: const Size(48, 48), shape: const RoundedRectangleBorder()),
            onPressed: _refresh,
            icon: const Icon(Icons.refresh),
            label: const Text('REFRESH'),
          ),
        ],
      ),
      const SizedBox(height: FnfSpace.lg),
      _pathRow('EXPORTS FOLDER', _exportsDir),
    ];
  }

  List<Widget> _importSection() {
    final accent = widget.ui.definition.accent;
    final report = _report;
    return [
      const Text('ZIPS FOUND', style: FnfType.eyebrow),
      const SizedBox(height: FnfSpace.sm),
      if (_zips.isEmpty)
        Text(
          'None yet. Export one here, or copy a .zip into the Imports folder and press refresh.',
          style: FnfType.body.copyWith(color: FnfColors.textMuted),
        )
      else
        for (final f in _zips) _zipRow(f, accent),
      const SizedBox(height: FnfSpace.sm),
      _pathRow('IMPORTS FOLDER', _importsDir),
      Row(
        children: [
          Expanded(
            child: TextField(
              controller: _zipPath,
              decoration: const InputDecoration(labelText: 'Or paste a full path to a .zip', border: _boxBorder),
              onSubmitted: (_) => _inspectTyped(),
            ),
          ),
          const SizedBox(width: FnfSpace.sm),
          OutlinedButton(
            style: OutlinedButton.styleFrom(minimumSize: const Size(48, 56), shape: const RoundedRectangleBorder()),
            onPressed: _inspectTyped,
            child: const Text('INSPECT'),
          ),
        ],
      ),
      OutlinedButton.icon(
        style: OutlinedButton.styleFrom(minimumSize: const Size(48, 48), shape: const RoundedRectangleBorder()),
        onPressed: _refresh,
        icon: const Icon(Icons.refresh),
        label: const Text('REFRESH LIST'),
      ),
      if (report != null) ...[
        const SizedBox(height: FnfSpace.lg),
        const Text('REPORT', style: FnfType.eyebrow),
        const SizedBox(height: FnfSpace.sm),
        if (report.manifest != null)
          Text(
            '${report.manifest!.name}  \u00b7  ${report.manifest!.kind.wireName}  \u00b7  ${report.count(ReportStatus.supported)} supported, ${report.count(ReportStatus.needsAttention)} need attention, ${report.count(ReportStatus.unsupported)} unsupported',
            style: FnfType.body,
          ),
        for (final e in report.errors)
          Text(e, style: FnfType.body.copyWith(color: _errorColor)),
        const SizedBox(height: FnfSpace.sm),
        for (final item in report.items) _reportRow(item, accent),
        const SizedBox(height: FnfSpace.md),
        Wrap(
          spacing: FnfSpace.sm,
          runSpacing: FnfSpace.sm,
          children: [
            FilledButton.icon(
              key: const ValueKey('package-import'),
              style: FilledButton.styleFrom(minimumSize: const Size(48, 48), shape: const RoundedRectangleBorder()),
              onPressed: report.canImport && !_busy ? _importCopy : null,
              icon: const Icon(Icons.download_outlined),
              label: const Text('IMPORT A COPY'),
            ),
            OutlinedButton.icon(
              key: const ValueKey('package-apply-ui'),
              style: OutlinedButton.styleFrom(minimumSize: const Size(48, 48), shape: const RoundedRectangleBorder()),
              onPressed: report.ui == null ? null : _applyUi,
              icon: const Icon(Icons.palette_outlined),
              label: const Text('APPLY MENU/UI'),
            ),
          ],
        ),
        if (_imported != null) ...[
          const SizedBox(height: FnfSpace.sm),
          _pathRow('IMPORTED TO', _imported!.directory),
        ],
      ],
    ];
  }

  Widget _zipRow(File f, Color accent) {
    final selected = _zipLabel == f.path;
    return InkWell(
      onTap: () => _inspectFile(f),
      child: Container(
        constraints: const BoxConstraints(minHeight: 48),
        margin: const EdgeInsets.only(bottom: 4),
        padding: const EdgeInsets.symmetric(horizontal: FnfSpace.md),
        color: selected ? FnfColors.surfaceRaised : FnfColors.surface,
        child: Row(
          children: [
            Container(width: 4, height: 24, color: selected ? accent : Colors.transparent),
            const SizedBox(width: FnfSpace.sm),
            Expanded(
              child: Text(f.uri.pathSegments.last, overflow: TextOverflow.ellipsis),
            ),
          ],
        ),
      ),
    );
  }

  Widget _reportRow(ReportItem item, Color accent) {
    final (color, label) = switch (item.status) {
      ReportStatus.supported => (accent, 'SUPPORTED'),
      ReportStatus.needsAttention => (_warnColor, 'NEEDS ATTENTION'),
      ReportStatus.unsupported => (_errorColor, 'UNSUPPORTED'),
    };
    return Padding(
      padding: const EdgeInsets.only(bottom: FnfSpace.sm),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Container(
            padding: const EdgeInsets.symmetric(horizontal: FnfSpace.sm, vertical: FnfSpace.xs),
            decoration: BoxDecoration(border: Border.all(color: color)),
            child: Text(label, style: FnfType.tag.copyWith(color: color)),
          ),
          const SizedBox(width: FnfSpace.sm),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                SelectableText(item.path, style: FnfType.body),
                Text(item.message, style: FnfType.hint),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
