import 'dart:convert';
import 'dart:io';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

void main() => runApp(const FnfUniversalApp());

class FnfUniversalApp extends StatelessWidget {
  const FnfUniversalApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'FNF Universal',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        brightness: Brightness.dark,
        colorSchemeSeed: const Color(0xFFB66CFF),
        useMaterial3: true,
      ),
      home: const EditorShell(),
    );
  }
}

class EditorShell extends StatefulWidget {
  const EditorShell({super.key});

  @override
  State<EditorShell> createState() => _EditorShellState();
}

class _EditorShellState extends State<EditorShell> {
  int _page = 0;
  final _projectName = TextEditingController(text: 'My First Mod');
  final _songName = TextEditingController(text: 'Tutorial Remix');
  final _bpm = TextEditingController(text: '120');
  final _characterName = TextEditingController(text: 'bf-custom');
  final _characterDescription = TextEditingController(text: 'A custom playable character.');
  final _stageName = TextEditingController(text: 'my-stage');
  String _targetEngine = 'V-Slice';
  final Set<String> _notes = {};
  final List<String> _animations = ['idle', 'singLEFT', 'singDOWN', 'singUP', 'singRIGHT'];
  final List<String> _stageObjects = ['Background', 'Stage floor', 'Opponent', 'Player'];
  final Map<String, bool> _layers = {
    'Background': true,
    'Stage floor': true,
    'Opponent': true,
    'Player': true,
    'Foreground': false,
  };

  @override
  void dispose() {
    _projectName.dispose();
    _songName.dispose();
    _bpm.dispose();
    _characterName.dispose();
    _characterDescription.dispose();
    _stageName.dispose();
    super.dispose();
  }

  String _safeFileName(String value) {
    final cleaned = value.trim().replaceAll(RegExp(r'[^a-zA-Z0-9_-]+'), '_');
    return cleaned.isEmpty ? 'untitled_mod' : cleaned;
  }

  Future<Directory> _projectDirectory() async {
    final appDir = await getApplicationDocumentsDirectory();
    final dir = Directory('${appDir.path}${Platform.pathSeparator}FNFUniversal${Platform.pathSeparator}Projects');
    await dir.create(recursive: true);
    return dir;
  }

