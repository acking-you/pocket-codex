/// Browser-redirect (authorization-code) login plumbing: the platform-specific
/// OAuth callback and a thin, mockable wrapper over flutter_web_auth_2.
library;

import 'dart:io' show Platform;

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
/// (its `callbackUrlScheme` must be a full `http://localhost:{port}`).
WebAuthCallback webAuthCallback() {
  if (Platform.isWindows || Platform.isLinux) {
    final loopback = 'http://localhost:$desktopCallbackPort';
    return (redirectUri: loopback, callbackScheme: loopback);
  }
  return (redirectUri: '$appAuthScheme://auth', callbackScheme: appAuthScheme);
}

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
    // CRITICAL for desktop: flutter_web_auth_2 v4 defaults useWebview=true, which
    // on Windows/Linux routes to an embedded webview that matches the callback by
    // scheme ONLY (`uri.scheme != callbackUrlScheme`). Our desktop callback is a
    // full `http://localhost:{port}`, whose scheme is just `http`, so it would
    // NEVER match and the flow would hang. useWebview=false selects the loopback
    // HTTP-server path (system browser + 127.0.0.1:{port} listener) — exactly the
    // design here. The option is desktop-only; mobile/macOS native sessions
    // (custom `pocketcodex` scheme) ignore it.
    options: const FlutterWebAuth2Options(useWebview: false),
  );
}
