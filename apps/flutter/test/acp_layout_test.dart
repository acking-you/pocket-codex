// ACP layouts with the app's real light and dark themes on a 320 and a 390
// phone, an 800 tablet and a 1440 desktop, in English and Chinese: the
// hosting form with long custom agent names, host details after a failure
// (with restart), and a session with long agent option labels and several
// permission requests at once.
//
// Set PCX_UI_PREVIEW_DIR to also write each layout as a PNG for review.

import 'dart:io';
import 'dart:ui' as ui;

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/app_session/agent_permission_card.dart';
import 'package:pocket_codex/src/screens/app_session_screen.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/widgets/local_host_dialog.dart';

import 'fake_bridge_api.dart';

const _acp = 'pcx:dev:acp:agent';
const _preview = Key('acp-preview');

const _devices = [
  ('phone320', Size(320, 640)),
  ('phone390', Size(390, 844)),
  ('tablet800', Size(800, 1100)),
  ('desktop1440', Size(1440, 900)),
];

const _names = {
  'en': 'An exceptionally long custom agent name for reviews and refactoring',
  'zh': '一个名字非常非常长的自定义智能体用于代码审查与大规模重构工作',
};

const _program =
    '/Applications/A Very Long Application Name.app/Contents/MacOS/agent-binary';

AppCapabilities _caps(String agent) => AppCapabilities(
  provider: 'acp',
  fast: false,
  permissionPresets: false,
  guardian: false,
  rateLimits: false,
  takeover: false,
  externalWriterMonitor: false,
  localSessions: false,
  planMode: false,
  protocol: 'acp',
  providerName: agent,
  negotiated: true,
  generation: 1,
  sessionConfig: true,
  permissionOptions: true,
  sessionReopen: 'load',
  sessionList: 'agent',
  runningInventory: 'engine',
);

Widget _app(Widget home, BridgeApi api, Locale locale, Brightness brightness) =>
    ProviderScope(
      overrides: [bridgeApiProvider.overrideWithValue(api)],
      child: MaterialApp(
        locale: locale,
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        theme: brightness == Brightness.light ? lightTheme() : darkTheme(),
        // Wraps the navigator, so dialogs are part of the captured image.
        builder: (context, child) =>
            RepaintBoundary(key: _preview, child: child),
        home: home,
      ),
    );

void _size(WidgetTester t, Size size) {
  t.view.physicalSize = size;
  t.view.devicePixelRatio = 1;
  addTearDown(t.view.reset);
}

Future<void> _frames(WidgetTester t) async {
  for (var i = 0; i < 8; i++) {
    await t.pump(const Duration(milliseconds: 50));
  }
}

/// Nothing laid out past the right edge of the screen.
void _withinWidth(WidgetTester t, Finder finder, Size size) {
  for (final element in finder.evaluate()) {
    final box = element.renderObject;
    if (box is! RenderBox || !box.hasSize) continue;
    final rect = box.localToGlobal(Offset.zero) & box.size;
    expect(
      rect.left,
      greaterThanOrEqualTo(-0.5),
      reason: '$finder starts on screen',
    );
    expect(
      rect.right,
      lessThanOrEqualTo(size.width + 0.5),
      reason: '$finder ends on screen',
    );
  }
}

Future<void> _capture(WidgetTester t, String name) async {
  final dir = Platform.environment['PCX_UI_PREVIEW_DIR'];
  if (dir == null) return;
  final boundary = t.renderObject<RenderRepaintBoundary>(find.byKey(_preview));
  await t.runAsync(() async {
    final image = await boundary.toImage();
    final data = await image.toByteData(format: ui.ImageByteFormat.png);
    await Directory(dir).create(recursive: true);
    await File('$dir/$name.png').writeAsBytes(data!.buffer.asUint8List());
  });
}

Future<void> _openDialog(
  WidgetTester t,
  FakeBridgeApi api,
  Locale locale,
  Brightness brightness, {
  AppServeStatus? existing,
}) async {
  await t.pumpWidget(
    _app(
      Builder(
        builder: (context) => Scaffold(
          body: TextButton(
            onPressed: () => showDialog<void>(
              context: context,
              builder: (_) => LocalHostDialog(existing: existing),
            ),
            child: const Text('open'),
          ),
        ),
      ),
      api,
      locale,
      brightness,
    ),
  );
  await t.tap(find.text('open'));
  await t.pumpAndSettle();
}

