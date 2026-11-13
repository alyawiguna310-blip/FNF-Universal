import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/ui/fnf_app.dart';

void _useWideScreen(WidgetTester tester) {
  tester.view.physicalSize = const Size(1280, 800);
  tester.view.devicePixelRatio = 1;
  addTearDown(tester.view.reset);
}

void main() {
  testWidgets('main menu shows every required entry', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(const FnfMenuApp());
    await tester.pump();

    for (final id in ['story', 'freeplay', 'mods', 'settings', 'credits']) {
      expect(find.byKey(ValueKey('menu-$id')), findsOneWidget);
    }
    expect(find.byKey(const ValueKey('menu-developer')), findsNothing);
  });

  testWidgets('keyboard: arrows move, Enter opens, Escape returns', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(const FnfMenuApp());
    await tester.pump();

    await tester.sendKeyEvent(LogicalKeyboardKey.arrowDown);
    await tester.pump();
    await tester.sendKeyEvent(LogicalKeyboardKey.enter);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('feature-status-freeplay')), findsOneWidget);

    await tester.sendKeyEvent(LogicalKeyboardKey.escape);
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('feature-status-freeplay')), findsNothing);
    expect(find.byKey(const ValueKey('menu-story')), findsOneWidget);
  });

  testWidgets('touch: tapping an entry opens it and back returns', (tester) async {
    _useWideScreen(tester);
    await tester.pumpWidget(const FnfMenuApp());
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
    await tester.pumpWidget(const FnfMenuApp());
    await tester.pump();

    for (var i = 0; i < 7; i++) {
      await tester.tap(find.byKey(const ValueKey('version-tag')));
      await tester.pump();
    }
    await tester.pump();
    expect(find.byKey(const ValueKey('menu-developer')), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('menu-developer')));
    await tester.pumpAndSettle();
    expect(find.text('FNF Universal Studio'), findsOneWidget);

    await tester.tap(find.byKey(const ValueKey('studio-exit')));
    await tester.pumpAndSettle();
    expect(find.byKey(const ValueKey('menu-story')), findsOneWidget);
  });
}
