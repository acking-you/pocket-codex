import 'package:flutter_rust_bridge/flutter_rust_bridge.dart'
    show AnyhowException;
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/error_format.dart';

void main() {
  test('unwraps AnyhowException and drops the backtrace', () {
    final e = AnyhowException(
      'cannot bind 127.0.0.1:18180 (os error 10048)\n\n'
      'Stack backtrace:\n'
      '   0: <unknown>\n'
      '   1: <unknown>',
    );
    expect(friendlyError(e), 'cannot bind 127.0.0.1:18180 (os error 10048)');
  });

  test('keeps the anyhow caused-by chain, only cuts the backtrace', () {
    final e = AnyhowException(
      'subscribe failed\n\nCaused by:\n    relay unreachable\n\n'
      'Stack backtrace:\n   0: <unknown>',
    );
    expect(
      friendlyError(e),
      'subscribe failed\n\nCaused by:\n    relay unreachable',
    );
  });

  test('passes through plain errors untouched', () {
    expect(friendlyError(StateError('boom')), contains('boom'));
  });

  test('handles a backtrace with no leading blank line', () {
    final e = AnyhowException('boom\nStack backtrace:\n   0: <unknown>');
    expect(friendlyError(e), 'boom');
  });

  group('isSandboxHelperFailure', () {
    test('detects the embedded Windows-sandbox helper spawn failures', () {
      // The real shapes seen from an embedded host with no bundled helpers.
      expect(
        isSandboxHelperFailure(
          'windows sandbox: spawn setup refresh: program not found',
        ),
        isTrue,
      );
      expect(
        isSandboxHelperFailure(
          'failed to spawn codex-windows-sandbox-setup.exe: program not found',
        ),
        isTrue,
      );
      expect(
        isSandboxHelperFailure('Windows Sandbox setup failed with status 1'),
        isTrue,
      );
    });

    test('leaves unrelated turn failures alone', () {
      expect(isSandboxHelperFailure('model overloaded'), isFalse);
      expect(isSandboxHelperFailure('connection closed'), isFalse);
      // Mentions a sandbox but not the Windows helper path — not our case.
      expect(isSandboxHelperFailure('seatbelt sandbox denied write'), isFalse);
    });
  });
}
