import 'package:flutter/material.dart';

import '../content/content_catalog.dart';
import '../content/content_controller.dart';
import 'browse_screen.dart';

/// Freeplay: every song found in the player's folder, in level order, then any
/// song that is in no level.
class FreeplayScreen extends StatelessWidget {
  const FreeplayScreen({super.key, required this.content, required this.accent});

  final ContentController content;
  final Color accent;

  @override
  Widget build(BuildContext context) {
    return BrowseScreen(
      screenId: 'freeplay',
      title: 'Freeplay',
      content: content,
      accent: accent,
      emptyMessage:
          'No songs yet. Point FNF Universal at your own copy of the game, or a mod folder you have the right to use, then press SCAN.',
      itemsOf: (ContentCatalog catalog) => [
        for (final s in catalog.freeplaySongs)
          BrowseItem(
            id: '${s.origin ?? ''}/${s.id}',
            title: s.title,
            subtitle: s.artist ?? s.origin,
            chips: s.difficulties,
            facts: [
              if (s.bpm != null) (label: 'BPM', value: s.bpm!.round().toString()),
              (label: 'ID', value: s.id),
              if (s.origin != null) (label: 'FROM', value: s.origin!),
            ],
          ),
      ],
    );
  }
}

/// Story: the visible levels (weeks) from the player's folder, in file order.
class StoryScreen extends StatelessWidget {
  const StoryScreen({super.key, required this.content, required this.accent});

  final ContentController content;
  final Color accent;

  @override
  Widget build(BuildContext context) {
    return BrowseScreen(
      screenId: 'story',
      title: 'Story Mode',
      content: content,
      accent: accent,
      emptyMessage:
          'No story levels yet. Point FNF Universal at your own copy of the game, or a mod folder you have the right to use, then press SCAN.',
      itemsOf: (ContentCatalog catalog) => [
        for (final level in catalog.storyLevels)
          BrowseItem(
            id: '${level.origin ?? ''}/${level.id}',
            title: level.name,
            subtitle: '${level.songIds.length} song(s)'
                '${level.origin == null ? '' : '  \u00b7  ${level.origin}'}',
            facts: [
              (
                label: 'SONGS',
                value: level.songIds.isEmpty
                    ? 'None listed'
                    : level.songIds
                        .map((id) => catalog.songs[id]?.title ?? id)
                        .join('  /  '),
              ),
              if (level.origin != null) (label: 'FROM', value: level.origin!),
            ],
          ),
      ],
    );
  }
}