  Future<void> _saveProject() async {
    try {
      final dir = await _projectDirectory();
      final file = File('${dir.path}${Platform.pathSeparator}${_safeFileName(_projectName.text)}.fnfup.json');
      const encoder = JsonEncoder.withIndent('  ');
      await file.writeAsString(encoder.convert(_projectData()), flush: true);
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Project saved: ${file.path}')),
      );
    } catch (error) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Could not save project: $error')),
      );
    }
  }

  Future<void> _loadProject() async {
    try {
      final dir = await _projectDirectory();
      final files = dir
          .listSync()
          .whereType<File>()
          .where((file) => file.path.endsWith('.fnfup.json'))
          .toList()
        ..sort((a, b) => a.path.toLowerCase().compareTo(b.path.toLowerCase()));

      if (!mounted) return;
      if (files.isEmpty) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(content: Text('No saved FNF Universal projects found yet.')),
        );
        return;
      }

      final selected = await showDialog<File>(
        context: context,
        builder: (dialogContext) => AlertDialog(
          title: const Text('Load project'),
          content: SizedBox(
            width: 520,
            child: ListView(
              shrinkWrap: true,
              children: files.map((file) {
                final name = file.uri.pathSegments.last;
                return ListTile(
                  leading: const Icon(Icons.description_outlined),
                  title: Text(name),
                  onTap: () => Navigator.pop(dialogContext, file),
                );
              }).toList(),
            ),
          ),
          actions: [
            TextButton(onPressed: () => Navigator.pop(dialogContext), child: const Text('Cancel')),
          ],
        ),
      );
      if (selected == null || !mounted) return;

      final decoded = jsonDecode(await selected.readAsString());
      if (decoded is! Map<String, dynamic> ||
          decoded['format'] != 'fnf-universal-project' ||
          decoded['formatVersion'] != 1) {
        throw const FormatException('Unsupported or invalid FNF Universal project file.');
      }

      final project = decoded['project'] as Map<String, dynamic>? ?? {};
      final chart = decoded['chart'] as Map<String, dynamic>? ?? {};
      final character = decoded['character'] as Map<String, dynamic>? ?? {};
      final stage = decoded['stage'] as Map<String, dynamic>? ?? {};
      final validEngines = {
        'V-Slice',
        'Psych Engine',
        'Kade Engine',
        'Codename Engine',
        'Engine-independent',
      };
      final loadedEngine = project['targetEngine']?.toString() ?? 'Engine-independent';
      final loadedNotes = <String>{};
      final rawNotes = chart['notes'];
      if (rawNotes is List) {
        for (final note in rawNotes) {
          if (note is Map) {
            final lane = note['lane'];
            final step = note['step'];
            if (lane is int && step is int && lane >= 0 && lane < 4 && step >= 0 && step < 16) {
              loadedNotes.add('$lane:$step');
            }
          }
        }
      }
      final loadedAnimations = (character['animations'] as List?)
              ?.map((value) => value.toString())
              .toList() ??
          <String>[];
      final loadedObjects = (stage['objects'] as List?)
              ?.map((value) => value.toString())
              .toList() ??
          <String>[];
      final rawLayers = stage['layers'];
      final loadedLayers = <String, bool>{};
      if (rawLayers is Map) {
        for (final entry in rawLayers.entries) {
          if (entry.key is String && entry.value is bool) {
            loadedLayers[entry.key as String] = entry.value as bool;
          }
        }
      }

      setState(() {
        _projectName.text = project['name']?.toString() ?? 'Untitled Mod';
        _targetEngine = validEngines.contains(loadedEngine) ? loadedEngine : 'Engine-independent';
        _songName.text = chart['song']?.toString() ?? 'Untitled Song';
        _bpm.text = (chart['bpm'] ?? 120).toString();
        _characterName.text = character['name']?.toString() ?? 'character';
        _characterDescription.text = character['description']?.toString() ?? '';
        _stageName.text = stage['name']?.toString() ?? 'stage';
        _notes
          ..clear()
          ..addAll(loadedNotes);
        _animations
          ..clear()
          ..addAll(loadedAnimations);
        _stageObjects
          ..clear()
          ..addAll(loadedObjects);
        _layers
          ..clear()
          ..addAll(loadedLayers);
        _page = 0;
      });

      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Loaded ${selected.uri.pathSegments.last}')),
      );
    } catch (error) {
      if (!mounted) return;
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(content: Text('Could not load project: $error')),
      );
    }
  }

  Map<String, dynamic> _projectData() => {
        'format': 'fnf-universal-project',
        'formatVersion': 1,
        'project': {
          'name': _projectName.text,
          'targetEngine': _targetEngine,
        },
        'chart': {
          'song': _songName.text,
          'bpm': int.tryParse(_bpm.text) ?? 120,
          'lanes': 4,
          'notes': _notes.map((key) {
            final parts = key.split(':');
            return {'lane': int.parse(parts[0]), 'step': int.parse(parts[1])};
          }).toList(),
        },
        'character': {
          'name': _characterName.text,
          'description': _characterDescription.text,
          'animations': _animations,
          'sprite': null,
        },
        'stage': {
          'name': _stageName.text,
          'objects': _stageObjects,
          'layers': _layers,
        },
        'assets': <String>[],
        'compatibility': {
          'status': 'editor-draft',
          'note': 'Engine-specific export and runtime validation are not implemented yet.',
        },
      };

  Future<void> _showExport() async {
    final pretty = const JsonEncoder.withIndent('  ').convert(_projectData());
    await showDialog<void>(
      context: context,
      builder: (context) => AlertDialog(
        title: const Text('Project data preview'),
        content: SizedBox(
          width: 620,
          child: SingleChildScrollView(
            child: SelectableText(pretty, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
          ),
        ),
        actions: [
          TextButton(
            onPressed: () async {
              await Clipboard.setData(ClipboardData(text: pretty));
              if (context.mounted) {
                Navigator.pop(context);
                ScaffoldMessenger.of(this.context).showSnackBar(
                  const SnackBar(content: Text('Project JSON copied to clipboard')),
                );
              }
            },
            child: const Text('Copy JSON'),
          ),
          FilledButton(onPressed: () => Navigator.pop(context), child: const Text('Close')),
        ],
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    final pages = [
      _WorkshopPage(
        projectName: _projectName,
        targetEngine: _targetEngine,
        onEngineChanged: (value) => setState(() => _targetEngine = value),
        onOpenEditor: (index) => setState(() => _page = index),
        onExport: _showExport,
      ),
      _ChartEditor(
        songName: _songName,
        bpm: _bpm,
        notes: _notes,
        onChanged: () => setState(() {}),
      ),
      _CharacterEditor(
        name: _characterName,
        description: _characterDescription,
        animations: _animations,
        onChanged: () => setState(() {}),
      ),
      _StageEditor(
        name: _stageName,
        objects: _stageObjects,
        layers: _layers,
        onChanged: () => setState(() {}),
      ),
    ];

    return Scaffold(
      appBar: AppBar(
        title: const Text('FNF Universal Studio'),
        actions: [
          IconButton(
            tooltip: 'Load saved project',
            onPressed: _loadProject,
            icon: const Icon(Icons.folder_open),
          ),
          IconButton(
            tooltip: 'Save project',
            onPressed: _saveProject,
            icon: const Icon(Icons.save_outlined),
          ),
          IconButton(
            tooltip: 'Preview project JSON',
            onPressed: _showExport,
            icon: const Icon(Icons.data_object),
          ),
          const SizedBox(width: 8),
        ],
      ),
      body: IndexedStack(index: _page, children: pages),
      bottomNavigationBar: NavigationBar(
        selectedIndex: _page,
        onDestinationSelected: (index) => setState(() => _page = index),
        destinations: const [
          NavigationDestination(icon: Icon(Icons.folder_open), label: 'Workshop'),
          NavigationDestination(icon: Icon(Icons.piano), label: 'Chart'),
          NavigationDestination(icon: Icon(Icons.person), label: 'Character'),
          NavigationDestination(icon: Icon(Icons.landscape), label: 'Stage'),
        ],
      ),
    );
  }
}

class _WorkshopPage extends StatelessWidget {
  const _WorkshopPage({
    required this.projectName,
    required this.targetEngine,
    required this.onEngineChanged,
    required this.onOpenEditor,
    required this.onExport,
  });

  final TextEditingController projectName;
  final String targetEngine;
  final ValueChanged<String> onEngineChanged;
  final ValueChanged<int> onOpenEditor;
  final VoidCallback onExport;

  @override
  Widget build(BuildContext context) => ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _SectionCard(
            title: 'Mod project',
            subtitle: 'A single workspace for your song, characters, stages and assets.',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                TextField(
                  controller: projectName,
                  decoration: const InputDecoration(labelText: 'Project name', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 12),
                DropdownButtonFormField<String>(
                  value: targetEngine,
                  decoration: const InputDecoration(labelText: 'Intended engine', border: OutlineInputBorder()),
                  items: const ['V-Slice', 'Psych Engine', 'Kade Engine', 'Codename Engine', 'Engine-independent']
                      .map((name) => DropdownMenuItem(value: name, child: Text(name)))
                      .toList(),
                  onChanged: (value) {
                    if (value != null) onEngineChanged(value);
                  },
                ),
                const SizedBox(height: 12),
                const Row(
                  children: [
                    Icon(Icons.info_outline, size: 18),
                    SizedBox(width: 8),
                    Expanded(child: Text('Target selection is project metadata, not proof of engine compatibility.')),
                  ],
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          Text('EDITOR WORKSPACES', style: Theme.of(context).textTheme.labelLarge),
          const SizedBox(height: 8),
          _WorkspaceTile(icon: Icons.piano, title: 'Chart editor', subtitle: 'Place notes on a 4-lane step grid', onTap: () => onOpenEditor(1)),
          _WorkspaceTile(icon: Icons.person, title: 'Character editor', subtitle: 'Define character identity and animations', onTap: () => onOpenEditor(2)),
          _WorkspaceTile(icon: Icons.landscape, title: 'Stage editor', subtitle: 'Organize stage objects and visibility layers', onTap: () => onOpenEditor(3)),
          const SizedBox(height: 16),
          _SectionCard(
            title: 'Project storage',
            subtitle: 'Save and reopen your editor project on this device.',
            child: const Row(
              children: [
                Icon(Icons.save_alt),
                SizedBox(width: 10),
                Expanded(
                  child: Text('Use the save and folder buttons in the top bar. Projects are stored in the app documents directory as .fnfup.json files.'),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          _SectionCard(
            title: 'Assets & export',
            subtitle: 'Asset importing and engine-specific packaging are planned next.',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                const ListTile(
                  contentPadding: EdgeInsets.zero,
                  leading: Icon(Icons.image_outlined),
                  title: Text('Sprites and backgrounds'),
                  subtitle: Text('Import slot not connected yet'),
                ),
                const ListTile(
                  contentPadding: EdgeInsets.zero,
                  leading: Icon(Icons.audio_file_outlined),
                  title: Text('Inst and vocals audio'),
                  subtitle: Text('Audio import and waveform preview not connected yet'),
                ),
                const ListTile(
                  contentPadding: EdgeInsets.zero,
                  leading: Icon(Icons.code),
                  title: Text('Scripts and metadata'),
                  subtitle: Text('Metadata draft is stored in this editor session'),
                ),
                FilledButton.icon(onPressed: onExport, icon: const Icon(Icons.copy), label: const Text('Preview / copy project JSON')),
              ],
            ),
          ),
          const SizedBox(height: 12),
          const Text('Prototype note: project save/load is now available locally. Asset import, undo/redo and real engine export are still pending.'),
        ],
      );
}

class _ChartEditor extends StatelessWidget {
  const _ChartEditor({
    required this.songName,
    required this.bpm,
    required this.notes,
    required this.onChanged,
  });

  final TextEditingController songName;
  final TextEditingController bpm;
  final Set<String> notes;
  final VoidCallback onChanged;

  @override
  Widget build(BuildContext context) {
    const lanes = ['LEFT', 'DOWN', 'UP', 'RIGHT'];
    return ListView(
      padding: const EdgeInsets.all(16),
      children: [
        _SectionCard(
          title: 'Chart setup',
          subtitle: 'A simple four-lane note grid. Tap cells to add or remove notes.',
          child: Column(
            children: [
              TextField(controller: songName, decoration: const InputDecoration(labelText: 'Song name', border: OutlineInputBorder())),
              const SizedBox(height: 12),
              TextField(
                controller: bpm,
                keyboardType: TextInputType.number,
                decoration: const InputDecoration(labelText: 'BPM', helperText: 'Tempo metadata only in this prototype', border: OutlineInputBorder()),
              ),
            ],
          ),
        ),
        const SizedBox(height: 16),
        Text('16-STEP PATTERN', style: Theme.of(context).textTheme.labelLarge),
        const SizedBox(height: 8),
        Card(
          child: Padding(
            padding: const EdgeInsets.all(8),
            child: Column(
              children: [
                Row(
                  children: [
                    const SizedBox(width: 62, child: Text('LANE')),
                    for (var step = 0; step < 16; step++)
                      Expanded(child: Center(child: Text('${step + 1}', style: Theme.of(context).textTheme.labelSmall))),
                  ],
                ),
                const SizedBox(height: 8),
                for (var lane = 0; lane < 4; lane++)
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 4),
                    child: Row(
                      children: [
                        SizedBox(width: 62, child: Text(lanes[lane], style: Theme.of(context).textTheme.labelSmall)),
                        for (var step = 0; step < 16; step++)
                          Expanded(
                            child: Padding(
                              padding: const EdgeInsets.all(1),
                              child: AspectRatio(
                                aspectRatio: 0.78,
                                child: _NoteCell(
                                  active: notes.contains('$lane:$step'),
                                  onTap: () {
                                    final key = '$lane:$step';
                                    if (!notes.add(key)) notes.remove(key);
                                    onChanged();
                                  },
                                ),
                              ),
                            ),
                          ),
                      ],
                    ),
                  ),
              ],
            ),
          ),
        ),
        const SizedBox(height: 8),
        Text('${notes.length} notes placed · one pattern / 16 steps', style: Theme.of(context).textTheme.bodySmall),
        const SizedBox(height: 12),
        const _Notice(text: 'Next chart milestones: hold notes, multiple sections, BPM changes, events, difficulty tracks, audio playback and chart-format adapters.'),
      ],
    );
  }
}

class _NoteCell extends StatelessWidget {
  const _NoteCell({required this.active, required this.onTap});
  final bool active;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => Material(
        color: active ? Theme.of(context).colorScheme.primary : Theme.of(context).colorScheme.surfaceContainerHighest,
        borderRadius: BorderRadius.circular(5),
        child: InkWell(
          borderRadius: BorderRadius.circular(5),
          onTap: onTap,
          child: Center(
            child: active ? const Icon(Icons.arrow_upward, size: 13) : const SizedBox.shrink(),
          ),
        ),
      );
}

class _CharacterEditor extends StatelessWidget {
  const _CharacterEditor({
    required this.name,
    required this.description,
    required this.animations,
    required this.onChanged,
  });

  final TextEditingController name;
  final TextEditingController description;
  final List<String> animations;
  final VoidCallback onChanged;

  @override
  Widget build(BuildContext context) => ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _SectionCard(
            title: 'Character definition',
            subtitle: 'Create the metadata before connecting a spritesheet.',
            child: Column(
              children: [
                TextField(controller: name, onChanged: (_) => onChanged(), decoration: const InputDecoration(labelText: 'Character ID', border: OutlineInputBorder())),
                const SizedBox(height: 12),
                TextField(controller: description, onChanged: (_) => onChanged(), maxLines: 3, decoration: const InputDecoration(labelText: 'Description', border: OutlineInputBorder())),
              ],
            ),
          ),
          const SizedBox(height: 16),
          _SectionCard(
            title: 'Sprite & animations',
            subtitle: 'Animation names are editable metadata; no sprite playback yet.',
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Container(
                  height: 150,
                  width: double.infinity,
                  decoration: BoxDecoration(
                    color: Theme.of(context).colorScheme.surfaceContainerHighest,
                    borderRadius: BorderRadius.circular(12),
                    border: Border.all(color: Theme.of(context).colorScheme.outlineVariant),
                  ),
                  child: const Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Icon(Icons.add_photo_alternate_outlined, size: 36),
                      SizedBox(height: 8),
                      Text('Sprite preview placeholder'),
                      Text('PNG / spritesheet import not connected', style: TextStyle(fontSize: 12)),
                    ],
                  ),
                ),
                const SizedBox(height: 12),
                for (final animation in animations)
                  ListTile(
                    contentPadding: EdgeInsets.zero,
                    leading: const Icon(Icons.animation),
                    title: Text(animation),
                    subtitle: const Text('Prefix / frame mapping pending'),
                    trailing: IconButton(
                      tooltip: 'Remove animation',
                      icon: const Icon(Icons.delete_outline),
                      onPressed: () {
                        animations.remove(animation);
                        onChanged();
                      },
                    ),
                  ),
                OutlinedButton.icon(
                  onPressed: () {
                    final next = 'custom_${animations.length + 1}';
                    animations.add(next);
                    onChanged();
                  },
                  icon: const Icon(Icons.add),
                  label: const Text('Add animation'),
                ),
              ],
            ),
          ),
          const SizedBox(height: 12),
          const _Notice(text: 'Next character milestones: spritesheet import, animation prefix/frame editor, offsets, icon generation, character preview and engine-specific character definitions.'),
        ],
      );
}

