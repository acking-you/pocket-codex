// The macOS Dock icon follows the app's appearance. macOS ships one AppIcon per
// bundle (no asset-catalog light/dark variant), so the swap is a runtime
// platform call — these tests pin what gets sent and when.
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/dock_icon.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  const channel = MethodChannel('pocket_codex/dock_icon');
  final calls = <MethodCall>[];

  setUp(() {
    calls.clear();
    DockIcon.reset();
    debugDefaultTargetPlatformOverride = TargetPlatform.macOS;
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          return null;
        });
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
    debugDefaultTargetPlatformOverride = null;
  });

  test('pushes the resolved appearance, and only on a change', () async {
    DockIcon.apply(Brightness.dark);
    await pumpEventQueue();
    expect(calls.single.method, 'setAppearance');
    expect(calls.single.arguments, {'mode': 'dark'});

    // Re-applying the same appearance is a no-op: this runs from a build
    // callback, so it must not hit the platform every frame.
    DockIcon.apply(Brightness.dark);
    await pumpEventQueue();
    expect(calls, hasLength(1));

    DockIcon.apply(Brightness.light);
    await pumpEventQueue();
    expect(calls, hasLength(2));
    expect(calls.last.arguments, {'mode': 'light'});
  });

  test('is a no-op off macOS', () async {
    debugDefaultTargetPlatformOverride = TargetPlatform.android;
    DockIcon.apply(Brightness.dark);
    await pumpEventQueue();
    expect(calls, isEmpty);
  });

  test('a platform failure does not throw into the frame', () async {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, (call) async {
          calls.add(call);
          throw PlatformException(code: 'boom');
        });
    // The icon is cosmetic — a failed swap must never surface as an uncaught
    // async error (which is fatal on desktop, see main.dart's onError).
    DockIcon.apply(Brightness.dark);
    await pumpEventQueue();
    expect(calls, hasLength(1));
  });
}
