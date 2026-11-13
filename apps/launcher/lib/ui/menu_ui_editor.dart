import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../export/fnf_paths.dart';
import 'background_image.dart';
import 'developer_mode.dart';
import 'fnf_tokens.dart';
import 'launcher_ui_controller.dart';
import 'main_menu.dart';
import 'menu_definition.dart';

const _swatches = <Color>[
  Color(0xFFFF6A1F),
  Color(0xFFFFC53D),
  Color(0xFF5BE37D),
  Color(0xFF2FD3FF),
  Color(0xFFFF4D4D),
  Color(0xFFFF5DA2),
  Color(0xFFF2F0E6),
];

const _boxBorder = OutlineInputBorder(borderRadius: BorderRadius.zero);
const _errorColor = Color(0xFFFF5D5D);

/// Developer Mode tool: edit the launcher menu/UI with a live preview.
/// Desktop: controls left, preview right. Mobile: preview on top, controls
/// below.
class MenuUiEditorScreen extends StatelessWidget {
  const MenuUiEditorScreen({
    super.key,
    required this.controller,
    this.animated = true,
  });

  final LauncherUiController controller;
  final bool animated;

  @override
  Widget build(BuildContext context) {
    return Material(
      color: FnfColors.background,
      child: LayoutBuilder(
        builder: (context, constraints) {
          final controls = _Controls(controller: controller);
          final preview = _PreviewPane(controller: controller, animated: animated);
          if (constraints.maxWidth >= 900) {
            return Row(
              children: [
                Expanded(flex: 4, child: controls),
                Container(width: 1, color: FnfColors.line),
                Expanded(flex: 5, child: preview),
              ],
            );
          }
          return Column(
            children: [
              SizedBox(height: 220, child: preview),
              Container(height: 1, color: FnfColors.line),
              Expanded(child: controls),
            ],
          );
        },
      ),
    );
  }
}

class _Heading extends StatelessWidget {
  const _Heading(this.text);

  final String text;

  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.only(top: FnfSpace.lg, bottom: FnfSpace.sm),
        child: Text(text, style: FnfType.eyebrow),
      );
}

class _Controls extends StatelessWidget {
  const _Controls({required this.controller});

  final LauncherUiController controller;

  void _set(LauncherUiDefinition Function(LauncherUiDefinition d) change) =>
      controller.update(change(controller.definition));

  Future<void> _save(BuildContext context) async {
    final messenger = ScaffoldMessenger.of(context);
    try {
      await controller.save();
      messenger.showSnackBar(const SnackBar(content: Text('Menu/UI saved')));
    } catch (e) {
      messenger.showSnackBar(SnackBar(content: Text('Could not save: $e')));
    }
  }

