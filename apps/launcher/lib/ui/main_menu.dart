import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'developer_mode.dart';
import 'feature_status_screen.dart';
import 'fnf_tokens.dart';
import 'launcher_ui_controller.dart';
import 'menu_background.dart';
import 'menu_definition.dart';
import 'menu_entry.dart';

/// Keep in sync with apps/launcher/pubspec.yaml until a version source is wired.
const String kAppVersionLabel = 'v0.1.0';

/// The main menu IS the launcher: keyboard, controller and touch all drive the
/// same selection state. Its look (entries, title, accent, background) comes
/// from [LauncherUiController], which the Menu/UI editor and mod packages edit.
///
/// - Arrow keys / D-pad: move selection (wraps).
/// - Enter / controller A: confirm (brief accent flash, then opens).
/// - Escape / controller B: back (inside sub-screens).
/// - Touch: tap an entry; optional on-screen up/down/confirm pad.
///
/// Motion stops when the OS asks for reduced motion, when [animated] is false,
/// or while the menu is covered by another surface.
class MainMenuScreen extends StatefulWidget {
  const MainMenuScreen({
    super.key,
    required this.developerMode,
    required this.ui,
    required this.onOpenDeveloper,
    this.isActive = true,
    this.showTouchControls,
    this.animated = true,
    this.preview = false,
  });

  final DeveloperModeController developerMode;
  final LauncherUiController ui;
  final VoidCallback onOpenDeveloper;

  /// False while another surface (Developer tools) is covering the menu.
  final bool isActive;

  /// Null means: show on Android and iOS only.
  final bool? showTouchControls;

  /// Turns the backdrop and selection motion on or off.
  final bool animated;

  /// Read-only mode used by the editor's live preview: no autofocus, no keys,
  /// no navigation.
  final bool preview;

  @override
  State<MainMenuScreen> createState() => _MainMenuScreenState();
}

