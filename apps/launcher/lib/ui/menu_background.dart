import 'package:flutter/material.dart';

import 'fnf_tokens.dart';

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

/// Original animated backdrop: slow diagonal stripes that scroll one full
/// stripe per bar (so the loop is seamless) and warm up very slightly on each
/// beat. Purely decorative, so it is excluded from semantics.
///
/// When [running] is false (reduced motion, tests, menu hidden) it paints a
/// static frame.
class MenuBackground extends StatelessWidget {
  const MenuBackground({super.key, required this.beat, required this.running});

  final Animation<double> beat;
  final bool running;

  @override
  Widget build(BuildContext context) {
    return ExcludeSemantics(
      child: RepaintBoundary(
        child: AnimatedBuilder(
          animation: beat,
          builder: (context, _) {
            final phase = running ? beat.value : 0.0;
            final pulse = running ? beatPulse(beat.value) : 0.0;
            return SizedBox.expand(
              child: CustomPaint(painter: _StripePainter(phase: phase, pulse: pulse)),
            );
          },
        ),
      ),
    );
  }
}

class _StripePainter extends CustomPainter {
  const _StripePainter({required this.phase, required this.pulse});

  final double phase;
  final double pulse;

  static const double _spacing = 112;
  static const double _width = 44;

  @override
  void paint(Canvas canvas, Size size) {
    canvas.clipRect(Offset.zero & size);
    final base =
        Color.lerp(FnfColors.background, FnfColors.surfaceRaised, 0.45)!;
    final paint = Paint()
      ..color = Color.lerp(base, FnfColors.accent, pulse * 0.10)!;
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
      oldDelegate.phase != phase || oldDelegate.pulse != pulse;
}