  Future<void> _confirmReset(BuildContext context) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (context) => AlertDialog(
        shape: const RoundedRectangleBorder(),
        title: const Text('RESET TO DEFAULTS?'),
        content: const Text('This replaces your current menu/UI edits. Saved data is only changed if you press Save afterwards.'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(context, false), child: const Text('CANCEL')),
          TextButton(onPressed: () => Navigator.pop(context, true), child: const Text('RESET')),
        ],
      ),
    );
    if (ok == true) controller.resetToDefaults();
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final def = controller.definition;
        final rev = controller.revision;
        final hex = colorToHex(def.accent);
        return ListView(
          padding: const EdgeInsets.all(FnfSpace.lg),
          children: [
            Row(
              children: [
                const Text('MENU / UI EDITOR', style: FnfType.eyebrow),
                const Spacer(),
                if (controller.hasUnsavedChanges)
                  Text('UNSAVED', style: FnfType.tag.copyWith(color: def.accent)),
              ],
            ),
            const _Heading('IDENTITY'),
            TextFormField(
              key: ValueKey('ui-top-$rev'),
              initialValue: def.wordmarkTop,
              inputFormatters: [LengthLimitingTextInputFormatter(kMaxWordmarkLength)],
              decoration: const InputDecoration(labelText: 'Small line above the title', border: _boxBorder),
              onChanged: (v) => _set((d) => d.copyWith(wordmarkTop: v)),
            ),
            const SizedBox(height: FnfSpace.md),
            TextFormField(
              key: ValueKey('ui-main-$rev'),
              initialValue: def.wordmarkMain,
              inputFormatters: [LengthLimitingTextInputFormatter(kMaxWordmarkLength)],
              decoration: const InputDecoration(labelText: 'Title', border: _boxBorder),
              onChanged: (v) => _set((d) => d.copyWith(wordmarkMain: v)),
            ),
            const _Heading('LOOK'),
            Wrap(
              spacing: FnfSpace.sm,
              runSpacing: FnfSpace.sm,
              children: [
                for (final c in _swatches)
                  _Swatch(
                    color: c,
                    selected: c.toARGB32() == def.accent.toARGB32(),
                    onTap: () => _set((d) => d.copyWith(accent: c)),
                  ),
              ],
            ),
            const SizedBox(height: FnfSpace.md),
            TextFormField(
              key: ValueKey('ui-hex-$hex-$rev'),
              initialValue: hex,
              decoration: const InputDecoration(
                labelText: 'Accent color (#RRGGBB, press Enter)',
                border: _boxBorder,
              ),
              onFieldSubmitted: (v) {
                final parsed = parseHexColor(v);
                if (parsed == null || !accentIsReadable(parsed)) {
                  ScaffoldMessenger.of(context).showSnackBar(const SnackBar(
                    content: Text('Use a readable #RRGGBB color (not too dark).'),
                  ));
                  return;
                }
                _set((d) => d.copyWith(accent: parsed));
              },
            ),
            const SizedBox(height: FnfSpace.md),
            SegmentedButton<MenuBackgroundStyle>(
              segments: const [
                ButtonSegment(value: MenuBackgroundStyle.stripes, label: Text('STRIPES')),
                ButtonSegment(value: MenuBackgroundStyle.image, label: Text('IMAGE')),
                ButtonSegment(value: MenuBackgroundStyle.plain, label: Text('PLAIN')),
              ],
              selected: {def.background},
              onSelectionChanged: (s) => _set((d) => d.copyWith(background: s.first)),
            ),
            if (def.background == MenuBackgroundStyle.image)
              _BackgroundPicker(controller: controller),
            SwitchListTile(
              contentPadding: EdgeInsets.zero,
              title: const Text('Beat bop on the selected entry'),
              subtitle: const Text(
                'Players with reduced motion turned on never see animation, whatever you pick.',
                style: FnfType.hint,
              ),
              value: def.beatBop,
              onChanged: (v) => _set((d) => d.copyWith(beatBop: v)),
            ),
            const _Heading('ENTRIES (DRAG TO REORDER)'),
            ReorderableListView.builder(
              shrinkWrap: true,
              physics: const NeverScrollableScrollPhysics(),
              buildDefaultDragHandles: false,
              itemCount: def.entries.length,
              onReorderItem: (oldIndex, newIndex) {
                final list = [...controller.definition.entries];
                final item = list.removeAt(oldIndex);
                list.insert(newIndex, item);
                _set((d) => d.copyWith(entries: list));
              },
              itemBuilder: (context, index) {
                final e = def.entries[index];
                return _EntryRow(
                  key: ValueKey('entry-${e.id}'),
                  index: index,
                  entry: e,
                  revision: rev,
                  protected: kProtectedEntryIds.contains(e.id),
                  onChanged: (next) => _set((d) => d.copyWith(
                        entries: [for (final x in d.entries) x.id == next.id ? next : x],
                      )),
                );
              },
            ),
            if (controller.warnings.isNotEmpty) ...[
              const _Heading('NOTES FROM THE LAST LOAD'),
              for (final w in controller.warnings)
                Padding(
                  padding: const EdgeInsets.only(bottom: FnfSpace.xs),
                  child: Text('\u2022 $w', style: FnfType.hint),
                ),
            ],
            const SizedBox(height: FnfSpace.lg),
            Wrap(
              spacing: FnfSpace.sm,
              runSpacing: FnfSpace.sm,
              children: [
                FilledButton.icon(
                  key: const ValueKey('ui-save'),
                  style: FilledButton.styleFrom(minimumSize: const Size(48, 48), shape: const RoundedRectangleBorder()),
                  onPressed: () => _save(context),
                  icon: const Icon(Icons.save_outlined),
                  label: const Text('SAVE'),
                ),
                OutlinedButton.icon(
                  key: const ValueKey('ui-reset'),
                  style: OutlinedButton.styleFrom(minimumSize: const Size(48, 48), shape: const RoundedRectangleBorder()),
                  onPressed: () => _confirmReset(context),
                  icon: const Icon(Icons.restart_alt),
                  label: const Text('RESET TO DEFAULTS'),
                ),
              ],
            ),
            const SizedBox(height: FnfSpace.xl),
          ],
        );
      },
    );
  }
}

/// Wallpaper chooser. Images live in Documents/FNFUniversal/Backgrounds. Add
/// one by pasting the full path of an image file (it is copied in, never
/// moved). A native file picker needs a plugin and comes later.
class _BackgroundPicker extends StatefulWidget {
  const _BackgroundPicker({required this.controller});

  final LauncherUiController controller;

  @override
  State<_BackgroundPicker> createState() => _BackgroundPickerState();
}