class _StageEditor extends StatelessWidget {
  const _StageEditor({
    required this.name,
    required this.objects,
    required this.layers,
    required this.onChanged,
  });

  final TextEditingController name;
  final List<String> objects;
  final Map<String, bool> layers;
  final VoidCallback onChanged;

  @override
  Widget build(BuildContext context) => ListView(
        padding: const EdgeInsets.all(16),
        children: [
          _SectionCard(
            title: 'Stage setup',
            subtitle: 'Name the stage and manage its scene objects.',
            child: TextField(controller: name, onChanged: (_) => onChanged(), decoration: const InputDecoration(labelText: 'Stage ID', border: OutlineInputBorder())),
          ),
          const SizedBox(height: 16),
          _SectionCard(
            title: 'Scene layers',
            subtitle: 'Toggle visibility and add placeholder objects.',
            child: Column(
              children: [
                for (final entry in layers.entries)
                  SwitchListTile(
                    contentPadding: EdgeInsets.zero,
                    title: Text(entry.key),
                    value: entry.value,
                    onChanged: (value) {
                      layers[entry.key] = value;
                      onChanged();
                    },
                  ),
                OutlinedButton.icon(
                  onPressed: () {
                    final label = 'Object ${objects.length + 1}';
                    objects.add(label);
                    layers[label] = true;
                    onChanged();
                  },
                  icon: const Icon(Icons.add),
                  label: const Text('Add stage object'),
                ),
              ],
            ),
          ),
          const SizedBox(height: 16),
          _SectionCard(
            title: 'Object list',
            subtitle: 'Scene ordering is currently a simple list.',
            child: Column(
              children: [
                for (var i = 0; i < objects.length; i++)
                  ListTile(
                    contentPadding: EdgeInsets.zero,
                    leading: const Icon(Icons.layers_outlined),
                    title: Text(objects[i]),
                    subtitle: Text('Layer ${i + 1}'),
                    trailing: IconButton(
                      tooltip: 'Remove object',
                      icon: const Icon(Icons.close),
                      onPressed: () {
                        layers.remove(objects[i]);
                        objects.removeAt(i);
                        onChanged();
                      },
                    ),
                  ),
              ],
            ),
          ),
          const _Notice(text: 'Next stage milestones: drag-and-drop canvas, position/scale/scroll-factor controls, camera preview, parallax and background image import.'),
        ],
      );
}

