/// Browser-redirect (authorization-code) login plumbing: the platform-specific
/// OAuth callback and a thin, mockable wrapper over flutter_web_auth_2.
library;

import 'dart:io' show Platform;

import 'package:flutter/foundation.dart'
    show TargetPlatform, defaultTargetPlatform, kIsWeb, visibleForTesting;
import 'package:flutter_web_auth_2/flutter_web_auth_2.dart';

/// The custom URL scheme registered for the browser-redirect login deep link
/// (Android intent-filter / iOS + macOS CFBundleURLSchemes). Mobile + macOS use
/// it; Windows + Linux use a loopback http server instead.
const String appAuthScheme = 'pocketcodex';

/// The loopback port flutter_web_auth_2 listens on for the desktop
/// (Windows/Linux) browser flow. Any free high port works — GitHub never sees it
/// (only the backend's callback is registered with GitHub); the backend just
/// redirects the browser here at the end. Pinned high to avoid common clashes.
const int desktopCallbackPort = 53682;

/// One platform's web-auth callback: the `redirectUri` the backend redirects the
/// browser to, and the `callbackScheme` flutter_web_auth_2 watches for.
typedef WebAuthCallback = ({String redirectUri, String callbackScheme});

/// Resolve the platform-appropriate callback. Mobile + macOS use the app's
/// custom scheme (a deep link captured by ASWebAuthenticationSession / Custom
/// Tabs); Windows + Linux use a loopback http server flutter_web_auth_2 spins up
/// (its `callbackUrlScheme` must be a full `http://localhost:{port}` or
/// `http://127.0.0.1:{port}`).
///
/// # Why the desktop redirect names 127.0.0.1, not localhost
///
/// The plugin binds its listener to IPv4 only (`HttpServer.bind('127.0.0.1', …)`
/// in its `server.dart`). On a machine where `localhost` resolves to the IPv6
/// `::1` — the default on Windows, and what this project hit — the browser
/// follows the redirect to `::1`, finds nothing listening, and the callback never
/// arrives: the flow appears to hang and then fails on the plugin's timeout with
/// no indication why.
///
/// Naming the literal address sidesteps name resolution entirely, so the browser
/// reaches the listener whichever family `localhost` happens to prefer.
/// `callbackScheme` matches, since the plugin parses the port out of it and
/// accepts either host.
WebAuthCallback webAuthCallback() {
  if (Platform.isWindows || Platform.isLinux) {
    final loopback = 'http://127.0.0.1:$desktopCallbackPort';
    return (redirectUri: loopback, callbackScheme: loopback);
  }
  return (redirectUri: '$appAuthScheme://auth', callbackScheme: appAuthScheme);
}

/// Whether the device code should be the DEFAULT sign-in, with the browser
/// redirect offered as the alternative.
///
/// True on desktop. The two idioms don't cost the same per platform: a phone has
/// one browser with shared cookies and a deep link that returns straight to the
/// app, so tapping through is the shortest path. On a desktop the redirect has
/// to come back through a loopback listener or a custom scheme, and it usually
/// opens whichever browser profile is default — often not the one signed into
/// GitHub. The device code has no redirect at all and reaches GitHub through the
/// backend, so it also survives a proxy or VPN that the in-app browser can't.
///
/// Gated on `defaultTargetPlatform` (not `dart:io`) so `flutter test` — which
/// reports android — exercises the mobile default deterministically.
bool get prefersDeviceCodeLogin =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.linux);

/// Drives the system browser / in-app auth tab for the browser-redirect login.
/// Abstracted behind an interface so widget tests can supply a fake instead of
/// the real (platform-channel) plugin.
abstract interface class WebAuthenticator {
  /// Open [url] and resolve with the final redirect URL once the browser reaches
  /// a URL whose scheme matches [callbackUrlScheme]. Throws a
  /// `PlatformException(code: 'CANCELED')` if the user dismisses the tab.
  Future<String> authenticate({
    required String url,
    required String callbackUrlScheme,
  });
}

/// Real [WebAuthenticator] backed by flutter_web_auth_2.
class FlutterWebAuthenticator implements WebAuthenticator {
  /// Creates the real authenticator.
  const FlutterWebAuthenticator();