class _BackgroundPickerState extends State<_BackgroundPicker> {
  final TextEditingController _path = TextEditingController();
  List<String> _names = const [];
  Directory? _dir;
  String _message = '';
  bool _error = false;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void dispose() {
    _path.dispose();
    super.dispose();
  }

  void _say(String text, {bool error = false}) {
    if (!mounted) return;
    setState(() {
      _message = text;
      _error = error;
    });
  }

  Future<void> _refresh() async {
    try {
      final names = await widget.controller.listBackgrounds();
      final dir = await FnfPaths.backgrounds();
      if (!mounted) return;
      setState(() {
        _names = names;
        _dir = dir;
      });
    } catch (e) {
      _say('Could not read the Backgrounds folder: $e', error: true);
    }
  }

  Future<void> _addFromPath() async {
    final path = _path.text.trim();
    if (path.isEmpty) return;
    try {
      final name = await widget.controller.importBackgroundFile(path);
      if (name == null) {
        _say('Saving images is not available here.', error: true);
        return;
      }
      final d = widget.controller.definition;
      widget.controller.update(
        d.copyWith(background: MenuBackgroundStyle.image, backgroundImage: name),
      );
      _path.clear();
      await _refresh();
      _say('Added $name and selected it.');
    } on BackgroundImportException catch (e) {
      _say(e.message, error: true);
    } catch (e) {
      _say('Could not add that image: $e', error: true);
    }
  }

  void _choose(String name) {
    final d = widget.controller.definition;
    widget.controller.update(
      d.copyWith(background: MenuBackgroundStyle.image, backgroundImage: name),
    );
  }

  Future<void> _openFolder() async {
    final dir = _dir;
    if (dir == null || !Platform.isWindows) return;
    try {
      await Process.start('explorer.exe', [dir.path]);
    } catch (e) {
      _say('Could not open the folder: $e', error: true);
    }
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.controller,
      builder: (context, _) {
        final def = widget.controller.definition;
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const _Heading('WALLPAPER IMAGE'),
            const Text(
              'Use images you have the right to use. They are copied into the Backgrounds folder and packed into exported zips (png, jpg or webp, up to 8 MB).',
              style: FnfType.hint,
            ),
            const SizedBox(height: FnfSpace.sm),
            if (_names.isEmpty)
              Text(
                'No images yet. Add one below.',
                style: FnfType.body.copyWith(color: FnfColors.textMuted),
              )
            else
              for (final n in _names)
                InkWell(
                  onTap: () => _choose(n),
                  child: Container(
                    constraints: const BoxConstraints(minHeight: 48),
                    margin: const EdgeInsets.only(bottom: 4),
                    padding: const EdgeInsets.symmetric(horizontal: FnfSpace.md),
                    color: n == def.backgroundImage ? FnfColors.surfaceRaised : FnfColors.surface,
                    child: Row(
                      children: [
                        Container(
                          width: 4,
                          height: 24,
                          color: n == def.backgroundImage ? def.accent : Colors.transparent,
                        ),
                        const SizedBox(width: FnfSpace.sm),
                        Expanded(child: Text(n, overflow: TextOverflow.ellipsis)),
                      ],
                    ),
                  ),
                ),
            const SizedBox(height: FnfSpace.sm),
            Row(
              children: [
                Expanded(
                  child: TextField(
                    key: const ValueKey('ui-bg-path'),
                    controller: _path,
                    decoration: const InputDecoration(
                      labelText: 'Full path to an image (png, jpg, webp)',
                      border: _boxBorder,
                    ),
                    onSubmitted: (_) => _addFromPath(),
                  ),
                ),
                const SizedBox(width: FnfSpace.sm),
                OutlinedButton(
                  style: OutlinedButton.styleFrom(minimumSize: const Size(48, 56), shape: const RoundedRectangleBorder()),
                  onPressed: _addFromPath,
                  child: const Text('ADD'),
                ),
              ],
            ),
            Wrap(
              spacing: FnfSpace.sm,
              children: [
                TextButton.icon(
                  onPressed: _refresh,
                  icon: const Icon(Icons.refresh, size: 18),
                  label: const Text('REFRESH'),
                ),
                if (Platform.isWindows)
                  TextButton.icon(
                    onPressed: _openFolder,
                    icon: const Icon(Icons.folder_open, size: 18),
                    label: const Text('OPEN FOLDER'),
                  ),
              ],
            ),
            if (_dir != null) SelectableText(_dir!.path, style: FnfType.hint),
            if (_message.isNotEmpty)
              Padding(
                padding: const EdgeInsets.only(top: FnfSpace.xs),
                child: Text(
                  _message,
                  style: FnfType.body.copyWith(color: _error ? _errorColor : def.accent),
                ),
              ),
            const SizedBox(height: FnfSpace.md),
            Text('DARKEN  ${(def.backgroundDim * 100).round()}%', style: FnfType.eyebrow),
            Slider(
              value: def.backgroundDim.clamp(0.0, kMaxBackgroundDim).toDouble(),
              min: 0,
              max: kMaxBackgroundDim,
              onChanged: (v) => widget.controller.update(
                widget.controller.definition.copyWith(backgroundDim: v),
              ),
            ),
          ],
        );
      },
    );
  }
}

