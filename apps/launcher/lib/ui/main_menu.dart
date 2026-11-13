import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'developer_mode.dart';
import 'feature_status_screen.dart';
import 'fnf_tokens.dart';
import 'menu_background.dart';
import 'menu_entry.dart';

/// Keep in sync with apps/launcher/pubspec.yaml until a version source is wired.
const String kAppVersionLabel = 'v0.1.0';

/// The main menu IS the launcher: keyboard, controller and touch all drive the
/// same selection state.
///
/// - Arrow keys / D-pad: move selection (wraps).
/// - Enter / controller A: confirm (brief accent flash, then opens).
/// - Escape / controller B: back (inside sub-screens; the root has nowhere to go).
/// - Touch: tap an entry; optional on-screen up/down/confirm pad.
///
/// Motion: animated backdrop plus a beat bop on the selected entry. All of it
/// stops when the OS asks for reduced motion, when [animated] is false, or
/// while the menu is covered by another surface.
class MainMenuScreen extends StatefulWidget {
  const MainMenuScreen({
    super.key,
    required this.developerMode,
    required this.onOpenDeveloper,
    this.isActive = true,
    this.showTouchControls,
    this.animated = true,
  });

  final DeveloperModeController developerMode;
  final VoidCallback onOpenDeveloper;

  /// False while another surface (Developer tools) is covering the menu.
  final bool isActive;

  /// Null means: show on Android and iOS only.
  final bool? showTouchControls;

  /// Turns the backdrop and selection motion on or off.
  final bool animated;

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
  int _versionTaps = 0;
  DateTime? _lastVersionTap;
  bool _confirming = false;
  bool _reduceMotion = false;

  List<MenuEntry> get _entries => [
        ...kMainMenuEntries,
        if (widget.developerMode.unlocked) kDeveloperEntry,
      ];

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
    if (widget.isActive && !oldWidget.isActive) {
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

  void _move(int delta) {
    final count = _entries.length;
    final next = (((_selected + delta) % count) + count) % count;
    setState(() => _selected = next);
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
    if (_confirming) return;
    final entry = _entries[_selected];
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
            builder: (_) => FeatureStatusScreen(entry: entry),
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
      animation: widget.developerMode,
      builder: (context, _) {
        final entries = _entries;
        final selected =
            _selected >= entries.length ? entries.length - 1 : _selected;
        final entry = entries[selected];
        return Scaffold(
          body: Stack(
            children: [
              Positioned.fill(
                child: MenuBackground(beat: _beat, running: _beat.isAnimating),
              ),
              Focus(
                focusNode: _focus,
                autofocus: true,
                onKeyEvent: _onKey,
                child: SafeArea(
                  child: LayoutBuilder(
                    builder: (context, constraints) {
                      final wide = constraints.maxWidth >= _wideBreakpoint;
                      return wide
                          ? _buildWide(entries, selected, entry)
                          : _buildNarrow(entries, selected, entry);
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

  Widget _buildWide(List<MenuEntry> entries, int selected, MenuEntry entry) {
    return Row(
      children: [
        Expanded(
          flex: 5,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              const Padding(
                padding: EdgeInsets.fromLTRB(FnfSpace.xl, FnfSpace.xl, FnfSpace.xl, FnfSpace.lg),
                child: _Wordmark(),
              ),
              Expanded(child: _buildList(entries, selected)),
              _buildFooter(),
            ],
          ),
        ),
        Container(width: 1, color: FnfColors.line),
        Expanded(flex: 4, child: _DetailPanel(entry: entry)),
      ],
    );
  }

  Widget _buildNarrow(List<MenuEntry> entries, int selected, MenuEntry entry) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Padding(
          padding: EdgeInsets.fromLTRB(FnfSpace.lg, FnfSpace.lg, FnfSpace.lg, FnfSpace.md),
          child: _Wordmark(compact: true),
        ),
        Expanded(child: _buildList(entries, selected)),
        Padding(
          key: const ValueKey('menu-summary'),
          padding: const EdgeInsets.symmetric(horizontal: FnfSpace.lg, vertical: FnfSpace.sm),
          child: Text(
            entry.summary,
            style: FnfType.body.copyWith(color: FnfColors.textMuted),
          ),
        ),
        if (_touch) _buildTouchPad(),
        _buildFooter(),
      ],
    );
  }

  Widget _buildList(List<MenuEntry> entries, int selected) {
    final running = _beat.isAnimating;
    return ListView.builder(
      controller: _scroll,
      itemExtent: _itemExtent,
      itemCount: entries.length,
      itemBuilder: (context, index) => _MenuItem(
        entry: entries[index],
        selected: index == selected,
        confirming: _confirming && index == selected,
        beat: _beat,
        running: running,
        onTap: () {
          setState(() => _selected = index);
          _activate();
        },
      ),
    );
  }

  Widget _buildTouchPad() {
    return Padding(
      padding: const EdgeInsets.symmetric(horizontal: FnfSpace.md, vertical: FnfSpace.sm),
      child: Row(
        children: [
          Expanded(
            child: _PadButton(
              key: const ValueKey('pad-up'),
              icon: Icons.keyboard_arrow_up,
              label: 'Previous entry',
              onPressed: () => _move(-1),
            ),
          ),
          const SizedBox(width: FnfSpace.sm),
          Expanded(
            child: _PadButton(
              key: const ValueKey('pad-down'),
              icon: Icons.keyboard_arrow_down,
              label: 'Next entry',
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
  const _Wordmark({this.compact = false});

  final bool compact;

  @override
  Widget build(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        const Text('FNF', style: FnfType.eyebrow),
        const SizedBox(height: FnfSpace.xs),
        Text(
          'UNIVERSAL',
          style: FnfType.display.copyWith(fontSize: compact ? 38 : 52),
        ),
        const SizedBox(height: FnfSpace.sm),
        Container(width: 72, height: 6, color: FnfColors.accent),
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
    required this.running,
    required this.onTap,
  });

  final MenuEntry entry;
  final bool selected;
  final bool confirming;
  final Animation<double> beat;
  final bool running;
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
    if (selected && running) {
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
              ? FnfColors.accent
              : (selected ? FnfColors.surfaceRaised : Colors.transparent),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              AnimatedContainer(
                duration: FnfMotion.fast,
                curve: FnfMotion.curve,
                width: selected ? 8 : 0,
                color: FnfColors.accent,
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
                child: Center(child: StatusTag(status: entry.status)),
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
  const StatusTag({super.key, required this.status});

  final MenuStatus status;

  @override
  Widget build(BuildContext context) {
    final color =
        status == MenuStatus.ready ? FnfColors.accent : FnfColors.textMuted;
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
  const _DetailPanel({required this.entry});

  final MenuEntry entry;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(FnfSpace.xl),
      child: AnimatedSwitcher(
        duration: FnfMotion.medium,
        child: Column(
          key: ValueKey('detail-${entry.id}'),
          crossAxisAlignment: CrossAxisAlignment.start,
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            const Text('SELECTED', style: FnfType.eyebrow),
            const SizedBox(height: FnfSpace.sm),
            Text(entry.label.toUpperCase(), style: FnfType.display.copyWith(fontSize: 40)),
            const SizedBox(height: FnfSpace.md),
            StatusTag(status: entry.status),
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
    );
  }
}

class _PadButton extends StatelessWidget {
  const _PadButton({
    super.key,
    required this.icon,
    required this.label,
    required this.onPressed,
    this.primary = false,
  });

  final IconData icon;
  final String label;
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
        color: primary ? FnfColors.accent : FnfColors.surfaceRaised,
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
