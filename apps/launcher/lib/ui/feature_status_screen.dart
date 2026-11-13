import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'fnf_tokens.dart';
import 'main_menu.dart' show StatusTag;
import 'menu_entry.dart';

/// Shown for menu entries that are not built yet. It says plainly what is
/// missing instead of pretending the feature exists. Escape, controller B and
/// the back button all return to the menu.
class FeatureStatusScreen extends StatelessWidget {
  const FeatureStatusScreen({
    super.key,
    required this.entry,
    this.accent = FnfColors.accent,
  });

  final MenuEntry entry;
  final Color accent;

  @override
  Widget build(BuildContext context) {
    void back() => Navigator.of(context).maybePop();
    return CallbackShortcuts(
      bindings: {
        const SingleActivator(LogicalKeyboardKey.escape): back,
        const SingleActivator(LogicalKeyboardKey.gameButtonB): back,
      },
      child: Focus(
        autofocus: true,
        child: Scaffold(
          key: ValueKey('feature-status-${entry.id}'),
          body: SafeArea(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(FnfSpace.lg),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 640),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    TextButton.icon(
                      key: const ValueKey('status-back'),
                      onPressed: back,
                      icon: const Icon(Icons.arrow_back),
                      label: const Text('BACK'),
                    ),
                    const SizedBox(height: FnfSpace.lg),
                    Text(entry.label.toUpperCase(), style: FnfType.display),
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
      ),
    );
  }
}
