import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/content/content_catalog.dart';
import 'package:fnf_universal/content/content_controller.dart';
import 'package:fnf_universal/ui/fnf_app.dart';
import 'package:fnf_universal/ui/launcher_ui_controller.dart';

void _useWideScreen(WidgetTester tester) {
  tester.view.physicalSize = const Size(1280, 800);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

const _sample = ContentCatalog(
  levels: [
    LevelInfo(id: 'week-1', name: 'Week One', songIds: ['alpha']),
  ],
  songs: {
    'alpha': SongInfo(
      id: 'alpha',
      title: 'Alpha Song',
      artist: 'Someone',
      difficulties: ['easy', 'hard'],
      bpm: 120,
    ),
  },
);

// Motion is switched off in most tests: a looping backdrop never "settles",
// which would make pumpAndSettle hang. One test below covers the animated path.
Widget _still({LauncherUiController? ui, ContentController? content}) => FnfMenuApp(
      animated: false,
      uiController: ui ?? LauncherUiController(),
      contentController: content ?? ContentController(),
    );

Future<void> _unlockDeveloper(WidgetTester tester) async {
  for (var i = 0; i < 7; i++) {
    await tester.tap(find.byKey(const ValueKey('version-tag')));
    await tester.pump();
  }
  await tester.pump();
}

void main() {
  testWidgets('main menu shows every required entry', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still());
    await tester.pump();

    for (final id in ['story', 'freeplay', 'mods', 'settings', 'credits']) {
      expect(find.byKey(ValueKey('menu-$id')), findsOneWidget);
    }
    expect(find.byKey(const ValueKey('menu-developer')), findsNothing);
  });

  testWidgets('keyboard: Freeplay lists songs from the content catalog, Escape returns', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still(content: ContentController(initial: _sample)));
    await tester.pump();

    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('browse-freeplay')), findsOneWidget);
    expect(find.text('ALPHA SONG'), findsWidgets);

    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('browse-freeplay')), findsNothing);
    expect(find.byKey(const ValueKey('menu-story')), findsOneWidget);
  });

  testWidgets('Story lists the visible levels and says so when there is nothing yet', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still(content: ContentController(initial: _sample)));
    await tester.pump();

    await tester.tap(find.byKey(const ValueKey('menu-story')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('browse-story')), findsOneWidget);
    expect(find.text('WEEK ONE'), findsWidgets);
    await tester.tap(find.byKey(const ValueKey('browse-back')));
    await tester.pumpAndSettle();

    // Same screen with an empty catalog: honest empty state, no fake weeks.
    await tester.pumpWidget(_still());
    await tester.pump();
    await tester.tap(find.byKey(const ValueKey('menu-story')));
    await tester.pumpAndSettle();
    expect(find.textContaining('No story levels yet'), findsOneWidget);
  });

  testWidgets('Credits credits The Funkin Crew and says no game content is bundled', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still());
    await tester.pump();

    await tester.tap(find.byKey(const ValueKey('menu-credits')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('credits-screen')), findsOneWidget);
    expect(find.textContaining('The Funkin\u2019 Crew Inc.'), findsWidgets);
    expect(find.textContaining('does not bundle'), findsOneWidget);

    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('credits-screen')), findsNothing);
  });

  testWidgets('moving the selection back and forth quickly does not crash', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still());
    await tester.pump();

    for (final key in [
      LogicalKeyboardKey.arrowDown,
      LogicalKeyboardKey.arrowUp,
      LogicalKeyboardKey.arrowDown,
      LogicalKeyboardKey.arrowUp,
    ]) {
      await tester.sendKeyEvent(key);
      await tester.pump(const Duration(milliseconds: 40));
    }
    await tester.pumpAndSettle();
    expect(tester.takeException(), isNull);
  });

  testWidgets('touch: tapping an entry opens it and back returns', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still());
    await tester.pump();

    await tester.tap(find.byKey(const ValueKey('menu-mods')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('feature-status-mods')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('status-back')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('feature-status-mods')), findsNothing);
  });

  testWidgets('developer mode unlocks the existing studio and returns', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still());
    await tester.pump();

    await _unlockDeveloper(tester);
    expect(find.byKey(const ValueKey('menu-developer')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('menu-developer')));
    await tester.pumpAndSettle();
    expect(find.text('FNF Universal Studio'), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('studio-exit')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('menu-story')), findsOneWidget);
  });

  testWidgets('animated menu: confirm flash, then the entry opens', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(FnfMenuApp(
      uiController: LauncherUiController(),
      contentController: ContentController(initial: _sample),
    ));
    await tester.pump(const Duration(milliseconds: 300));

    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pump(const Duration(milliseconds: 200));
    await tester.pump(const Duration(milliseconds: 500));
    expect(find.byKey(const ValueKey('browse-story')), findsOneWidget);
  });

  testWidgets('menu follows the launcher UI definition', (tester) async {
    _useWideScreen(tester);
    final ui = LauncherUiController();
    await tester.pumpWidget(_still(ui: ui));
    await tester.pump();

    ui.update(ui.definition.copyWith(
      wordmarkMain: 'My Mod',
      entries: [
        for (final e in ui.definition.entries)
          e.id == 'story' ? e.copyWith(visible: false) : e,
      ],
    ));
    await tester.pump();

    expect(find.byKey(const ValueKey('menu-story')), findsNothing);
    expect(find.byKey(const ValueKey('menu-freeplay')), findsOneWidget);
    expect(find.text('MY MOD'), findsOneWidget);
  });

  testWidgets('developer tools expose the Menu/UI editor with a live preview', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(_still());
    await tester.pump();

    await _unlockDeveloper(tester);
    await tester.tap(find.byKey(const ValueKey('menu-developer')));
    await tester.pumpAndSettle();

    await tester.tap(find.byKey(const ValueKey('dev-tab-menuUi')));
    await tester.pumpAndSettle();
    expect(find.text('LIVE PREVIEW'), findsOneWidget);
    expect(find.text('MENU / UI EDITOR'), findsOneWidget);
  });
}
