@TestOn('windows || linux')
library;

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/web_authenticator.dart';

void main() {
  test('the desktop OAuth callback names 127.0.0.1, not localhost', () {
    // flutter_web_auth_2 binds its loopback listener to IPv4 only
    // (`HttpServer.bind('127.0.0.1', …)`). On a machine where `localhost`
    // resolves to the IPv6 `::1` — the Windows default, and what broke sign-in
    // here — the browser follows the redirect to `::1`, finds nothing listening,
    // and the callback never arrives. The flow then hangs until the plugin's
    // timeout and blames the browser.
    //
    // Naming the literal address sidesteps resolution, so this must not drift
    // back to `localhost`: the failure it causes is silent and slow.
    final cb = webAuthCallback();
    expect(cb.redirectUri, 'http://127.0.0.1:$desktopCallbackPort');
    expect(cb.callbackScheme, cb.redirectUri);
    expect(
      cb.redirectUri,
      isNot(contains('localhost')),
      reason:
          'a name that may resolve to ::1 cannot reach an IPv4-only listener',
    );
  });

  test('the callback URI is one the plugin and the backend both accept', () {
    // The plugin parses the port out of `callbackScheme` and requires
    // scheme==http with host localhost or 127.0.0.1; the backend's redirect
    // allowlist requires the same host set. Assert the shape both sides check.
    final uri = Uri.parse(webAuthCallback().callbackScheme);
    expect(uri.scheme, 'http');
    expect(uri.host, anyOf('127.0.0.1', 'localhost'));
    expect(uri.hasPort, isTrue);
    expect(uri.port, desktopCallbackPort);
  });

  test('the desktop landing page survives being written as latin-1', () async {
    // flutter_web_auth_2 writes this page with `HttpResponse.write` after
    // setting `Content-Type: text/html` with NO charset, so Dart encodes the
    // body as latin-1. A literal CJK character throws INSIDE the listener's
    // request handler, which kills the listener — the port closes and the
    // browser shows ERR_CONNECTION_REFUSED, failing sign-in at the last step
    // with the callback already in hand.
    //
    // Reproduced end to end rather than asserted on the string, so the test
    // fails for the same reason production did.
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));

    Object? handlerFailure;
    server.listen((req) async {
      try {
        // Exactly the plugin's two lines.
        req.response.headers.add('Content-Type', 'text/html');
        req.response.write(desktopLandingPage);
      } catch (e) {
        handlerFailure = e;
      }
      await req.response.close();
    });

    final client = HttpClient();
    addTearDown(client.close);
    final response = await (await client.get(
      '127.0.0.1',
      server.port,
      '/',
    )).close();
    await response.drain<void>();

    expect(
      handlerFailure,
      isNull,
      reason:
          'the landing page must be latin-1 encodable; write Chinese as '
          'numeric entities (&#NNNN;) instead of literal characters',
    );
  });

  test('the landing page still shows Chinese, via entities', () {
    // The constraint above must not be met by dropping the localization: the
    // page is bilingual on purpose, since it is served by a plain listener with
    // no access to the app's l10n.
    expect(desktopLandingPage, contains('&#'), reason: 'entities expected');
    expect(desktopLandingPage, contains('<meta charset="utf-8">'));
    expect(desktopLandingPage, contains('Signed in.'));
  });

  test('an IPv4-only listener is unreachable over ::1', () async {
    // The mechanism itself, so the reasoning above stays verifiable rather than
    // becoming folklore. Skipped where the host has no IPv6 loopback at all.
    final server = await HttpServer.bind(InternetAddress.loopbackIPv4, 0);
    addTearDown(() => server.close(force: true));

    Object? failure;
    try {
      final socket = await Socket.connect(
        InternetAddress.loopbackIPv6,
        server.port,
        timeout: const Duration(seconds: 2),
      );
      socket.destroy();
    } on SocketException catch (e) {
      failure = e;
    }
    expect(
      failure,
      isNotNull,
      reason:
          'binding IPv4 must not accidentally serve ::1; if it does, the '
          'localhost-vs-127.0.0.1 distinction would be moot',
    );
  });
}
