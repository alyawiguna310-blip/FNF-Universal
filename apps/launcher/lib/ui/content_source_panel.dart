import 'package:flutter/material.dart';

import '../content/content_controller.dart';
import 'fnf_tokens.dart';

const _boxBorder = OutlineInputBorder(borderRadius: BorderRadius.zero);

/// Lets the player point FNF Universal at their own copy of the game (or a mod
/// folder). FNF Universal does not ship the original game's content.
class ContentSourcePanel extends StatefulWidget {
  const ContentSourcePanel({super.key, required this.content, required this.accent});

  final ContentController content;
  final Color accent;

  @override
  State<ContentSourcePanel> createState() => _ContentSourcePanelState();
}

class _ContentSourcePanelState extends State<ContentSourcePanel> {
  late final TextEditingController _path;
  bool _dirty = false;

  @override
  void initState() {
    super.initState();
    _path = TextEditingController(text: widget.content.path ?? '');
    widget.content.addListener(_sync);
  }

  void _sync() {
    final saved = widget.content.path;
    if (!_dirty && saved != null && _path.text != saved) _path.text = saved;
  }

  @override
  void dispose() {
    widget.content.removeListener(_sync);
    _path.dispose();
    super.dispose();
  }

  void _scan() {
    _dirty = false;
    widget.content.setPath(_path.text);
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: widget.content,
      builder: (context, _) {
        final c = widget.content;
        final catalog = c.catalog;
        final String summary;
        if (c.scanning) {
          summary = 'Scanning\u2026';
        } else if (c.error != null) {
          summary = c.error!;
        } else if (c.path == null) {
          summary = 'No folder chosen yet. Only names, artists, BPM and difficulties are read. Never code or audio.';
        } else if (catalog.isEmpty) {
          summary = 'No levels or songs found in that folder.';
        } else {
          summary = '${catalog.levels.length} level(s), ${catalog.songs.length} song(s) found. Only names, artists, BPM and difficulties are read.';
        }
        return Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Row(
              children: [
                Expanded(
                  child: TextField(
                    key: const ValueKey('content-path'),
                    controller: _path,
                    onChanged: (_) => _dirty = true,
                    onSubmitted: (_) => _scan(),
                    decoration: const InputDecoration(
                      labelText: 'Folder of your own FNF copy or a mod folder',
                      isDense: true,
                      border: _boxBorder,
                    ),
                  ),
                ),
                const SizedBox(width: FnfSpace.sm),
                OutlinedButton(
                  key: const ValueKey('content-scan'),
                  style: OutlinedButton.styleFrom(
                    minimumSize: const Size(48, 48),
                    shape: const RoundedRectangleBorder(),
                  ),
                  onPressed: c.scanning ? null : _scan,
                  child: const Text('SCAN'),
                ),
              ],
            ),
            const SizedBox(height: FnfSpace.xs),
            Text(summary, style: FnfType.hint),
            for (final w in catalog.warnings.take(3))
              Text('\u2022 $w', style: FnfType.hint),
          ],
        );
      },
    );
  }
}
