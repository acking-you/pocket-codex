import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/local_session_view_screen.dart';
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

  testWidgets('resume connects a reachable app-server when none is connected', (
    t,
  ) async {
    // A discoverable, reachable app-server that the user has NOT opened yet.
    final api = FakeBridgeApi(
      services: const [
        ServiceEntry(
          device: 'lb7666',
          kind: 'app',
          name: 'default',
          key: 'pcx:lb7666:app:default',
        ),
      ],
    );
    expect(api.appIsConnected('pcx:lb7666:app:default'), isFalse);

    String? resolved;
    await t.pumpWidget(
      ProviderScope(
        overrides: [bridgeApiProvider.overrideWithValue(api)],
        child: MaterialApp(
          home: Consumer(
            builder: (ctx, ref, _) => TextButton(
              onPressed: () async => resolved = await ensureResumeTarget(ref),
              child: const Text('go'),
            ),
          ),
        ),
      ),
    );
    await t.tap(find.text('go'));
    await t.pump();
    await t.pump();

    // It actively connected the reachable server instead of reporting
    // "no app-server" — the bug was requiring a pre-existing connection.
    expect(resolved, 'pcx:lb7666:app:default');
    expect(api.appConnectCount, 1);
    expect(api.appIsConnected('pcx:lb7666:app:default'), isTrue);
  });

  testWidgets('groups by activity time and the search box filters content', (
    t,
  ) async {
    // Pin "now" to a fixed mid-day instant and thread it into the screen via its
    // injected `clock`. The seeded "today" timestamps below are minutes before
    // this same instant, so they're unambiguously within the current day — and
    // because the screen reads the *same* fixed `now`, the test and production
    // clocks can never straddle midnight. With the real wall clock this flaked
    // when CI ran just after 00:00 UTC: `nowSec - 120/300/600` fell into the
    // previous calendar day, emptying the 今天 group and failing the assertion.
    final now = DateTime(2024, 6, 15, 12); // noon, local
    final nowSec = now.millisecondsSinceEpoch ~/ 1000;
    LocalSession mk(
      String id,
      String preview,
      int updated, {
      String safety = 'resumable',
      bool running = false,
    }) => LocalSession(
      threadId: id,
      cwd: '/proj',
      preview: preview,
      source: 'cli',
      updatedAt: updated,
      turnState: running ? 'incomplete' : 'completed',
      heldOpen: running,
      safety: safety,
      allowsResume: !running,
      requiresTakeover: false,
    );
    // >6 sessions spanning running / today / earlier so all three groups and
    // the search box render.
    final api = FakeBridgeApi()
      ..localSessions = [
        mk(
          'a',
          'running now',
          nowSec - 30,
          safety: 'ownedRunning',
          running: true,
        ),
        mk('b', 'today one', nowSec - 120),
        mk('c', 'today two', nowSec - 300),
        mk('d', 'today three', nowSec - 600),
        mk('e', 'old uniquexyz', nowSec - 3 * 86400),
        mk('f', 'old two', nowSec - 4 * 86400),
        mk('g', 'old three', nowSec - 5 * 86400),
      ];
    await t.pumpWidget(_host(LocalSessionsScreen(clock: () => now), api));
    await _settle(t);

    // Activity-time section headers (zh). '进行中' is the group label, distinct
    // from the running chip's '其他进程运行中'.
    expect(find.text('进行中'), findsOneWidget); // groupActive
    expect(find.text('今天'), findsOneWidget); // groupToday
    expect(find.text('更早'), findsOneWidget); // groupEarlier
    // The search box appears once there are more than 6 sessions.
    expect(find.byKey(const Key('local-search')), findsOneWidget);

    // Typing filters across the preview text.
    await t.enterText(find.byKey(const Key('local-search')), 'uniquexyz');
    await _settle(t);
    expect(find.text('old uniquexyz'), findsOneWidget);
    expect(find.text('today one'), findsNothing);
    expect(find.text('running now'), findsNothing);
  });

  testWidgets('viewer renders the transcript read-only and offers force-resume '
      'when the owning session is idle', (t) async {
    final api = FakeBridgeApi();
    api.transcripts['thr-owned'] = const [
      ThreadItem(id: 't1', itemType: 'userMessage', title: '', text: 'hello'),
      ThreadItem(
        id: 't2',
        itemType: 'agentMessage',
        title: '',
        text: 'hi there',
      ),
      ThreadItem(
        id: 't3',
        itemType: 'commandExecution',
        title: 'ls -la',
        text: 'file1\nfile2',
      ),
    ];
    // Held by a desktop codex app-server, but its turn has finished (idle).
    api.liveness['thr-owned'] = const SessionLiveness(
      threadId: 'thr-owned',
      turnState: 'completed',
      heldOpen: true,
      safety: 'ownedIdle',
      allowsResume: true,
      requiresTakeover: true,
      holders: [Holder(pid: 21348, name: 'codex.exe')],
    );
    await t.pumpWidget(
      _host(
        const LocalSessionViewScreen(threadId: 'thr-owned', preview: 'hello'),
        api,
      ),
    );
    await _settle(t);

    // The on-disk transcript renders read-only (no composer).
    expect(find.text('ls -la'), findsOneWidget);
    expect(find.text('hi there'), findsOneWidget);
    // Idle + held ⇒ a force-takeover action is offered.
    expect(find.byKey(const Key('view-resume')), findsOneWidget);
    expect(find.text('强制接管'), findsOneWidget); // forceTakeover (zh)

    await t.pumpWidget(const SizedBox()); // dispose → cancel the poll timer
  });

  testWidgets(
    'viewer stays read-only (no resume) while the session is actively '
    'running elsewhere',
    (t) async {
      final api = FakeBridgeApi();
      api.liveness['thr-run'] = const SessionLiveness(
        threadId: 'thr-run',
        turnState: 'incomplete',
        heldOpen: true,
        safety: 'ownedRunning',
        allowsResume: false,
        requiresTakeover: false,
        holders: [],
      );
      await t.pumpWidget(
        _host(const LocalSessionViewScreen(threadId: 'thr-run'), api),
      );
      await _settle(t);

      // No resume action; a read-only note is shown instead.
      expect(find.byKey(const Key('view-resume')), findsNothing);
      expect(
        find.text('只读 — 其他客户端正在使用此会话'),
        findsOneWidget,
      ); // readOnlyViewing (zh)

      await t.pumpWidget(const SizedBox()); // dispose → cancel the poll timer
    },
  );
}
