import 'package:flutter/foundation.dart' show kDebugMode;
import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import '../test/theme_toggle_test.dart' show themeToggleTests;

// Profile, like Release, disables assertions and debug-only render getters.
// Flutter Driver cannot attach to Release, so exercise this path in Profile.
void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();
  testWidgets('native regression runs with debug getters disabled', (
    tester,
  ) async {
    expect(kDebugMode, isFalse);
  });
  themeToggleTests();
}
