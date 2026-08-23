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
    test('detects the embedded Windows-sandbox helper LAUNCH failures', () {
      // The real shapes seen from an embedded host with no bundled helpers.
      expect(
        isSandboxHelperFailure(
          'windows sandbox: spawn setup refresh: program not found',
        ),
        isTrue,
      );
      expect(
        isSandboxHelperFailure(
          'windows sandbox: failed to spawn codex-windows-sandbox-setup.exe: '
          'program not found',
        ),
        isTrue,
      );
    });

    test('leaves unrelated turn failures alone', () {
      expect(isSandboxHelperFailure('model overloaded'), isFalse);
      expect(isSandboxHelperFailure('connection closed'), isFalse);
      // Mentions a sandbox but not the Windows helper path — not our case.
      expect(isSandboxHelperFailure('seatbelt sandbox denied write'), isFalse);
    });

    test('does NOT hijack a still-actionable Windows-sandbox remedy', () {
      // These are real, DIFFERENT failures on a host whose helpers ARE present:
      // the user should see the actual remedy, not "switch to Full mode".
      expect(
        isSandboxHelperFailure(
          'windows sandbox: Windows sandbox setup is missing or out of date; '
          'rerun the sandbox setup with elevation',
        ),
        isFalse,
      );
      expect(
        isSandboxHelperFailure(
          'windows sandbox: setup marker version mismatch',
        ),
        isFalse,
      );
    });
  });

  group('probe failure classification', () {
    test('a relay auth rejection is told apart from a dead backend', () {
      // What the relay actually answers the WebSocket upgrade with when the
      // authentication code is missing or stale — the tunnel carried bytes, so
      // the remedy is re-supplying the credential, NOT restarting the host.
      expect(
        isRelayAuthRejection(
          'probe: connect timed out: HTTP error: 403 Forbidden: '
          'missing or invalid authentication code',
        ),
        isTrue,
      );
      expect(isRelayAuthRejection('403 Forbidden'), isTrue);

      // A genuinely silent far end must NOT be reported as an auth problem.
      expect(isRelayAuthRejection('probe: initialize timed out'), isFalse);
      expect(isRelayAuthRejection('connection refused'), isFalse);
    });

    test('a silent far end is classified as a timeout', () {
      expect(isProbeTimeout('probe: initialize timed out'), isTrue);
      expect(isProbeTimeout('probe: connect timed out'), isTrue);
      expect(isProbeTimeout('connection refused'), isFalse);
    });
  });
}
