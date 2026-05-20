// End-to-end test: load the bundled Rust dylib and confirm a
// round-trip call to `greet` produces the expected output.

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'package:pocket_codex/main.dart';
import 'package:pocket_codex/src/rust/frb_generated.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());
  testWidgets('Rust greet round-trip', (WidgetTester tester) async {
    await tester.pumpWidget(const PocketCodexApp());
    expect(find.textContaining('Hello, Pocket-Codex!'), findsOneWidget);
  });
}
