import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../content/content_catalog.dart';
import '../content/content_controller.dart';
import 'content_source_panel.dart';
import 'fnf_tokens.dart';
import 'main_menu.dart' show StatusTag;
import 'menu_entry.dart';

class BrowseItem {
  const BrowseItem({
    required this.id,
    required this.title,
    this.subtitle,
    this.chips = const [],
    this.facts = const [],
  });

  final String id;
  final String title;
  final String? subtitle;
  final List<String> chips;
  final List<({String label, String value})> facts;
}

/// Shared list + detail browser used by Freeplay and Story. Browse only: it
/// reads names from the player's folder and launches nothing.
///
/// Arrow keys / D-pad move, Escape / controller B / BACK return, tap selects.
class BrowseScreen extends StatefulWidget {
  const BrowseScreen({
    super.key,
    required this.screenId,
    required this.title,
    required this.content,
    required this.accent,
    required this.itemsOf,
    required this.emptyMessage,
  });

  final String screenId;
  final String title;
  final ContentController content;
  final Color accent;
  final List<BrowseItem> Function(ContentCatalog catalog) itemsOf;
  final String emptyMessage;

  @override
  State<BrowseScreen> createState() => _BrowseScreenState();
}

class _BrowseScreenState extends State<BrowseScreen> {
  static const double _extent = 68;
  final ScrollController _scroll = ScrollController();
  int _selected = 0;

  @override
  void dispose() {
    _scroll.dispose();
    super.dispose();
  }

  void _move(int delta, int count) {
    if (count == 0) return;
    setState(() => _selected = (((_selected + delta) % count) + count) % count);
    if (!_scroll.hasClients) return;
    final p = _scroll.position;
    final target = (_selected * _extent - (p.viewportDimension - _extent) / 2)
        .clamp(0.0, p.maxScrollExtent)
        .toDouble();
    _scroll.animateTo(target, duration: FnfMotion.fast, curve: FnfMotion.curve);
  }

  KeyEventResult _onKey(KeyEvent event, int count) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.arrowDown) {
      _move(1, count);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.arrowUp) {
      _move(-1, count);
      return KeyEventResult.handled;
    }
    if (event is KeyDownEvent &&
        (key == LogicalKeyboardKey.escape || key == LogicalKeyboardKey.gameButtonB)) {
      Navigator.of(context).maybePop();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      key: ValueKey('browse-${widget.screenId}'),
      body: SafeArea(
        child: ListenableBuilder(
          listenable: widget.content,
          builder: (context, _) {
            final items = widget.itemsOf(widget.content.catalog);
            final selected =
                items.isEmpty ? 0 : (_selected >= items.length ? items.length - 1 : _selected);
            return Focus(
              autofocus: true,
              onKeyEvent: (node, event) => _onKey(event, items.length),
              child: LayoutBuilder(
                builder: (context, constraints) {
                  final wide = constraints.maxWidth >= 900;
                  final list = items.isEmpty
                      ? Padding(
                          padding: const EdgeInsets.all(FnfSpace.lg),
                          child: Text(
                            widget.emptyMessage,
                            style: FnfType.body.copyWith(color: FnfColors.textMuted),
                          ),
                        )
                      : ListView.builder(
                          controller: _scroll,
                          itemExtent: _extent,
                          itemCount: items.length,
                          itemBuilder: (context, i) => _Row(
                            item: items[i],
                            selected: i == selected,
                            accent: widget.accent,
                            onTap: () => setState(() => _selected = i),
                          ),
                        );
                  final detail = items.isEmpty
                      ? const SizedBox.shrink()
                      : _Detail(item: items[selected], accent: widget.accent);
                  return Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Padding(
                        padding: const EdgeInsets.fromLTRB(FnfSpace.md, FnfSpace.sm, FnfSpace.lg, 0),
                        child: Row(
                          children: [
                            TextButton.icon(
                              key: const ValueKey('browse-back'),
                              onPressed: () => Navigator.of(context).maybePop(),
                              icon: const Icon(Icons.arrow_back),
                              label: const Text('BACK'),
                            ),
                            const SizedBox(width: FnfSpace.sm),
                            Expanded(
                              child: Text(
                                widget.title.toUpperCase(),
                                style: FnfType.display.copyWith(fontSize: 30),
                                overflow: TextOverflow.ellipsis,
                              ),
                            ),
                            StatusTag(status: MenuStatus.partial, accent: widget.accent),
                          ],
                        ),
                      ),
                      Padding(
                        padding: const EdgeInsets.all(FnfSpace.lg),
                        child: ContentSourcePanel(content: widget.content, accent: widget.accent),
                      ),
                      Container(height: 1, color: FnfColors.line),
                      Expanded(
                        child: wide
                            ? Row(
                                children: [
                                  Expanded(flex: 5, child: list),
                                  Container(width: 1, color: FnfColors.line),
                                  Expanded(flex: 4, child: detail),
                                ],
                              )
                            : Column(
                                children: [
                                  Expanded(child: list),
                                  if (items.isNotEmpty) ...[
                                    Container(height: 1, color: FnfColors.line),
                                    SizedBox(height: 200, child: detail),
                                  ],
                                ],
                              ),
                      ),
                    ],
                  );
                },
              ),
            );
          },
        ),
      ),
    );
  }
}

