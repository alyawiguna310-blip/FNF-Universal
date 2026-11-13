import 'dart:ui' show Color;

import 'fnf_tokens.dart';
import 'menu_entry.dart';

/// Data-driven look of the launcher menu. This is what the Menu/UI editor edits
/// and what a mod package carries. It is plain data: no code, no commands.
/// Entries can only point at built-in destinations.
const int kLauncherUiFormat = 1;
const Set<String> kProtectedEntryIds = {'settings'};
const int kMaxWordmarkLength = 16;
const int kMaxLabelLength = 24;
const int kMaxSummaryLength = 120;

enum MenuBackgroundStyle { stripes, plain }

String colorToHex(Color c) =>
    '#${(c.toARGB32() & 0xFFFFFF).toRadixString(16).padLeft(6, '0').toUpperCase()}';

Color? parseHexColor(Object? raw) {
  if (raw is! String) return null;
  final match = RegExp(r'^#?([0-9a-fA-F]{6})$').firstMatch(raw.trim());
  if (match == null) return null;
  return Color(0xFF000000 | int.parse(match.group(1)!, radix: 16));
}

/// The menu is dark, so an accent that is too dark would vanish.
bool accentIsReadable(Color c) => c.computeLuminance() >= 0.15;

class MenuEntryDef {
  const MenuEntryDef({
    required this.id,
    required this.label,
    required this.summary,
    this.visible = true,
  });

  final String id;
  final String label;
  final String summary;
  final bool visible;

  MenuEntryDef copyWith({String? label, String? summary, bool? visible}) =>
      MenuEntryDef(
        id: id,
        label: label ?? this.label,
        summary: summary ?? this.summary,
        visible: visible ?? this.visible,
      );

  Map<String, dynamic> toJson() => {
        'id': id,
        'label': label,
        'summary': summary,
        'visible': visible,
      };
}

class LauncherUiParseResult {
  const LauncherUiParseResult(this.definition, this.warnings);

  final LauncherUiDefinition definition;
  final List<String> warnings;
}

class LauncherUiDefinition {
  const LauncherUiDefinition({
    required this.wordmarkTop,
    required this.wordmarkMain,
    required this.accent,
    required this.background,
    required this.beatBop,
    required this.entries,
  });

  factory LauncherUiDefinition.defaults() => LauncherUiDefinition(
        wordmarkTop: 'FNF',
        wordmarkMain: 'Universal',
        accent: FnfColors.accent,
        background: MenuBackgroundStyle.stripes,
        beatBop: true,
        entries: [
          for (final e in kMainMenuEntries)
            MenuEntryDef(id: e.id, label: e.label, summary: e.summary),
        ],
      );

  final String wordmarkTop;
  final String wordmarkMain;
  final Color accent;
  final MenuBackgroundStyle background;
  final bool beatBop;
  final List<MenuEntryDef> entries;

  LauncherUiDefinition copyWith({
    String? wordmarkTop,
    String? wordmarkMain,
    Color? accent,
    MenuBackgroundStyle? background,
    bool? beatBop,
    List<MenuEntryDef>? entries,
  }) =>
      LauncherUiDefinition(
        wordmarkTop: wordmarkTop ?? this.wordmarkTop,
        wordmarkMain: wordmarkMain ?? this.wordmarkMain,
        accent: accent ?? this.accent,
        background: background ?? this.background,
        beatBop: beatBop ?? this.beatBop,
        entries: entries ?? this.entries,
      );

  Map<String, dynamic> toJson() => {
        'format': kLauncherUiFormat,
        'wordmarkTop': wordmarkTop,
        'wordmarkMain': wordmarkMain,
        'accent': colorToHex(accent),
        'background': background.name,
        'beatBop': beatBop,
        'entries': entries.map((e) => e.toJson()).toList(),
      };

