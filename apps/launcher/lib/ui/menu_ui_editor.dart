import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

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
                ButtonSegment(value: MenuBackgroundStyle.plain, label: Text('PLAIN')),
              ],
              selected: {def.background},
              onSelectionChanged: (s) => _set((d) => d.copyWith(background: s.first)),
            ),
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
              onReorder: (oldIndex, newIndex) {
                if (newIndex > oldIndex) newIndex -= 1;
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
