import 'package:flutter/material.dart';

/// Design tokens for FNF Universal.
///
/// Direction: dark foundation, one bold accent, hard edges (no pill/card
/// rounding), heavy uppercase type. Every screen should pull from here so the
/// app reads as one design language.
abstract final class FnfColors {
  static const background = Color(0xFF0B0B10);
  static const surface = Color(0xFF14141C);
  static const surfaceRaised = Color(0xFF1E1E2A);
  static const line = Color(0xFF2E2E3C);
  static const text = Color(0xFFF2F0E6);
  static const textMuted = Color(0xFF8E8E9E);
  static const accent = Color(0xFFFF6A1F);
  static const onAccent = Color(0xFF160A04);
}

abstract final class FnfSpace {
  static const double xs = 4;
  static const double sm = 8;
  static const double md = 16;
  static const double lg = 24;
  static const double xl = 48;
}

abstract final class FnfMotion {
  static const fast = Duration(milliseconds: 120);
  static const medium = Duration(milliseconds: 220);
  static const curve = Curves.easeOutCubic;
}

/// Typography. No custom font is bundled yet; when an open-licensed pixel or
/// display font is added (and listed in LICENSES.md), set the family here once.
abstract final class FnfType {
  static const TextStyle display = TextStyle(
    fontSize: 52,
    fontWeight: FontWeight.w900,
    letterSpacing: 2,
    height: 0.95,
    color: FnfColors.text,
  );
  static const TextStyle menuItem = TextStyle(
    fontSize: 34,
    fontWeight: FontWeight.w900,
    letterSpacing: 1.5,
    height: 1.0,
  );
  static const TextStyle eyebrow = TextStyle(
    fontSize: 12,
    fontWeight: FontWeight.w800,
    letterSpacing: 3,
    color: FnfColors.textMuted,
  );
  static const TextStyle body = TextStyle(
    fontSize: 16,
    height: 1.45,
    color: FnfColors.text,
  );
  static const TextStyle tag = TextStyle(
    fontSize: 11,
    fontWeight: FontWeight.w800,
    letterSpacing: 1.6,
  );
  static const TextStyle hint = TextStyle(
    fontSize: 11,
    fontWeight: FontWeight.w700,
    letterSpacing: 1.4,
    color: FnfColors.textMuted,
  );
}

ThemeData buildFnfTheme() {
  const scheme = ColorScheme.dark(
    primary: FnfColors.accent,
    onPrimary: FnfColors.onAccent,
    secondary: FnfColors.accent,
    onSecondary: FnfColors.onAccent,
    surface: FnfColors.surface,
    onSurface: FnfColors.text,
    surfaceContainerHighest: FnfColors.surfaceRaised,
    outlineVariant: FnfColors.line,
  );
  return ThemeData(
    useMaterial3: true,
    brightness: Brightness.dark,
    colorScheme: scheme,
    scaffoldBackgroundColor: FnfColors.background,
    appBarTheme: const AppBarTheme(
      backgroundColor: FnfColors.surface,
      foregroundColor: FnfColors.text,
      elevation: 0,
      scrolledUnderElevation: 0,
    ),
  );
}