class _Row extends StatelessWidget {
  const _Row({
    required this.item,
    required this.selected,
    required this.accent,
    required this.onTap,
  });

  final BrowseItem item;
  final bool selected;
  final Color accent;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      selected: selected,
      label: item.title,
      excludeSemantics: true,
      onTap: onTap,
      child: InkWell(
        key: ValueKey('browse-item-${item.id}'),
        onTap: onTap,
        child: Container(
          color: selected ? FnfColors.surfaceRaised : Colors.transparent,
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Container(width: selected ? 6 : 0, color: accent),
              Expanded(
                child: Padding(
                  padding: EdgeInsets.only(
                    left: selected ? FnfSpace.lg : FnfSpace.md,
                    right: FnfSpace.md,
                  ),
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        item.title.toUpperCase(),
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: FnfType.menuItem.copyWith(
                          fontSize: 22,
                          color: selected ? FnfColors.text : FnfColors.textMuted,
                        ),
                      ),
                      if (item.subtitle != null)
                        Text(
                          item.subtitle!,
                          maxLines: 1,
                          overflow: TextOverflow.ellipsis,
                          style: FnfType.hint,
                        ),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class _Detail extends StatelessWidget {
  const _Detail({required this.item, required this.accent});

  final BrowseItem item;
  final Color accent;

  @override
  Widget build(BuildContext context) {
    return SingleChildScrollView(
      padding: const EdgeInsets.all(FnfSpace.lg),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          const Text('SELECTED', style: FnfType.eyebrow),
          const SizedBox(height: FnfSpace.sm),
          Text(item.title.toUpperCase(), style: FnfType.display.copyWith(fontSize: 32)),
          if (item.subtitle != null) ...[
            const SizedBox(height: FnfSpace.xs),
            Text(item.subtitle!, style: FnfType.body.copyWith(color: FnfColors.textMuted)),
          ],
          if (item.chips.isNotEmpty) ...[
            const SizedBox(height: FnfSpace.md),
            Wrap(
              spacing: FnfSpace.sm,
              runSpacing: FnfSpace.sm,
              children: [
                for (final c in item.chips)
                  Container(
                    padding: const EdgeInsets.symmetric(horizontal: FnfSpace.sm, vertical: FnfSpace.xs),
                    decoration: BoxDecoration(border: Border.all(color: accent)),
                    child: Text(c.toUpperCase(), style: FnfType.tag.copyWith(color: accent)),
                  ),
              ],
            ),
          ],
          for (final f in item.facts) ...[
            const SizedBox(height: FnfSpace.md),
            Text(f.label, style: FnfType.eyebrow),
            const SizedBox(height: FnfSpace.xs),
            Text(f.value, style: FnfType.body),
          ],
          const SizedBox(height: FnfSpace.lg),
          Container(
            padding: const EdgeInsets.all(FnfSpace.md),
            decoration: BoxDecoration(border: Border.all(color: FnfColors.line)),
            child: const Text(
              'Playing is not available yet. It needs the engine adapter launch runner (ARCHITECTURE.md). This screen only lists what is in your folder.',
              style: FnfType.hint,
            ),
          ),
        ],
      ),
    );
  }
}