class _Swatch extends StatelessWidget {
  const _Swatch({required this.color, required this.selected, required this.onTap});

  final Color color;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      selected: selected,
      label: 'Accent color ${colorToHex(color)}',
      child: InkWell(
        onTap: onTap,
        child: Container(
          width: 48,
          height: 48,
          decoration: BoxDecoration(
            color: color,
            border: Border.all(
              color: selected ? FnfColors.text : FnfColors.line,
              width: selected ? 3 : 1,
            ),
          ),
        ),
      ),
    );
  }
}

class _EntryRow extends StatelessWidget {
  const _EntryRow({
    super.key,
    required this.index,
    required this.entry,
    required this.revision,
    required this.protected,
    required this.onChanged,
  });

  final int index;
  final MenuEntryDef entry;
  final int revision;
  final bool protected;
  final ValueChanged<MenuEntryDef> onChanged;

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(bottom: FnfSpace.sm),
      padding: const EdgeInsets.all(FnfSpace.sm),
      decoration: BoxDecoration(
        color: FnfColors.surface,
        border: Border.all(color: FnfColors.line),
      ),
      child: Row(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          ReorderableDragStartListener(
            index: index,
            child: const SizedBox(
              width: 48,
              height: 48,
              child: Icon(Icons.drag_handle),
            ),
          ),
          Expanded(
            child: Column(
              children: [
                TextFormField(
                  key: ValueKey('label-${entry.id}-$revision'),
                  initialValue: entry.label,
                  inputFormatters: [LengthLimitingTextInputFormatter(kMaxLabelLength)],
                  decoration: const InputDecoration(labelText: 'Label', isDense: true, border: _boxBorder),
                  onChanged: (v) => onChanged(entry.copyWith(label: v)),
                ),
                const SizedBox(height: FnfSpace.sm),
                TextFormField(
                  key: ValueKey('summary-${entry.id}-$revision'),
                  initialValue: entry.summary,
                  maxLines: 2,
                  inputFormatters: [LengthLimitingTextInputFormatter(kMaxSummaryLength)],
                  decoration: const InputDecoration(labelText: 'Description', isDense: true, border: _boxBorder),
                  onChanged: (v) => onChanged(entry.copyWith(summary: v)),
                ),
              ],
            ),
          ),
          SizedBox(
            width: 64,
            height: 48,
            child: protected
                ? const Tooltip(
                    message: 'Required. This entry cannot be hidden.',
                    child: Icon(Icons.lock_outline),
                  )
                : Switch(
                    value: entry.visible,
                    onChanged: (v) => onChanged(entry.copyWith(visible: v)),
                  ),
          ),
        ],
      ),
    );
  }
}

class _PreviewPane extends StatefulWidget {
  const _PreviewPane({required this.controller, required this.animated});

  final LauncherUiController controller;
  final bool animated;

  @override
  State<_PreviewPane> createState() => _PreviewPaneState();
}

class _PreviewPaneState extends State<_PreviewPane> {
  final DeveloperModeController _dev = DeveloperModeController();

  @override
  void dispose() {
    _dev.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final media = MediaQuery.of(context);
    return Padding(
      padding: const EdgeInsets.all(FnfSpace.md),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('LIVE PREVIEW', style: FnfType.eyebrow),
          const SizedBox(height: FnfSpace.sm),
          Expanded(
            child: DecoratedBox(
              decoration: BoxDecoration(border: Border.all(color: FnfColors.line)),
              child: ClipRect(
                child: FittedBox(
                  fit: BoxFit.contain,
                  child: SizedBox(
                    width: 960,
                    height: 540,
                    child: ExcludeSemantics(
                      child: IgnorePointer(
                        child: MediaQuery(
                          data: media.copyWith(
                            padding: EdgeInsets.zero,
                            viewPadding: EdgeInsets.zero,
                            viewInsets: EdgeInsets.zero,
                          ),
                          child: MainMenuScreen(
                            developerMode: _dev,
                            ui: widget.controller,
                            onOpenDeveloper: () {},
                            animated: widget.animated,
                            showTouchControls: false,
                            preview: true,
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}