  /// Tolerant, validating parser. Anything unsafe or unknown is dropped or
  /// replaced with a default, and every adjustment is reported in `warnings`.
  static LauncherUiParseResult parse(Object? raw) {
    final defaults = LauncherUiDefinition.defaults();
    if (raw is! Map) {
      return LauncherUiParseResult(
        defaults,
        ['launcher_ui is not a JSON object. Defaults were used.'],
      );
    }
    final warnings = <String>[];
    final format = raw['format'];
    if (format is int && format > kLauncherUiFormat) {
      warnings.add('Made with a newer UI format ($format). Unknown fields are ignored.');
    }

    String text(Object? value, String fallback, int max, String field) {
      if (value is! String) return fallback;
      final cleaned =
          value.replaceAll(RegExp(r'[\u0000-\u001F\u007F]'), ' ').trim();
      if (cleaned.isEmpty) return fallback;
      if (cleaned.length > max) {
        warnings.add('$field was shortened to $max characters.');
        return cleaned.substring(0, max);
      }
      return cleaned;
    }

    var accent = defaults.accent;
    if (raw.containsKey('accent')) {
      final parsed = parseHexColor(raw['accent']);
      if (parsed == null) {
        warnings.add('Accent color is not a valid #RRGGBB value. Default used.');
      } else if (!accentIsReadable(parsed)) {
        warnings.add('Accent color is too dark to read on the dark menu. Default used.');
      } else {
        accent = parsed;
      }
    }

    final background =
        MenuBackgroundStyle.values.asNameMap()[raw['background']] ??
            defaults.background;
    final beatBop = raw['beatBop'] is bool ? raw['beatBop'] as bool : true;

    final entries = <MenuEntryDef>[];
    final rawEntries = raw['entries'];
    if (rawEntries is List) {
      final seen = <String>{};
      for (final item in rawEntries) {
        if (item is! Map) continue;
        final id = item['id'];
        final base = id is String ? builtInEntry(id) : null;
        if (id is! String || base == null) {
          warnings.add('Entry "$id" is not a built-in destination and was skipped.');
          continue;
        }
        if (!seen.add(id)) continue;
        entries.add(MenuEntryDef(
          id: id,
          label: text(item['label'], base.label, kMaxLabelLength, 'Label'),
          summary: text(item['summary'], base.summary, kMaxSummaryLength, 'Summary'),
          visible: item['visible'] is bool ? item['visible'] as bool : true,
        ));
      }
    } else {
      entries.addAll(defaults.entries);
    }

    for (final protectedId in kProtectedEntryIds) {
      final base = builtInEntry(protectedId)!;
      final index = entries.indexWhere((e) => e.id == protectedId);
      if (index < 0) {
        entries.add(MenuEntryDef(
          id: protectedId,
          label: base.label,
          summary: base.summary,
        ));
        warnings.add('"${base.label}" is required and was restored.');
      } else if (!entries[index].visible) {
        entries[index] = entries[index].copyWith(visible: true);
        warnings.add('"${base.label}" cannot be hidden and was made visible again.');
      }
    }

    return LauncherUiParseResult(
      LauncherUiDefinition(
        wordmarkTop: text(raw['wordmarkTop'], defaults.wordmarkTop, kMaxWordmarkLength, 'Top line'),
        wordmarkMain: text(raw['wordmarkMain'], defaults.wordmarkMain, kMaxWordmarkLength, 'Title'),
        accent: accent,
        background: background,
        beatBop: beatBop,
        entries: entries,
      ),
      warnings,
    );
  }
}

/// Turns the definition into the entries the menu actually shows. Hidden
/// entries are dropped (except protected ones). Status and requirements come
/// from the built-in entries.
List<MenuEntry> resolveMenuEntries(
  LauncherUiDefinition def, {
  required bool developerUnlocked,
}) {
  final out = <MenuEntry>[];
  for (final d in def.entries) {
    if (!d.visible && !kProtectedEntryIds.contains(d.id)) continue;
    final base = builtInEntry(d.id);
    if (base == null) continue;
    out.add(base.copyWith(
      label: d.label.trim().isEmpty ? base.label : d.label,
      summary: d.summary.trim().isEmpty ? base.summary : d.summary,
    ));
  }
  if (out.isEmpty) out.addAll(kMainMenuEntries);
  if (developerUnlocked) out.add(kDeveloperEntry);
  return out;
}