class _MainMenuScreenState extends State<MainMenuScreen>
    with SingleTickerProviderStateMixin {
  static const double _itemExtent = 76;
  static const double _wideBreakpoint = 900;
  static const Duration _confirmFlash = Duration(milliseconds: 140);

  final FocusNode _focus = FocusNode(debugLabel: 'main-menu');
  final ScrollController _scroll = ScrollController();
  late final AnimationController _beat;
  int _selected = 0;

  /// Bumped on every selection change so the detail panel's fade always gets a
  /// fresh key (quick A, B, A moves would otherwise reuse a key still fading
  /// out and crash).
  int _detailSerial = 0;
  int _versionTaps = 0;
  DateTime? _lastVersionTap;
  bool _confirming = false;
  bool _reduceMotion = false;

  List<MenuEntry> get _entries => resolveMenuEntries(
        widget.ui.definition,
        developerUnlocked: widget.developerMode.unlocked,
      );

  int _safeIndex(List<MenuEntry> entries) =>
      _selected >= entries.length ? entries.length - 1 : _selected;

  bool get _touch =>
      widget.showTouchControls ??
      (defaultTargetPlatform == TargetPlatform.android ||
          defaultTargetPlatform == TargetPlatform.iOS);

  @override
  void initState() {
    super.initState();
    _beat = AnimationController(vsync: this, duration: kMenuBar);
  }

  @override
  void didChangeDependencies() {
    super.didChangeDependencies();
    _syncAnimation();
  }

  @override
  void didUpdateWidget(MainMenuScreen oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (widget.isActive && !oldWidget.isActive && !widget.preview) {
      _focus.requestFocus();
    }
    _syncAnimation();
  }

  void _syncAnimation() {
    _reduceMotion = MediaQuery.of(context).disableAnimations;
    final shouldRun = widget.animated && widget.isActive && !_reduceMotion;
    if (shouldRun) {
      if (!_beat.isAnimating) _beat.repeat();
    } else if (_beat.isAnimating) {
      _beat.stop();
      WidgetsBinding.instance.addPostFrameCallback((_) {
        if (mounted) _beat.value = 0;
      });
    }
  }

  @override
  void dispose() {
    _beat.dispose();
    _focus.dispose();
    _scroll.dispose();
    super.dispose();
  }

  void _select(int index) {
    if (index == _selected) return;
    _selected = index;
    _detailSerial++;
  }

  void _move(int delta) {
    final count = _entries.length;
    final next = (((_selected + delta) % count) + count) % count;
    setState(() => _select(next));
    _scrollToSelected();
  }

  void _scrollToSelected() {
    if (!_scroll.hasClients) return;
    final position = _scroll.position;
    final target = (_selected * _itemExtent -
            (position.viewportDimension - _itemExtent) / 2)
        .clamp(0.0, position.maxScrollExtent)
        .toDouble();
    _scroll.animateTo(target, duration: FnfMotion.fast, curve: FnfMotion.curve);
  }

  Future<void> _activate() async {
    if (_confirming || widget.preview) return;
    final entries = _entries;
    final entry = entries[_safeIndex(entries)];
    if (widget.animated && !_reduceMotion) {
      setState(() => _confirming = true);
      await Future<void>.delayed(_confirmFlash);
      if (!mounted) return;
      setState(() => _confirming = false);
    }
    if (entry.id == kDeveloperEntry.id) {
      widget.onOpenDeveloper();
      return;
    }
    if (!mounted) return;
    Navigator.of(context)
        .push<void>(
          MaterialPageRoute<void>(
            builder: (_) => FeatureStatusScreen(
              entry: entry,
              accent: widget.ui.definition.accent,
            ),
          ),
        )
        .then((_) {
      if (mounted) _focus.requestFocus();
    });
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent event) {
    if (event is! KeyDownEvent && event is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final key = event.logicalKey;
    if (key == LogicalKeyboardKey.arrowDown) {
      _move(1);
      return KeyEventResult.handled;
    }
    if (key == LogicalKeyboardKey.arrowUp) {
      _move(-1);
      return KeyEventResult.handled;
    }
    if (event is KeyDownEvent &&
        (key == LogicalKeyboardKey.enter ||
            key == LogicalKeyboardKey.numpadEnter ||
            key == LogicalKeyboardKey.gameButtonA ||
            key == LogicalKeyboardKey.select)) {
      _activate();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  /// Temporary hidden unlock: seven quick taps on the version label. Replace
  /// with the Controller Editor control when that editor exists.
  void _onVersionTap() {
    if (widget.developerMode.unlocked) return;
    final now = DateTime.now();
    final last = _lastVersionTap;
    if (last == null || now.difference(last) > const Duration(seconds: 2)) {
      _versionTaps = 0;
    }
    _lastVersionTap = now;
    _versionTaps++;
    if (_versionTaps >= 7) {
      _versionTaps = 0;
      widget.developerMode.unlock();
      ScaffoldMessenger.of(context).showSnackBar(
        const SnackBar(content: Text('Developer Mode enabled for this session')),
      );
    }
  }

  @override
  Widget build(BuildContext context) {
    return AnimatedBuilder(
      animation: Listenable.merge([widget.developerMode, widget.ui]),
      builder: (context, _) {
        final ui = widget.ui.definition;
        final entries = _entries;
        final selected = _safeIndex(entries);
        final entry = entries[selected];
        return Scaffold(
          body: Stack(
            children: [
              Positioned.fill(
                child: MenuBackground(
                  beat: _beat,
                  running: _beat.isAnimating,
                  accent: ui.accent,
                  style: ui.background,
                  imageBytes: widget.ui.backgroundBytes,
                  dim: ui.backgroundDim,
                ),
              ),
              Focus(
                focusNode: _focus,
                autofocus: !widget.preview,
                canRequestFocus: !widget.preview,
                onKeyEvent: _onKey,
                child: SafeArea(
                  child: LayoutBuilder(
                    builder: (context, constraints) {
                      final wide = constraints.maxWidth >= _wideBreakpoint;
                      return wide
                          ? _buildWide(entries, selected, entry, ui)
                          : _buildNarrow(entries, selected, entry, ui);
                    },
                  ),
                ),
              ),
            ],
          ),
        );
      },
    );
  }

  Widget _buildWide(
    List<MenuEntry> entries,
    int selected,
    MenuEntry entry,
    LauncherUiDefinition ui,
  ) {
    return Row(
      children: [
        Expanded(
          flex: 5,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Padding(
                padding: const EdgeInsets.fromLTRB(FnfSpace.xl, FnfSpace.xl, FnfSpace.xl, FnfSpace.lg),
                child: _Wordmark(
                  top: ui.wordmarkTop,
                  main: ui.wordmarkMain,
                  accent: ui.accent,
                ),
              ),
              Expanded(child: _buildList(entries, selected, ui)),
              _buildFooter(),
            ],
          ),
        ),
        Container(width: 1, color: FnfColors.line),
        Expanded(
          flex: 4,
          child: _DetailPanel(entry: entry, accent: ui.accent, serial: _detailSerial),
        ),
      ],
    );
  }

  Widget _buildNarrow(
    List<MenuEntry> entries,
    int selected,
    MenuEntry entry,
    LauncherUiDefinition ui,
  ) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Padding(
          padding: const EdgeInsets.fromLTRB(FnfSpace.lg, FnfSpace.lg, FnfSpace.lg, FnfSpace.md),
          child: _Wordmark(
            top: ui.wordmarkTop,
            main: ui.wordmarkMain,
            accent: ui.accent,
            compact: true,
          ),
        ),
        Expanded(child: _buildList(entries, selected, ui)),
        Padding(
          key: const ValueKey('menu-summary'),
          padding: const EdgeInsets.symmetric(horizontal: FnfSpace.lg, vertical: FnfSpace.sm),
          child: Text(
            entry.summary,
            style: FnfType.body.copyWith(color: FnfColors.textMuted),
          ),
        ),
        if (_touch) _buildTouchPad(ui.accent),
        _buildFooter(),
      ],
    );
  }

  Widget _buildList(List<MenuEntry> entries, int selected, LauncherUiDefinition ui) {
    final bop = _beat.isAnimating && ui.beatBop;
    return ListView.builder(
      controller: _scroll,
      itemExtent: _itemExtent,
      itemCount: entries.length,
      itemBuilder: (context, index) => _MenuItem(
        entry: entries[index],
        selected: index == selected,
        confirming: _confirming && index == selected,
        beat: _beat,
        bop: bop,
        accent: ui.accent,
        onTap: () {
          setState(() => _select(index));
          _activate();
        },
      ),
    );
  }

  Widget _buildTouchPad(Color accent) {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: FnfSpace.md, vertical: FnfSpace.sm),
      child: Row(
        children: [
          Expanded(
            child: _PadButton(
              key: const ValueKey('pad-up'),
              icon: Icons.keyboard_arrow_up,
              label: 'Previous entry',
              accent: accent,
              onPressed: () => _move(-1),
            ),
          ),
          const SizedBox(width: FnfSpace.sm),
          Expanded(
            child: _PadButton(
              key: const ValueKey('pad-down'),
              icon: Icons.keyboard_arrow_down,
              label: 'Next entry',
              accent: accent,
              onPressed: () => _move(1),
            ),
          ),
          const SizedBox(width: FnfSpace.sm),
          Expanded(
            flex: 2,
            child: _PadButton(
              key: const ValueKey('pad-confirm'),
              icon: Icons.check,
              label: 'Confirm',
              primary: true,
              accent: accent,
              onPressed: _activate,
            ),
          ),
        ],
      ),
    );
  }

  Widget _buildFooter() {
    return Padding(
      padding: const EdgeInsets.fromLTRB(FnfSpace.xl, 0, FnfSpace.sm, FnfSpace.sm),
      child: Row(
        children: [
          if (_touch)
            const Spacer()
          else
            const Expanded(
              child: Text(
                '\u2191 \u2193  SELECT     ENTER  CONFIRM     ESC  BACK',
                style: FnfType.hint,
                overflow: TextOverflow.ellipsis,
              ),
            ),
          InkWell(
            key: const ValueKey('version-tag'),
            onTap: _onVersionTap,
            child: const Padding(
              padding: EdgeInsets.all(FnfSpace.md),
              child: Text(kAppVersionLabel, style: FnfType.hint),
            ),
          ),
        ],
      ),
    );
  }
}