class _SectionCard extends StatelessWidget {
  const _SectionCard({required this.title, required this.subtitle, required this.child});
  final String title;
  final String subtitle;
  final Widget child;

  @override
  Widget build(BuildContext context) => Card(
        child: Padding(
          padding: const EdgeInsets.all(16),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(title, style: Theme.of(context).textTheme.titleLarge),
              const SizedBox(height: 4),
              Text(subtitle, style: Theme.of(context).textTheme.bodySmall),
              const SizedBox(height: 16),
              child,
            ],
          ),
        ),
      );
}

class _WorkspaceTile extends StatelessWidget {
  const _WorkspaceTile({required this.icon, required this.title, required this.subtitle, required this.onTap});
  final IconData icon;
  final String title;
  final String subtitle;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) => Card(
        child: ListTile(
          leading: CircleAvatar(child: Icon(icon)),
          title: Text(title),
          subtitle: Text(subtitle),
          trailing: const Icon(Icons.chevron_right),
          onTap: onTap,
        ),
      );
}

class _Notice extends StatelessWidget {
  const _Notice({required this.text});
  final String text;

  @override
  Widget build(BuildContext context) => Container(
        padding: const EdgeInsets.all(12),
        decoration: BoxDecoration(
          color: Theme.of(context).colorScheme.surfaceContainerHighest,
          borderRadius: BorderRadius.circular(10),
        ),
        child: Row(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            const Icon(Icons.lightbulb_outline, size: 18),
            const SizedBox(width: 8),
            Expanded(child: Text(text, style: Theme.of(context).textTheme.bodySmall)),
          ],
        ),
      );
}
