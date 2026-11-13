import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/ui/background_image.dart';
import 'package:fnf_universal/ui/menu_definition.dart';
import 'package:fnf_universal/ui/menu_entry.dart';

void main() {
  test('defaults round trip through JSON without warnings', () {
    final d = LauncherUiDefinition.defaults();
    final parsed = LauncherUiDefinition.parse(jsonDecode(jsonEncode(d.toJson())));
    expect(parsed.warnings, isEmpty);
    expect(parsed.definition.toJson(), d.toJson());
  });

  test('Settings can never be hidden or removed', () {
    final hidden = LauncherUiDefinition.parse({
      'format': 1,
      'entries': [
        {'id': 'story'},
        {'id': 'settings', 'visible': false},
      ],
    });
    final settings = hidden.definition.entries.firstWhere((e) => e.id == 'settings');
    expect(settings.visible, isTrue);
    expect(hidden.warnings, isNotEmpty);

    final removed = LauncherUiDefinition.parse({
      'entries': [
        {'id': 'story'},
      ],
    });
    expect(removed.definition.entries.any((e) => e.id == 'settings'), isTrue);
  });

  test('unknown destinations and bad or unreadable colors are rejected', () {
    final r = LauncherUiDefinition.parse({
      'accent': '#101010',
      'entries': [
        {'id': 'rm -rf'},
        {'id': 'story', 'label': 'Play\u0000Now'},
      ],
    });
    expect(r.definition.accent.toARGB32(), LauncherUiDefinition.defaults().accent.toARGB32());
    expect(r.definition.entries.any((e) => e.id == 'rm -rf'), isFalse);
    expect(r.definition.entries.first.label, 'Play Now');
    expect(r.warnings.length, greaterThanOrEqualTo(2));

    final bad = LauncherUiDefinition.parse({'accent': '#GGGGGG'});
    expect(bad.warnings, isNotEmpty);
  });

  test('resolve hides hidden entries, keeps protected ones, adds Developer', () {
    final d = LauncherUiDefinition.defaults().copyWith(entries: [
      for (final e in LauncherUiDefinition.defaults().entries)
        e.id == 'story' || e.id == 'settings' ? e.copyWith(visible: false) : e,
    ]);
    final ids = resolveMenuEntries(d, developerUnlocked: false).map((e) => e.id).toList();
    expect(ids.contains('story'), isFalse);
    expect(ids.contains('settings'), isTrue);
    expect(ids.contains('developer'), isFalse);
    final withDev = resolveMenuEntries(d, developerUnlocked: true);
    expect(withDev.last.id, 'developer');
  });

  test('restyling text never changes status or requirements', () {
    final d = LauncherUiDefinition.defaults().copyWith(entries: [
      for (final e in LauncherUiDefinition.defaults().entries)
        e.id == 'story' ? e.copyWith(label: 'READY GAME', summary: 'Totally works') : e,
    ]);
    final story = resolveMenuEntries(d, developerUnlocked: false).firstWhere((e) => e.id == 'story');
    expect(story.label, 'READY GAME');
    expect(story.status, MenuStatus.planned);
    expect(story.requires, isNotNull);
  });

  test('wallpaper names must be plain image file names', () {
    for (final bad in ['../x.png', 'a/b.png', r'a\b.png', 'run.exe', 'noext', 'CON.png', '.hidden.png', '']) {
      expect(cleanBackgroundName(bad), isNull, reason: bad);
    }
    expect(cleanBackgroundName('wall paper-1.PNG'), 'wall paper-1.PNG');
    expect(sanitizeBackgroundName('my pic (1).jpg'), 'my pic _1_.jpg');
  });

  test('image style without a valid wallpaper falls back to stripes', () {
    final r = LauncherUiDefinition.parse({
      'background': 'image',
      'backgroundImage': '../x.png',
      'backgroundDim': 5,
    });
    expect(r.definition.background, MenuBackgroundStyle.stripes);
    expect(r.definition.backgroundImage, isNull);
    expect(r.definition.backgroundDim, kMaxBackgroundDim);
    expect(r.warnings.length, greaterThanOrEqualTo(2));
  });

  test('a valid wallpaper survives a round trip', () {
    final d = LauncherUiDefinition.defaults().copyWith(
      background: MenuBackgroundStyle.image,
      backgroundImage: 'wall.png',
      backgroundDim: 0.3,
    );
    final r = LauncherUiDefinition.parse(jsonDecode(jsonEncode(d.toJson())));
    expect(r.warnings, isEmpty);
    expect(r.definition.background, MenuBackgroundStyle.image);
    expect(r.definition.backgroundImage, 'wall.png');
    expect(r.definition.backgroundDim, 0.3);
  });

  test('looksLikeImage checks the header, not the name', () {
    final png = Uint8List.fromList([0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0, 0, 0, 0, 1]);
    final text = Uint8List.fromList(utf8.encode('this is not an image at all'));
    expect(looksLikeImage(png), isTrue);
    expect(looksLikeImage(text), isFalse);
  });
}
