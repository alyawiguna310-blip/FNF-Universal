import 'package:flutter/material.dart';

import '../main.dart' show EditorShell;
import 'developer_mode.dart';
import 'fnf_tokens.dart';
import 'main_menu.dart';

/// App shell with the main menu as the launcher. The existing editor
/// workspace (EditorShell in main.dart) is reused unchanged and only reachable
/// while Developer Mode is unlocked.
class FnfMenuApp extends StatelessWidget {
  const FnfMenuApp({super.key, this.animated = true});

  /// Turns menu motion (backdrop, beat bop, confirm flash) on or off.
  final bool animated;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'FNF Universal',
      debugShowCheckedModeBanner: false,
      theme: buildFnfTheme(),
      home: FnfRoot(animated: animated),
    );
  }
}

class FnfRoot extends StatefulWidget {
  const FnfRoot({super.key, this.animated = true});

  final bool animated;

  @override
  State<FnfRoot> createState() => _FnfRootState();
}

class _FnfRootState extends State<FnfRoot> {
  final DeveloperModeController _developerMode = DeveloperModeController();
  bool _studioOpen = false;
  bool _studioBuilt = false;

  @override
  void dispose() {
    _developerMode.dispose();
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
          onOpenDeveloper: _openStudio,
          isActive: !_studioOpen,
          animated: widget.animated,
        ),
        _studioBuilt
            ? _StudioHost(onExit: _closeStudio)
            : const SizedBox.shrink(),
      ],
    );
  }
}

class _StudioHost extends StatelessWidget {
  const _StudioHost({required this.onExit});

  final VoidCallback onExit;

  @override
  Widget build(BuildContext context) {
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
                    onPressed: onExit,
                    icon: const Icon(Icons.arrow_back),
                    label: const Text('MENU'),
                  ),
                  const Spacer(),
                  Padding(
                    padding: const EdgeInsets.only(right: FnfSpace.md),
                    child: Text(
                      'DEVELOPER MODE',
                      style: FnfType.tag.copyWith(color: FnfColors.accent),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
        Expanded(
          child: MediaQuery.removePadding(
            context: context,
            removeTop: true,
            child: const EditorShell(),
          ),
        ),
      ],
    );
  }
}
