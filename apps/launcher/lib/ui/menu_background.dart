import 'package:flutter/material.dart';
import 'dart:typed_data';

import 'fnf_tokens.dart';
import 'menu_definition.dart';

/// Placeholder tempo for the menu's idle groove. Replace with a real song BPM
/// once the audio layer exists.
const int kMenuBpm = 100;

/// One bar (4 beats) at [kMenuBpm]. The menu's beat controller loops over this.
const Duration kMenuBar = Duration(milliseconds: 60000 * 4 ~/ kMenuBpm);

/// Beat pulse for a bar phase in [0, 1): jumps to 1 on each beat, then eases
/// back to 0 before the next one.
double beatPulse(double barPhase) {
  final p = (barPhase * 4) % 1.0;
  final inv = 1.0 - p;
  return inv * inv * inv;
}

/// Menu backdrop. Three styles:
/// - stripes: original diagonal stripes that scroll one stripe per bar and
///   warm slightly on each beat.
/// - image: the player's wallpaper, darkened by [dim] so text stays readable.
///   Falls back to stripes while the image is missing or loading.
/// - plain: nothing.
///
/// Purely decorative, so excluded from semantics. When [running] is false
/// (reduced motion, tests, menu hidden) it paints a static frame.
class MenuBackground extends StatelessWidget {
  const MenuBackground({
    super.key,
    required this.beat,
    required this.running,
    required this.accent,
    this.style = MenuBackgroundStyle.stripes,
    this.imageBytes,
    this.dim = 0.55,
  });

  final Animation<double> beat;
  final bool running;
  final Color accent;
  final MenuBackgroundStyle style;
  final Uint8List? imageBytes;
  final double dim;

  @override
  Widget build(BuildContext context) {
    if (style == MenuBackgroundStyle.plain) return const SizedBox.shrink();
    final bytes = imageBytes;
    if (style == MenuBackgroundStyle.image && bytes != null) {
      return ExcludeSemantics(
        child: Stack(
          fit: StackFit.expand,
          children: [
            Image.memory(
              bytes,
              fit: BoxFit.cover,
              gaplessPlayback: true,
              errorBuilder: (context, error, stack) => const SizedBox.shrink(),
            ),
            ColoredBox(color: FnfColors.background.withValues(alpha: dim)),
            if (running)
              AnimatedBuilder(
                animation: beat,
                builder: (context, _) => ColoredBox(
                  color: accent.withValues(alpha: beatPulse(beat.value) * 0.06),
                ),
              ),
          ],
        ),
      );
    }
    return ExcludeSemantics(
      child: RepaintBoundary(
        child: AnimatedBuilder(
          animation: beat,
          builder: (context, _) {
            final phase = running ? beat.value : 0.0;
            final pulse = running ? beatPulse(beat.value) : 0.0;
            return SizedBox.expand(
              child: CustomPaint(
                painter: _StripePainter(phase: phase, pulse: pulse, accent: accent),
              ),
            );
          },
        ),
      ),
    );
  }
}

class _StripePainter extends CustomPainter {
  const _StripePainter({
    required this.phase,
    required this.pulse,
    required this.accent,
  });

  final double phase;
  final double pulse;
  final Color accent;

  static const double _spacing = 112;
  static const double _width = 44;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.clipRect(Offset.zero & size);
    final base =
        Color.lerp(FnfColors.background, FnfColors.surfaceRaised, 0.45)!;
    final paint = Paint()..color = Color.lerp(base, accent, pulse * 0.10)!;
    final shift = phase * _spacing;
    for (double x = -size.height - _spacing;
        x < size.width + _spacing;
        x += _spacing) {
      final dx = x + shift;
      final path = Path()
        ..moveTo(dx, size.height)
        ..lineTo(dx + _width, size.height)
        ..lineTo(dx + _width + size.height, 0)
        ..lineTo(dx + size.height, 0)
        ..close();
      canvas.drawPath(path, paint);
    }
  }

  @override
  bool shouldRepaint(_StripePainter oldDelegate) =>
      oldDelegate.phase != phase ||
      oldDelegate.pulse != pulse ||
      oldDelegate.accent != accent;
}
