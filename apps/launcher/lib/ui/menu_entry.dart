enum MenuStatus { ready, planned }

extension MenuStatusLabel on MenuStatus {
  String get label => switch (this) {
        MenuStatus.ready => 'Ready',
        MenuStatus.planned => 'Not in this build',
      };
}

class MenuEntry {
  const MenuEntry({
    required this.id,
    required this.label,
    required this.summary,
    required this.status,
    this.requires,
  });

  final String id;
  final String label;
  final String summary;
  final MenuStatus status;

  /// Honest description of what is missing, shown for entries that are not
  /// built yet. Null when the entry works.
  final String? requires;

  /// Only text can be restyled. Status and what-it-needs always come from the
  /// built-in entry, so a mod cannot make an unbuilt feature look ready.
  MenuEntry copyWith({String? label, String? summary}) => MenuEntry(
        id: id,
        label: label ?? this.label,
        summary: summary ?? this.summary,
        status: status,
        requires: requires,
      );
}

const kMainMenuEntries = <MenuEntry>[
  MenuEntry(
    id: 'story',
    label: 'Story Mode',
    summary: 'Play weeks in order with a fixed song list.',
    status: MenuStatus.planned,
    requires:
        'A runtime to launch. The engine adapter launch runner is not implemented yet (ARCHITECTURE.md, adapter lifecycle step 5).',
  ),
  MenuEntry(
    id: 'freeplay',
    label: 'Freeplay',
    summary: 'Pick any installed song and difficulty.',
    status: MenuStatus.planned,
    requires:
        'The asset index wired into a song browser, plus the same launch runner Story Mode needs.',
  ),
  MenuEntry(
    id: 'mods',
    label: 'Mods',
    summary: 'Install, enable and order mods.',
    status: MenuStatus.planned,
    requires:
        'An installer and mod list on top of manifest validation. Android storage must go through the Storage Access Framework (README design rule 8).',
  ),
  MenuEntry(
    id: 'settings',
    label: 'Settings',
    summary: 'Audio, video, controls and accessibility.',
    status: MenuStatus.planned,
    requires:
        'Player-owned input mapping lives in the Controller Editor, which is not built yet.',
  ),
  MenuEntry(
    id: 'credits',
    label: 'Credits',
    summary: 'Who made it and which licenses apply.',
    status: MenuStatus.planned,
    requires: 'A credits screen that reads LICENSES.md. Not built yet.',
  ),
];

/// Appended to the menu only while Developer Mode is unlocked.
const kDeveloperEntry = MenuEntry(
  id: 'developer',
  label: 'Developer',
  summary: 'Open the mod authoring tools: project, chart, character and stage editors.',
  status: MenuStatus.ready,
);

MenuEntry? builtInEntry(String id) {
  for (final entry in kMainMenuEntries) {
    if (entry.id == id) return entry;
  }
  return null;
}
