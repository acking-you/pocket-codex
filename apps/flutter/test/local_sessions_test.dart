import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/local_sessions_screen.dart';
import 'fake_bridge_api.dart';

/// Mount [child] with a fake bridge + localizations (Chinese, matching the
/// other screen tests' zh assertions).
Widget _host(Widget child, BridgeApi api) => ProviderScope(
  overrides: [bridgeApiProvider.overrideWithValue(api)],
  child: MaterialApp(
    locale: const Locale('zh'),
    localizationsDelegates: AppLocalizations.localizationsDelegates,
    supportedLocales: AppLocalizations.supportedLocales,
    home: child,
  ),
);

/// Settle without `pumpAndSettle`: the running session's pulsing dot animates
/// forever, so let the initial async load + any dialog transition complete with
/// a few bounded pumps instead.
Future<void> _settle(WidgetTester t) async {
  await t.pump(); // run the async appLocalSessions() microtask
  await t.pump(const Duration(milliseconds: 250));
  await t.pump(const Duration(milliseconds: 250));
}

void main() {
  // Three sessions spanning the resume-safety states the UI must distinguish.
  const sessions = [
    LocalSession(
      threadId: 'thr-free',
      cwd: '/proj',
      preview: 'free finished session',
      source: 'cli',
      updatedAt: 0,
      turnState: 'completed',
      heldOpen: false,
      safety: 'resumable',
      allowsResume: true,
      requiresTakeover: false,
    ),
    LocalSession(
      threadId: 'thr-owned',
      cwd: '/proj',
      preview: 'desktop-held session',
      source: 'vscode',
      updatedAt: 0,
      turnState: 'completed',
      heldOpen: true,
      safety: 'ownedIdle',
      allowsResume: true,
      requiresTakeover: true,
    ),
    LocalSession(
      threadId: 'thr-running',
      cwd: '/proj',
      preview: 'running session',
      source: 'cli',
      updatedAt: 0,
      turnState: 'incomplete',
      heldOpen: true,
      safety: 'ownedRunning',
      allowsResume: false,
      requiresTakeover: false,
    ),
  ];

  testWidgets('lists sessions with their resume-safety chips', (t) async {
    final api = FakeBridgeApi()..localSessions = sessions;
    await t.pumpWidget(_host(const LocalSessionsScreen(), api));
    await _settle(t);

    // Each session shows its preview...
    expect(find.text('free finished session'), findsOneWidget);
    expect(find.text('desktop-held session'), findsOneWidget);
    expect(find.text('running session'), findsOneWidget);
    // ...and the safety chip for its state (zh labels).
    expect(find.text('可恢复'), findsOneWidget); // resumable
    expect(find.text('被其他进程占用'), findsOneWidget); // ownedIdle
    expect(find.text('其他进程运行中'), findsOneWidget); // ownedRunning
  });

  testWidgets('a running session is read-only (no resume action)', (t) async {
    final api = FakeBridgeApi()..localSessions = sessions;
    await t.pumpWidget(_host(const LocalSessionsScreen(), api));
    await _settle(t);

    // Free + owned-idle expose a resume action; the actively-running one does
    // not (it must stay read-only).
    expect(find.byKey(const Key('resume-thr-free')), findsOneWidget);
    expect(find.byKey(const Key('resume-thr-owned')), findsOneWidget);
    expect(find.byKey(const Key('resume-thr-running')), findsNothing);
  });

  testWidgets('force-takeover opens a confirm dialog listing the holders', (
    t,
  ) async {
    final api = FakeBridgeApi()..localSessions = sessions;
    // The owned-idle session is held by a desktop codex app-server.
    api.liveness['thr-owned'] = const SessionLiveness(
      threadId: 'thr-owned',
      turnState: 'completed',
      heldOpen: true,
      safety: 'ownedIdle',
      allowsResume: true,
      requiresTakeover: true,
      holders: [Holder(pid: 21348, name: 'codex.exe')],
    );
    await t.pumpWidget(_host(const LocalSessionsScreen(), api));
    await _settle(t);

    // The held session's action is labelled "force takeover" (not plain resume).
    expect(find.widgetWithText(TextButton, '强制接管'), findsOneWidget);

    await t.tap(find.byKey(const Key('resume-thr-owned')));
    await _settle(t);

    // A confirm dialog appears, listing the holder process that will be killed.
    expect(find.byKey(const Key('takeover-dialog')), findsOneWidget);
    expect(find.text('codex.exe · PID 21348'), findsOneWidget);
  });

  testWidgets('empty state when there are no local sessions', (t) async {
    final api = FakeBridgeApi()..localSessions = const [];
    await t.pumpWidget(_host(const LocalSessionsScreen(), api));
    await t.pumpAndSettle();
    expect(find.text('没有本地会话'), findsOneWidget); // noLocalSessions (zh)
  });
}