AppEvent _permission(String handle, String title, List<String> options) {
  final encoded = [
    for (var i = 0; i < options.length; i++)
      '{"optionId":"$handle-$i","name":"${options[i]}",'
          '"kind":"${i == 0
              ? 'allow_once'
              : i == 1
              ? 'allow_always'
              : 'reject_once'}"}',
  ].join(',');
  return AppEvent(
    kind: agentPermissionKind,
    threadId: 'ses-1',
    requestId: handle,
    title: title,
    raw:
        '{"threadId":"ses-1","toolCall":{"title":"$title",'
        '"locations":[{"path":"$_program"}]},"options":[$encoded]}',
  );
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  setUpAll(() async {
    if (Platform.environment['PCX_UI_PREVIEW_DIR'] == null) return;
    await (FontLoader(
      'MaterialIcons',
    )..addFont(rootBundle.load('fonts/MaterialIcons-Regular.otf'))).load();
    final prose = FontLoader('Figtree');
    for (final weight in ['Light', 'Regular', 'Medium', 'SemiBold', 'Bold']) {
      prose.addFont(rootBundle.load('assets/fonts/Figtree-$weight.ttf'));
    }
    await prose.load();
    final mono = FontLoader('GeistMono');
    for (final weight in ['Regular', 'Medium', 'SemiBold']) {
      mono.addFont(rootBundle.load('assets/fonts/GeistMono-$weight.ttf'));
    }
    await mono.load();
    // The headless engine has no OS CJK fonts. This preview-only fallback
    // does not add the desktop font to the mobile asset bundle.
    await (FontLoader('Noto Sans SC')..addFont(
          File(
            'assets/fonts/NotoSansSC-VariableFont_wght.ttf',
          ).readAsBytes().then(ByteData.sublistView),
        ))
        .load();
  });
  setUp(AppSessionScreen.debugResetThreadMemory);

  for (final (device, size) in _devices) {
    for (final brightness in Brightness.values) {
      for (final language in ['en', 'zh']) {
        final locale = Locale(language);
        final variant = '$device-${brightness.name}-$language';

        testWidgets('custom agent form with a long name fits ($variant)', (
          t,
        ) async {
          _size(t, size);
          final l10n = lookupAppLocalizations(locale);
          final api = FakeBridgeApi();
          await _openDialog(t, api, locale, brightness);
          await t.tap(find.byKey(const Key('provider-acp')));
          await t.pumpAndSettle();
          await t.ensureVisible(find.byKey(const Key('acp-agent-choice')));
          await t.pumpAndSettle();
          await t.tap(find.byKey(const Key('acp-agent-choice')));
          await t.pumpAndSettle();
          await t.tap(find.text(l10n.acpAgentCustom).last);
          await t.pumpAndSettle();
          await t.enterText(
            find.byKey(const Key('acp-display-name-field')),
            _names[language]!,
          );
          await t.enterText(
            find.byKey(const Key('acp-program-field')),
            _program,
          );
          await t.ensureVisible(find.byKey(const Key('acp-arg-add')));
          await t.tap(find.byKey(const Key('acp-arg-add')));
          await t.pumpAndSettle();
          await t.enterText(
            find.byKey(const Key('acp-arg-field-0')),
            '--workspace=${_names[language]}',
          );
          // Let the debounced program lookup settle.
          await t.pump(const Duration(milliseconds: 400));
          await t.pumpAndSettle();
          expect(t.takeException(), isNull, reason: 'no overflow');
          _withinWidth(t, find.byKey(const Key('acp-host-form')), size);
          _withinWidth(t, find.byKey(const Key('host-provider-picker')), size);
          _withinWidth(t, find.byKey(const Key('acp-program-browse')), size);
          await t.ensureVisible(find.byKey(const Key('start-hosting-btn')));
          await t.pumpAndSettle();
          _withinWidth(t, find.byKey(const Key('start-hosting-btn')), size);
          final start = t.getRect(find.byKey(const Key('start-hosting-btn')));
          expect(
            start.bottom,
            lessThanOrEqualTo(size.height),
            reason: 'reachable',
          );
          expect(start.top, greaterThanOrEqualTo(0));
          await _capture(t, 'acp-form-$variant');
        });

        testWidgets('failed host details with restart fit ($variant)', (
          t,
        ) async {
          _size(t, size);
          final host = AppServeStatus(
            name: 'agent',
            device: 'local',
            alive: false,
            appListenAddr: '127.0.0.1:18200',
            appServiceKey: _acp,
            appRegistered: true,
            metaListenAddr: '127.0.0.1:18201',
            metaServiceKey: 'pcx:dev:meta:agent',
            metaRegistered: true,
            codexBinary: _program,
            provider: 'acp',
            protocol: 'acp',
            providerName: _names[language]!,
            profileId: 'custom-agent-0123abcd',
            agentPhase: 'failed',
            agentError: language == 'zh'
                ? '智能体在启动时退出，没有完成协议握手，请检查程序路径和参数后重新启动。'
                : 'the agent exited during the ACP handshake; check the program '
                      'path and its arguments, then restart it',
            authRequired: true,
          );
          final api = FakeBridgeApi()..serveHosts.add(host);
          await _openDialog(t, api, locale, brightness, existing: host);
          expect(t.takeException(), isNull, reason: 'no overflow');
          expect(find.byKey(const Key('acp-agent-error')), findsOneWidget);
          expect(find.byKey(const Key('acp-auth-required')), findsOneWidget);
          _withinWidth(t, find.byKey(const Key('acp-host-details')), size);
          _withinWidth(
            t,
            find.byKey(const Key('provider-badge-acp-tag')),
            size,
          );
          await _capture(t, 'acp-details-$variant');
          await t.ensureVisible(find.byKey(const Key('acp-restart')));
          await t.pumpAndSettle();
          _withinWidth(t, find.byKey(const Key('acp-restart')), size);
          await t.tap(find.byKey(const Key('acp-restart')));
          await t.pumpAndSettle();
          expect(api.acpRestartCalls, ['agent']);
          expect(t.takeException(), isNull);
        });

        testWidgets(
          'long agent options and several permission requests fit ($variant)',
          (t) async {
            _size(t, size);
            final agent = _names[language]!;
            final api = FakeBridgeApi()..acpCapabilities[_acp] = _caps(agent);
            final long = language == 'zh'
                ? '一个非常长的模型名称用于测试选项标签在窄屏上的显示效果'
                : 'An extremely long model name to test option labels on narrow screens';
            api.sessionSettings['$_acp|ses-1'] = SessionSettings(
              configOptions: [
                SessionConfigOption(
                  id: 'model',
                  name: language == 'zh' ? '模型' : 'Model',
                  category: 'model',
                  currentValue: 'long',
                  values: [
                    SessionConfigValue(value: 'long', name: long),
                    const SessionConfigValue(value: 'short', name: 'Fast'),
                  ],
                ),
                SessionConfigOption(
                  id: 'thought',
                  name: language == 'zh' ? '思考深度' : 'Thinking depth',
                  category: 'thought_level',
                  currentValue: 'deep',
                  values: [SessionConfigValue(value: 'deep', name: long)],
                ),
              ],
              currentMode: 'code',
              modes: [
                SessionMode(id: 'code', name: long),
                const SessionMode(id: 'ask', name: 'Ask'),
              ],
            );
            await api.appConnect(_acp, 28080);
            await t.pumpWidget(
              _app(
                const AppSessionScreen(serviceKey: _acp, threadId: 'ses-1'),
                api,
                locale,
                brightness,
              ),
            );
            await _frames(t);
            final options = language == 'zh'
                ? ['仅这一次允许执行这个命令', '总是允许这个项目中的同类命令', '拒绝并告诉智能体换一种做法']
                : [
                    'Allow this command once',
                    'Always allow commands like this in this project',
                    'Reject and ask the agent to try another way',
                  ];
            final title = language == 'zh'
                ? '运行一个很长的命令来检查整个仓库里所有测试的状态'
                : 'Run a long command that checks every test in the whole repository';
            for (final handle in ['h1', 'h2', 'h3']) {
              api.pushEvent(_acp, _permission(handle, title, options));
            }
            await _frames(t);
            expect(t.takeException(), isNull, reason: 'no overflow');
            final cards = find.byKey(const Key('agent-permission-card'));
            expect(cards, findsNWidgets(3), reason: 'every request is listed');
            final list = t.getRect(
              find.byKey(const Key('agent-permission-list')),
            );
            expect(
              list.height,
              lessThanOrEqualTo(size.height * 0.45 + 0.5),
              reason: 'requests share a bounded area',
            );
            _withinWidth(
              t,
              find.byKey(const Key('agent-permission-list')),
              size,
            );
            final input = t.getRect(find.byKey(const Key('composer-input')));
            expect(
              input.bottom,
              lessThanOrEqualTo(size.height),
              reason: 'composer stays on screen',
            );
            expect(
              input.top,
              greaterThan(list.top),
              reason: 'below the requests',
            );
            expect(
              find.byKey(const Key('agent-session-options')),
              findsOneWidget,
            );
            _withinWidth(
              t,
              find.byKey(const Key('agent-session-options')),
              size,
            );
            // The options share a horizontal viewport on compact screens.
            await t.ensureVisible(find.byKey(const Key('agent-config-model')));
            await _frames(t);
            _withinWidth(t, find.byKey(const Key('agent-config-model')), size);
            await _capture(t, 'acp-session-$variant');
            if (size.width <= 390) {
              api.pushEvent(
                _acp,
                const AppEvent(
                  kind: 'turn/started',
                  threadId: 'ses-1',
                  raw: '{"threadId":"ses-1","turn":{"id":"running"}}',
                ),
              );
              await t.enterText(
                find.byKey(const Key('composer-input')),
                'Follow up',
              );
              t.view.viewInsets = const FakeViewPadding(bottom: 280);
              await _frames(t);
              expect(t.takeException(), isNull, reason: 'keyboard fits');
              for (final key in ['composer-input', 'send-btn', 'stop-btn']) {
                final control = find.byKey(Key(key));
                expect(control.hitTestable(), findsOneWidget);
                expect(
                  t.getRect(control).bottom,
                  lessThanOrEqualTo(size.height - 280),
                  reason: '$key remains above the keyboard',
                );
              }
              await _capture(t, 'acp-keyboard-$variant');
            }
            // Every request stays answerable: the last one scrolls into view.
            final lastOption = find
                .byKey(const Key('agent-permission-option-0'))
                .last;
            await t.ensureVisible(lastOption);
            await _frames(t);
            await t.tap(lastOption);
            await _frames(t);
            expect(api.lastApprovalDecision, 'h3-0');
            expect(cards, findsNWidgets(2));
            expect(t.takeException(), isNull);
          },
        );
      }
    }
  }
}