  @override
  Future<String> authenticate({
    required String url,
    required String callbackUrlScheme,
  }) => FlutterWebAuth2.authenticate(
    url: url,
    callbackUrlScheme: callbackUrlScheme,
    options: FlutterWebAuth2Options(
      // CRITICAL for desktop: flutter_web_auth_2 v4 defaults useWebview=true,
      // which on Windows/Linux routes to an embedded webview that matches the
      // callback by scheme ONLY (`uri.scheme != callbackUrlScheme`). Our desktop
      // callback is a full `http://localhost:{port}`, whose scheme is just
      // `http`, so it would NEVER match and the flow would hang.
      // useWebview=false selects the loopback HTTP-server path (system browser +
      // 127.0.0.1:{port} listener) — exactly the design here. The option is
      // desktop-only; mobile/macOS native sessions ignore it.
      useWebview: false,
      // Android only. Adds FLAG_ACTIVITY_NO_HISTORY to the Custom Tab, so the
      // tab leaves no entry in the back stack: once the redirect fires, Back
      // returns to the app instead of to a spent authorize page the user then
      // can't get out of.
      intentFlags: ephemeralIntentFlags,
      // Windows/Linux only. The loopback listener otherwise waits out the
      // plugin's 5-minute default with no way to give up, which reads as a hung
      // app when the browser never comes back (a redirect that landed in another
      // profile, or a network that can't reach GitHub). 90s is long enough to
      // type a password and a 2FA code, short enough to fail visibly — and the
      // failure lands on the device-code hint.
      timeout: 90,
      // Windows/Linux only: what the browser shows after the redirect. The
      // plugin's stock page is a bare English "You may now close this page";
      // this one names the app and says the app is already continuing, so the
      // user knows the browser is done rather than wondering what to do next.
      landingPageHtml: _landingPage,
    ),
  );
}

/// The page the desktop loopback listener serves once GitHub redirects back.
///
/// Deliberately self-contained (no network, no fonts to fetch): it renders on a
/// machine that may have just failed to reach the internet. Text is bilingual
/// rather than localized — this is served by a plain HTTP listener that has no
/// access to the app's l10n, and a signed-in user reads it for two seconds.
///
/// # This source must stay ASCII-only
///
/// flutter_web_auth_2 writes this with `HttpResponse.write` after setting
/// `Content-Type: text/html` with NO charset, and Dart then encodes the body as
/// latin-1. A literal CJK character therefore throws `Invalid argument (string):
/// Contains invalid characters` INSIDE the listener's request handler, which kills
/// the listener: the port closes and the browser shows ERR_CONNECTION_REFUSED, so
/// sign-in fails at the very last step with the callback already in hand.
///
/// The Chinese text is written as numeric character references so the bytes are
/// ASCII while the browser still renders 中文. The `<meta charset>` is what makes
/// the entities decode; it cannot rescue raw high bytes, because the failure
/// happens before anything is sent.
@visibleForTesting
const String desktopLandingPage = _landingPage;

const String _landingPage = '''
<!DOCTYPE html>
<html>
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Pocket-Codex</title>
  <style>
    html, body { margin: 0; padding: 0; background: #F9F9F7; }
    main {
      display: flex; flex-direction: column; align-items: center;
      justify-content: center; min-height: 100vh; gap: 0.75rem;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Helvetica,
                   Arial, sans-serif;
      color: #1A1A19; text-align: center; padding: 0 1.5rem;
    }
    h1 { font-size: 1.25rem; font-weight: 600; margin: 0; }
    p { margin: 0; color: rgba(26, 26, 25, 0.75); line-height: 1.5; }
    .sub { font-size: 0.875rem; color: rgba(26, 26, 25, 0.5); }
    @media (prefers-color-scheme: dark) {
      html, body { background: #171717; }
      main { color: #FFFFFF; }
      p { color: rgba(255, 255, 255, 0.75); }
      .sub { color: rgba(255, 255, 255, 0.5); }
    }
  </style>
</head>
<body>
  <main>
    <h1>Pocket-Codex</h1>
    <!-- Chinese as numeric entities, NOT literal characters: see the note on
         _landingPage. A raw CJK byte anywhere in this literal kills the
         listener before the response is sent. -->
    <p>&#24050;&#30331;&#24405;&#65292;&#21487;&#20197;&#20851;&#38381;&#27492;&#39029;&#38754;&#12290;</p>
    <p class="sub">Signed in. You can close this page and return to the app.</p>
  </main>
</body>
</html>
''';
