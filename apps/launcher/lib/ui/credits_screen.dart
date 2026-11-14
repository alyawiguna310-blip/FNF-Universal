import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'fnf_tokens.dart';
import 'main_menu.dart' show StatusTag;
import 'menu_entry.dart';

class CreditSection {
  const CreditSection(this.title, this.lines);

  final String title;
  final List<String> lines;
}

/// Credits shown in the app. Edit here to add contributors.
const List<CreditSection> kCreditSections = [
  CreditSection('FNF UNIVERSAL', [
    'An independent, cross-platform rhythm-game engine and modding ecosystem.',
    'Built by alyawiguna310-blip and contributors.',
    'github.com/alyawiguna310-blip/FNF-Universal',
  ]),
  CreditSection('FRIDAY NIGHT FUNKIN\u2019', [
    'Friday Night Funkin\u2019 was created by The Funkin\u2019 Crew Inc. (ninjamuffin99, PhantomArcade, Kawai Sprite and evilsk8r).',
    'FNF Universal is an independent project. It is not affiliated with or endorsed by them.',
    'Please support the original game: github.com/FunkinCrew/Funkin',
  ]),
  CreditSection('CODENAME ENGINE', [
    'Codename Engine is made by the CodenameCrew. FNF Universal can list the weeks and songs of Codename mods you already have. It is not affiliated with or endorsed by them.',
    'github.com/CodenameCrew/CodenameEngine',
  ]),
  CreditSection('NO GAME CONTENT INCLUDED', [
    'This app does not bundle the original game\u2019s music, art or charts, or any mod. Point it at your own copy of the game, or at a mod folder you have the right to use.',
  ]),
  CreditSection('DOCUMENTATION', [
    'Level and song data layouts follow the Funkin\u2019 Modding Docs, licensed CC BY-SA 4.0: funkincrew.github.io/funkin-modding-docs',
    'Codename mod layouts follow the Codename Engine wiki: codename-engine.com/wiki',
  ]),
  CreditSection('LICENSES', [
    'See LICENSES.md in the repository for every license that applies.',
  ]),
];

class CreditsScreen extends StatelessWidget {
  const CreditsScreen({super.key, this.accent = FnfColors.accent});

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
          key: const ValueKey('credits-screen'),
          body: SafeArea(
            child: SingleChildScrollView(
              padding: const EdgeInsets.all(FnfSpace.lg),
              child: ConstrainedBox(
                constraints: const BoxConstraints(maxWidth: 720),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    TextButton.icon(
                      key: const ValueKey('credits-back'),
                      onPressed: back,
                      icon: const Icon(Icons.arrow_back),
                      label: const Text('BACK'),
                    ),
                    const SizedBox(height: FnfSpace.lg),
                    Text('CREDITS', style: FnfType.display),
                    const SizedBox(height: FnfSpace.md),
                    StatusTag(status: MenuStatus.ready, accent: accent),
                    for (final section in kCreditSections) ...[
                      const SizedBox(height: FnfSpace.xl),
                      Row(
                        children: [
                          Container(width: 6, height: 18, color: accent),
                          const SizedBox(width: FnfSpace.sm),
                          Text(section.title, style: FnfType.eyebrow),
                        ],
                      ),
                      const SizedBox(height: FnfSpace.sm),
                      for (final line in section.lines)
                        Padding(
                          padding: const EdgeInsets.only(bottom: FnfSpace.sm),
                          child: SelectableText(line, style: FnfType.body),
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