class _Wordmark extends StatelessWidget {
  const _Wordmark({
    required this.top,
    required this.main,
    required this.accent,
    this.compact = false,
  });

  final String top;
  final String main;
  final Color accent;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    final title = main.trim().isEmpty ? 'UNIVERSAL' : main.toUpperCase();
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        Text(top.toUpperCase(), style: FnfType.eyebrow),
        const SizedBox(height: FnfSpace.xs),
        FittedBox(
          fit: BoxFit.scaleDown,
          alignment: Alignment.centerLeft,
          child: Text(
            title,
            maxLines: 1,
            style: FnfType.display.copyWith(fontSize: compact ? 38 : 52),
          ),
        ),
        const SizedBox(height: FnfSpace.sm),
        Container(width: 72, height: 6, color: accent),
      ],
    );
  }
}

class _MenuItem extends StatelessWidget {
  const _MenuItem({
    required this.entry,
    required this.selected,
    required this.confirming,
    required this.beat,
    required this.bop,
    required this.accent,
    required this.onTap,
  });

  final MenuEntry entry;
  final bool selected;
  final bool confirming;
  final Animation<double> beat;
  final bool bop;
  final Color accent;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final textColor = confirming
        ? FnfColors.onAccent
        : (selected ? FnfColors.text : FnfColors.textMuted);
    Widget label = Text(
      entry.label.toUpperCase(),
      maxLines: 1,
      overflow: TextOverflow.ellipsis,
      style: FnfType.menuItem.copyWith(color: textColor),
    );
    if (selected && bop) {
      label = AnimatedBuilder(
        animation: beat,
        child: label,
        builder: (context, child) => Transform.scale(
          scale: 1 + 0.035 * beatPulse(beat.value),
          alignment: Alignment.centerLeft,
          child: child,
        ),
      );
    }
    return Semantics(
      button: true,
      selected: selected,
      label: '${entry.label}, ${entry.status.label}',
      excludeSemantics: true,
      onTap: onTap,
      child: InkWell(
        key: ValueKey('menu-${entry.id}'),
        onTap: onTap,
        child: AnimatedContainer(
          duration: FnfMotion.fast,
          curve: FnfMotion.curve,
          color: confirming
              ? accent
              : (selected ? FnfColors.surfaceRaised : Colors.transparent),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              AnimatedContainer(
                duration: FnfMotion.fast,
                curve: FnfMotion.curve,
                width: selected ? 8 : 0,
                color: accent,
              ),
              Expanded(
                child: AnimatedPadding(
                  duration: FnfMotion.fast,
                  curve: FnfMotion.curve,
                  padding: EdgeInsets.only(left: selected ? FnfSpace.xl : FnfSpace.lg),
                  child: Align(
                    alignment: Alignment.centerLeft,
                    child: label,
                  ),
                ),
              ),
              Padding(
                padding: const EdgeInsets.only(right: FnfSpace.md),
                child: Center(child: StatusTag(status: entry.status, accent: accent)),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Square status label. Accent = works, muted = not built yet.
class StatusTag extends StatelessWidget {
  const StatusTag({
    super.key,
    required this.status,
    this.accent = FnfColors.accent,
  });

  final MenuStatus status;
  final Color accent;

  @override
  Widget build(BuildContext context) {
    final color = status == MenuStatus.ready ? accent : FnfColors.textMuted;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: FnfSpace.sm, vertical: FnfSpace.xs),
      decoration: BoxDecoration(border: Border.all(color: color)),
      child: Text(
        status.label.toUpperCase(),
        style: FnfType.tag.copyWith(color: color),
      ),
    );
  }
}

class _DetailPanel extends StatelessWidget {
  const _DetailPanel({
    required this.entry,
    required this.accent,
    required this.serial,
  });

  final MenuEntry entry;
  final Color accent;
  final int serial;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(FnfSpace.xl),
      child: AnimatedSwitcher(
        duration: FnfMotion.medium,
        child: LayoutBuilder(
          key: ValueKey('detail-${entry.id}-$serial'),
          builder: (context, constraints) => SingleChildScrollView(
            child: ConstrainedBox(
              constraints: BoxConstraints(minHeight: constraints.maxHeight),
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.start,
                mainAxisAlignment: MainAxisAlignment.center,
                children: [
                  const Text('SELECTED', style: FnfType.eyebrow),
                  const SizedBox(height: FnfSpace.sm),
                  Text(entry.label.toUpperCase(), style: FnfType.display.copyWith(fontSize: 40)),
                  const SizedBox(height: FnfSpace.md),
                  StatusTag(status: entry.status, accent: accent),
                  const SizedBox(height: FnfSpace.lg),
                  Text(entry.summary, style: FnfType.body),
                  if (entry.requires != null) ...[
                    const SizedBox(height: FnfSpace.lg),
                    const Text('WHAT IT NEEDS', style: FnfType.eyebrow),
                    const SizedBox(height: FnfSpace.sm),
                    Text(
                      entry.requires!,
                      style: FnfType.body.copyWith(color: FnfColors.textMuted),
                    ),
                  ],
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }
}

class _PadButton extends StatelessWidget {
  const _PadButton({
    super.key,
    required this.icon,
    required this.label,
    required this.accent,
    required this.onPressed,
    this.primary = false,
  });

  final IconData icon;
  final String label;
  final Color accent;
  final VoidCallback onPressed;
  final bool primary;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: label,
      excludeSemantics: true,
      onTap: onPressed,
      child: Material(
        color: primary ? accent : FnfColors.surfaceRaised,
        child: InkWell(
          onTap: onPressed,
          child: SizedBox(
            height: 56,
            child: Center(
              child: Icon(
                icon,
                size: 28,
                color: primary ? FnfColors.onAccent : FnfColors.text,
              ),
            ),
          ),
        ),
      ),
    );
  }
}
