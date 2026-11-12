import 'package:flutter_test/flutter_test.dart';
import 'package:fnf_universal/main.dart';

void main() {
  testWidgets('FNF Universal Studio exposes project save and load actions', (tester) async {
    await tester.pumpWidget(const FnfUniversalApp());

    expect(find.text('FNF Universal Studio'), findsOneWidget);
    expect(find.byTooltip('Save project'), findsOneWidget);
    expect(find.byTooltip('Load saved project'), findsOneWidget);

    await tester.tap(find.text('Chart'));
    await tester.pumpAndSettle();
    expect(find.text('Chart setup'), findsOneWidget);

    await tester.tap(find.text('Character'));
    await tester.pumpAndSettle();
    expect(find.text('Character definition'), findsOneWidget);

    await tester.tap(find.text('Stage'));
    await tester.pumpAndSettle();
    expect(find.text('Stage setup'), findsOneWidget);
  });
}
