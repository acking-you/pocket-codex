// End-to-end test: load the bundled Rust dylib and confirm a round-trip
// call into the bridge (the sample `bridge_version` API) succeeds.

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'package:pocket_codex/src/rust/api/simple.dart';
import 'package:pocket_codex/src/rust/frb_generated.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async => RustLib.init());
  testWidgets('Rust bridge round-trip', (tester) async {
    final version = bridgeVersion();
    expect(version, isNotEmpty);
  });
}
