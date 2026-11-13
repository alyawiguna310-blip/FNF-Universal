import 'dart:async';

import 'package:flutter/material.dart';

import '../main.dart' show EditorShell;
import 'developer_mode.dart';
import 'fnf_tokens.dart';
import 'launcher_ui_controller.dart';
import 'main_menu.dart';
import 'menu_ui_editor.dart';
import 'package_screen.dart';

/// App shell with the main menu as the launcher. The existing editor
/// workspace (EditorShell in main.dart) is reused unchanged and, with the
/// Menu/UI editor and the Package tool, is only reachable while Developer Mode
/// is unlocked.
class FnfMenuApp extends StatelessWidget {
  const FnfMenuApp({super.key, this.animated = true, this.uiController});

  /// Turns menu motion (backdrop, beat bop, confirm flash) on or off.
  final bool animated;

  /// Supply one to control the launcher look from outside (tests do). If null,
  /// the app loads and saves <Documents>/FNFUniversal/launcher_ui.json.
  final LauncherUiController? uiController;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'FNF Universal',
      debugShowCheckedModeBanner: false,
      theme: buildFnfTheme(),
      home: FnfRoot(animated: animated, uiController: uiController),
    );
  }
}

class FnfRoot extends StatefulWidget {
  const FnfRoot({super.key, this.animated = true, this.uiController});

  final bool animated;
  final LauncherUiController? uiController;

  @override
  State<FnfRoot> createState() => _FnfRootState();
}

class _FnfRootState extends State<FnfRoot> {
  final DeveloperModeController _developerMode = DeveloperModeController();
  late final LauncherUiController _ui;
  late final bool _ownsUi;
  bool _studioOpen = false;
  bool _studioBuilt = false;

  @override
  void initState() {
    super.initState();
    final supplied = widget.uiController;
    _ownsUi = supplied == null;
    _ui = supplied ?? LauncherUiController(storage: FileLauncherUiStorage());
    if (_ownsUi) unawaited(_ui.load());
  }

  @override
  void dispose() {
    _developerMode.dispose();
    if (_ownsUi) _ui.dispose();
    super.dispose();
  }

  void _openStudio() => setState(() {
        _studioOpen = true;
        _studioBuilt = true;
      });

  void _closeStudio() => setState(() => _studioOpen = false);

  @override
  Widget build(BuildContext context) {
    // IndexedStack keeps the editor mounted while you are on the menu, so
    // unsaved editor state is not lost when you go back and forth.
    return IndexedStack(
      index: _studioOpen ? 1 : 0,
      children: [
        MainMenuScreen(
          developerMode: _developerMode,
          ui: _ui,
          onOpenDeveloper: _openStudio,
          isActive: !_studioOpen,
          animated: widget.animated,
        ),
        _studioBuilt
            ? _StudioHost(
                onExit: _closeStudio,
                ui: _ui,
                animated: widget.animated,
              )
            : const SizedBox.shrink(),
      ],
    );
  }
}

enum _DevTab { studio, menuUi, package }

class _StudioHost extends StatefulWidget {
  const _StudioHost({
    required this.onExit,
    required this.ui,
    required this.animated,
  });

  final VoidCallback onExit;
  final LauncherUiController ui;
  final bool animated;

  @override
  State<_StudioHost> createState() => _StudioHostState();
}

class _StudioHostState extends State<_StudioHost> {
  _DevTab _tab = _DevTab.studio;
  final Set<_DevTab> _visited = {_DevTab.studio};

  Widget _tabButton(_DevTab tab, String label) {
    final selected = _tab == tab;
    final accent = widget.ui.definition.accent;
    return DecoratedBox(
      decoration: BoxDecoration(
        border: Border(
          bottom: BorderSide(color: selected ? accent : Colors.transparent, width: 3),
        ),
      ),
      child: TextButton(
        key: ValueKey('dev-tab-${tab.name}'),
        onPressed: () => setState(() {
          _tab = tab;
          _visited.add(tab);
        }),
        style: TextButton.styleFrom(
          foregroundColor: selected ? accent : FnfColors.textMuted,
          minimumSize: const Size(48, 48),
          shape: const RoundedRectangleBorder(),
        ),
        child: Text(label, style: FnfType.tag),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final wide = MediaQuery.sizeOf(context).width >= 700;
    return Column(
      children: [
        Material(
          color: FnfColors.surface,
          child: SafeArea(
            bottom: false,
            child: SizedBox(
              height: 48,
              child: Row(
                children: [
                  TextButton.icon(
                    key: const ValueKey('studio-exit'),
                    onPressed: widget.onExit,
                    icon: const Icon(Icons.arrow_back),
                    label: const Text('MENU'),
                  ),
                  Expanded(
                    child: SingleChildScrollView(
                      scrollDirection: Axis.horizontal,
                      child: Row(
                        children: [
                          _tabButton(_DevTab.studio, 'STUDIO'),
                          _tabButton(_DevTab.menuUi, 'MENU / UI'),
                          _tabButton(_DevTab.package, 'PACKAGE'),
                        ],
                      ),
                    ),
                  ),
                  if (wide)
                    Padding(
                      padding: const EdgeInsets.only(right: FnfSpace.md),
                      child: Text(
                        'DEVELOPER MODE',
                        style: FnfType.tag.copyWith(color: widget.ui.definition.accent),
                      ),
                    ),
                ],
              ),
            ),
          ),
        ),
        Expanded(
          child: IndexedStack(
            index: _tab.index,
            children: [
              MediaQuery.removePadding(
                context: context,
                removeTop: true,
                child: const EditorShell(),
              ),
              _visited.contains(_DevTab.menuUi)
                  ? MenuUiEditorScreen(controller: widget.ui, animated: widget.animated)
                  : const SizedBox.shrink(),
              _visited.contains(_DevTab.package)
                  ? PackageScreen(ui: widget.ui)
                  : const SizedBox.shrink(),
            ],
          ),
        ),
      ],
    );
  }
}
