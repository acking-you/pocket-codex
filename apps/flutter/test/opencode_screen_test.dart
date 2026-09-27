import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/opencode_api.dart';
import 'package:pocket_codex/src/screens/opencode_screen.dart';
import 'opencode_controller_test.dart' show FakeOpenCodeApi;

Widget host(FakeOpenCodeApi api, {bool dark = false, String locale = 'en'}) =>
    ProviderScope(
      child: MaterialApp(
        locale: Locale(locale),
        theme: ThemeData(brightness: dark ? Brightness.dark : Brightness.light),
        localizationsDelegates: AppLocalizations.localizationsDelegates,
        supportedLocales: AppLocalizations.supportedLocales,
        home: OpenCodeScreen(api: api),
      ),
    );

void main() {
  testWidgets('local discovery is explicit and does not forward form secrets', (
    t,
  ) async {
    final api = FakeOpenCodeApi();
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.enterText(
      find.byKey(const Key('opencode-password')),
      'not-for-local',
    );
    expect(api.connections, isEmpty);
    await t.tap(find.byKey(const Key('opencode-connect-local')));
    await t.pumpAndSettle();
    expect(api.connections.single, {
      'baseUrl': null,
      'serviceKey': null,
      'directory': '/work',
      'username': 'opencode',
      'password': null,
    });
    expect(find.text('Session A'), findsOneWidget);
    expect(api.decisions, isEmpty);
  });

  for (final failure in {
    'AUTH': 'Authentication failed. Check the OpenCode service credentials.',
    'PROTOCOL': 'Unsupported or invalid OpenCode service protocol.',
    'NETWORK':
        'Cannot reach the OpenCode service. Check its address and status.',
    'LOCAL_DISCOVERY': 'No verified local OpenCode service was found.',
  }.entries) {
    testWidgets('connection reports ${failure.key} without exposing errors', (
      t,
    ) async {
      final api = FakeOpenCodeApi()
        ..connectionError = StateError(
          'PCX_OPENCODE_${failure.key}: private-password-and-url',
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      expect(find.text(failure.value), findsOneWidget);
      expect(find.textContaining('private-password-and-url'), findsNothing);
    });
  }

  testWidgets(
    'native v2 message text, tools and unknown records remain visible',
    (t) async {
      final api = FakeOpenCodeApi()
        ..current = const OpenCodeSnapshot(
          sessionId: 'a',
          messages: [
            {'id': 'user', 'type': 'user', 'text': 'Native user request'},
            {
              'id': 'assistant',
              'type': 'assistant',
              'content': [
                {'type': 'text', 'text': 'Native assistant reply'},
                {'type': 'reasoning', 'text': 'Native reasoning'},
                {
                  'type': 'tool',
                  'id': 'tool',
                  'name': 'bash',
                  'state': {
                    'status': 'completed',
                    'input': {'command': 'git status'},
                    'content': [
                      {'type': 'text', 'text': 'Native tool output'},
                    ],
                  },
                },
              ],
            },
            {'id': 'system', 'type': 'system', 'text': 'Native system notice'},
            {
              'id': 'future',
              'type': 'future.message',
              'value': 'Preserved detail',
              'content': 'Opaque future content',
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      expect(find.text('Native assistant reply'), findsOneWidget);
      expect(find.text('Native system notice'), findsOneWidget);
      expect(find.text('future.message'), findsWidgets);
      await t.tap(find.text('bash · completed'));
      await t.pumpAndSettle();
      expect(find.text('Native tool output'), findsOneWidget);
      await t.tap(find.text('reasoning'));
      await t.pumpAndSettle();
      expect(find.text('Native reasoning'), findsOneWidget);
      expect(t.takeException(), isNull);
    },
  );

  testWidgets('v2 permission always is explicit persistent project access', (
    t,
  ) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(
        sessionId: 'a',
        permissions: [
          {
            'id': 'permission',
            'sessionID': 'a',
            'action': 'shell',
            'resources': ['git status'],
            'save': ['git *'],
          },
        ],
      );
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    expect(find.text('shell'), findsOneWidget);
    expect(find.text('git status'), findsOneWidget);
    expect(find.text('Allow for this instance'), findsNothing);
    expect(api.decisions, isEmpty);
    await t.tap(find.text('Always allow for this project'));
    await t.pumpAndSettle();
    expect(
      find.text(
        'OpenCode will save this permission for the project, including future sessions and service restarts.',
      ),
      findsOneWidget,
    );
    expect(find.text('git *'), findsOneWidget);
    await t.tap(find.text('Cancel'));
    await t.pumpAndSettle();
    expect(api.decisions, isEmpty);
    await t.tap(find.text('Allow once'));
    await t.pumpAndSettle();
    expect(api.decisions, ['permission:once']);
  });

  testWidgets('v2 permission without save never offers persistent approval', (
    t,
  ) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(
        sessionId: 'a',
        permissions: [
          {
            'id': 'permission',
            'action': 'shell',
            'resources': ['pwd'],
          },
        ],
      );
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    expect(find.text('Always allow for this project'), findsNothing);
    expect(find.text('Allow for this instance'), findsNothing);
    expect(find.text('Allow once'), findsOneWidget);
  });

  testWidgets(
    'v2 forms submit typed fields with defaults and active conditions',
    (t) async {
      t.view.physicalSize = const Size(1200, 1000);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      final api = FakeOpenCodeApi()
        ..current = const OpenCodeSnapshot(
          sessionId: 'a',
          questions: [
            {
              'id': 'form',
              'sessionID': 'a',
              'title': 'Build options',
              'state': {'status': 'pending'},
              'fields': [
                {
                  'key': 'name',
                  'type': 'string',
                  'title': 'Name',
                  'required': true,
                },
                {
                  'key': 'count',
                  'type': 'integer',
                  'title': 'Count',
                  'default': 2,
                  'minimum': 1,
                },
                {
                  'key': 'ratio',
                  'type': 'number',
                  'title': 'Ratio',
                  'default': 0.5,
                },
                {
                  'key': 'enabled',
                  'type': 'boolean',
                  'title': 'Enabled',
                  'default': false,
                },
                {
                  'key': 'targets',
                  'type': 'multiselect',
                  'title': 'Targets',
                  'options': [
                    {'value': 'mac', 'label': 'macOS'},
                    {'value': 'linux', 'label': 'Linux'},
                  ],
                  'default': ['mac'],
                },
                {
                  'key': 'hidden',
                  'type': 'string',
                  'default': 'default-value',
                  'hidden': true,
                },
                {
                  'key': 'inactive',
                  'type': 'string',
                  'title': 'Extra',
                  'required': true,
                  'when': [
                    {'key': 'enabled', 'op': 'eq', 'value': true},
                  ],
                },
              ],
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      await t.tap(find.text('Answer form'));
      await t.pumpAndSettle();
      expect(find.text('Build options'), findsOneWidget);
      expect(find.byKey(const Key('opencode-form-inactive')), findsNothing);
      expect(find.byKey(const Key('opencode-form-hidden')), findsNothing);
      await t.tap(find.text('Submit answers'));
      await t.pumpAndSettle();
      expect(api.formAnswers, isNull);
      expect(find.text('Required'), findsOneWidget);
      await t.enterText(find.byKey(const Key('opencode-form-name')), 'Pocket');
      await t.tap(find.text('Linux'));
      await t.pump();
      await t.tap(find.text('Submit answers'));
      await t.pumpAndSettle();
      expect(api.formAnswers, {
        'name': 'Pocket',
        'count': 2,
        'ratio': 0.5,
        'enabled': false,
        'targets': ['mac', 'linux'],
        'hidden': 'default-value',
      });
      expect(api.answers, isNull);
    },
  );

  testWidgets('v2 form answered by another writer cannot be submitted again', (
    t,
  ) async {
    const form = {
      'id': 'form',
      'sessionID': 'a',
      'title': 'Name',
      'state': {'status': 'pending'},
      'fields': [
        {'key': 'name', 'type': 'string', 'default': 'Pocket'},
      ],
    };
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(sessionId: 'a', questions: [form]);
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    await t.tap(find.text('Answer form'));
    await t.pumpAndSettle();
    api.stream.add(
      OpenCodeSnapshot(
        sessionId: 'a',
        revision: 1,
        questions: [
          {
            ...form,
            'state': {
              'status': 'answered',
              'answer': {'name': 'Other'},
            },
          },
        ],
      ),
    );
    await t.tap(find.text('Submit answers'));
    await t.pumpAndSettle();
    expect(api.formAnswers, isNull);
    expect(
      t
          .widget<TextButton>(find.widgetWithText(TextButton, 'Answer form'))
          .onPressed,
      isNull,
    );
  });

  for (final example in [
    ({'type': 'integer', 'minimum': 1}, '0', '2'),
    ({'type': 'integer'}, '1.5', '2'),
    ({'type': 'number', 'maximum': 2}, '3', '1.5'),
    ({'type': 'string', 'minLength': 2}, 'a', 'abc'),
    ({'type': 'string', 'maxLength': 3}, 'long', 'abc'),
  ]) {
    testWidgets('v2 form validates ${example.$1}', (t) async {
      final api = FakeOpenCodeApi()
        ..current = OpenCodeSnapshot(
          sessionId: 'a',
          questions: [
            {
              'id': 'form',
              'title': 'Validated field',
              'fields': [
                {'key': 'value', ...example.$1},
              ],
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      await t.tap(find.text('Answer form'));
      await t.pumpAndSettle();
      await t.enterText(
        find.byKey(const Key('opencode-form-value')),
        example.$2,
      );
      await t.tap(find.text('Submit answers'));
      await t.pumpAndSettle();
      expect(api.formAnswers, isNull);
      await t.enterText(
        find.byKey(const Key('opencode-form-value')),
        example.$3,
      );
      await t.tap(find.text('Submit answers'));
      await t.pumpAndSettle();
      expect(api.formAnswers, isNotNull);
    });
  }

  for (final constraint in [
    {'pattern': '^safe-'},
    {'format': 'email'},
    {'format': 'uri'},
    {'format': 'date'},
    {'format': 'date-time'},
  ]) {
    testWidgets('v2 form refuses unsupported native constraint $constraint', (
      t,
    ) async {
      final api = FakeOpenCodeApi()
        ..current = OpenCodeSnapshot(
          sessionId: 'a',
          questions: [
            {
              'id': 'form',
              'title': 'Constrained field',
              'fields': [
                {'key': 'value', 'type': 'string', ...constraint},
              ],
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      await t.tap(find.text('Answer form'));
      await t.pumpAndSettle();
      expect(
        find.text(
          'This form has unsupported fields or constraints. Complete it in OpenCode.',
        ),
        findsOneWidget,
      );
      expect(
        t
            .widget<FilledButton>(
              find.widgetWithText(FilledButton, 'Submit answers'),
            )
            .onPressed,
        isNull,
      );
      expect(find.byKey(const Key('opencode-form-value')), findsNothing);
      expect(api.formAnswers, isNull);
    });
  }

  testWidgets(
    'v2 form choices use option values and allow explicit custom text',
    (t) async {
      t.view.physicalSize = const Size(1100, 1000);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      final api = FakeOpenCodeApi()
        ..current = const OpenCodeSnapshot(
          sessionId: 'a',
          questions: [
            {
              'id': 'form',
              'title': 'Choices',
              'fields': [
                {
                  'key': 'engine',
                  'type': 'string',
                  'required': true,
                  'options': [
                    {'value': 'rust', 'label': 'Rust host'},
                  ],
                },
                {
                  'key': 'nickname',
                  'type': 'string',
                  'custom': true,
                  'options': [
                    {'value': 'default', 'label': 'Default name'},
                  ],
                },
                {
                  'key': 'targets',
                  'type': 'multiselect',
                  'custom': true,
                  'minItems': 2,
                  'maxItems': 2,
                  'options': [
                    {'value': 'mac', 'label': 'macOS'},
                  ],
                },
              ],
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      await t.tap(find.text('Answer form'));
      await t.pumpAndSettle();
      await t.tap(find.text('Rust host'));
      await t.enterText(
        find.byKey(const Key('opencode-form-nickname')),
        'Custom name',
      );
      await t.tap(find.text('macOS'));
      await t.tap(find.text('Submit answers'));
      await t.pumpAndSettle();
      expect(api.formAnswers, isNull);
      await t.enterText(
        find.byKey(const Key('opencode-form-custom-targets')),
        'custom-target',
      );
      await t.tap(find.text('Submit answers'));
      await t.pumpAndSettle();
      expect(api.formAnswers, {
        'engine': 'rust',
        'nickname': 'Custom name',
        'targets': ['mac', 'custom-target'],
      });
    },
  );

  testWidgets(
    'v2 conditions retain empty defaults and omit inactive descendants',
    (t) async {
      final api = FakeOpenCodeApi()
        ..current = const OpenCodeSnapshot(
          sessionId: 'a',
          questions: [
            {
              'id': 'form',
              'title': 'Conditional fields',
              'fields': [
                {'key': 'mode', 'type': 'string', 'default': ''},
                {
                  'key': 'child',
                  'type': 'string',
                  'default': 'child-value',
                  'when': [
                    {'key': 'mode', 'op': 'eq', 'value': ''},
                  ],
                },
                {
                  'key': 'descendant',
                  'type': 'string',
                  'default': 'deep',
                  'when': [
                    {'key': 'child', 'op': 'eq', 'value': 'child-value'},
                  ],
                },
                {'key': 'missing', 'type': 'string'},
                {
                  'key': 'unanswered',
                  'type': 'string',
                  'required': true,
                  'when': [
                    {'key': 'missing', 'op': 'neq', 'value': 'yes'},
                  ],
                },
              ],
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      await t.tap(find.text('Answer form'));
      await t.pumpAndSettle();
      expect(find.byKey(const Key('opencode-form-child')), findsOneWidget);
      expect(find.byKey(const Key('opencode-form-descendant')), findsOneWidget);
      expect(find.byKey(const Key('opencode-form-unanswered')), findsNothing);
      await t.enterText(find.byKey(const Key('opencode-form-mode')), 'other');
      await t.pump();
      expect(find.byKey(const Key('opencode-form-child')), findsNothing);
      expect(find.byKey(const Key('opencode-form-descendant')), findsNothing);
      await t.tap(find.text('Submit answers'));
      await t.pumpAndSettle();
      expect(api.formAnswers, {'mode': 'other'});
    },
  );

  testWidgets('v2 multiselect defaults preserve exact custom values', (
    t,
  ) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(
        sessionId: 'a',
        questions: [
          {
            'id': 'form',
            'title': 'Custom defaults',
            'fields': [
              {
                'key': 'values',
                'type': 'multiselect',
                'custom': true,
                'options': [],
                'default': [' original\n value '],
              },
            ],
          },
        ],
      );
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    await t.tap(find.text('Answer form'));
    await t.pumpAndSettle();
    await t.tap(find.text('Submit answers'));
    await t.pumpAndSettle();
    expect(api.formAnswers, {
      'values': [' original\n value '],
    });
  });

  testWidgets('v2 forms never silently drop malformed fields', (t) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(
        sessionId: 'a',
        questions: [
          {
            'id': 'form',
            'title': 'Unknown field shape',
            'fields': [
              {'key': 'valid', 'type': 'string', 'default': 'not enough'},
              'future-field-shape',
            ],
          },
        ],
      );
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    await t.tap(find.text('Answer form'));
    await t.pumpAndSettle();
    expect(
      find.text(
        'This form has unsupported fields or constraints. Complete it in OpenCode.',
      ),
      findsOneWidget,
    );
    expect(
      t
          .widget<FilledButton>(
            find.widgetWithText(FilledButton, 'Submit answers'),
          )
          .onPressed,
      isNull,
    );
    expect(api.formAnswers, isNull);
  });

  testWidgets('v2 integer defaults do not silently overflow native integers', (
    t,
  ) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(
        sessionId: 'a',
        questions: [
          {
            'id': 'form',
            'title': 'Large integer',
            'fields': [
              {'key': 'value', 'type': 'integer', 'default': 1e20},
            ],
          },
        ],
      );
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    await t.tap(find.text('Answer form'));
    await t.pumpAndSettle();
    await t.tap(find.text('Submit answers'));
    await t.pumpAndSettle();
    expect(api.formAnswers, {'value': 1e20});
  });

  testWidgets('external v2 form cannot acknowledge or open an external action', (
    t,
  ) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(
        sessionId: 'a',
        questions: [
          {
            'id': 'external-form',
            'title': 'External sign-in',
            'fields': [
              {
                'key': 'auth',
                'type': 'external',
                'url': 'https://example.test/private-token',
              },
            ],
          },
        ],
      );
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    await t.tap(find.text('Answer form'));
    await t.pumpAndSettle();
    expect(
      find.text(
        'This form has unsupported fields or constraints. Complete it in OpenCode.',
      ),
      findsOneWidget,
    );
    expect(
      t
          .widget<FilledButton>(
            find.widgetWithText(FilledButton, 'Submit answers'),
          )
          .onPressed,
      isNull,
    );
    expect(find.textContaining('private-token'), findsNothing);
    await t.tap(find.text('Cancel'));
    await t.pumpAndSettle();
    expect(api.formAnswers, isNull);
    expect(api.decisions, isEmpty);
    await t.tap(find.text('Cancel form'));
    await t.pumpAndSettle();
    expect(api.decisions, ['external-form:reject']);
  });

  for (final width in [390.0, 800.0, 1440.0]) {
    for (final dark in [false, true]) {
      testWidgets('v2 form stays scrollable at $width dark=$dark', (t) async {
        t.view.physicalSize = Size(width, 740);
        t.view.devicePixelRatio = 1;
        addTearDown(t.view.resetPhysicalSize);
        addTearDown(t.view.resetDevicePixelRatio);
        addTearDown(t.view.resetViewInsets);
        final api = FakeOpenCodeApi()
          ..current = OpenCodeSnapshot(
            sessionId: 'a',
            questions: [
              {
                'id': 'form',
                'title': 'Native form',
                'fields': [
                  for (var i = 0; i < 10; i++)
                    {'key': 'field-$i', 'type': 'string', 'title': 'Field $i'},
                ],
              },
            ],
          );
        await t.pumpWidget(host(api, dark: dark, locale: dark ? 'zh' : 'en'));
        await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
        await t.tap(find.byKey(const Key('opencode-connect')));
        await t.pumpAndSettle();
        await t.tap(find.text('Session A'));
        await t.pumpAndSettle();
        await t.tap(find.text(dark ? '回答表单' : 'Answer form'));
        await t.pumpAndSettle();
        t.view.viewInsets = const FakeViewPadding(bottom: 280);
        await t.pumpAndSettle();
        await t.ensureVisible(find.byKey(const Key('opencode-form-field-9')));
        await t.enterText(
          find.byKey(const Key('opencode-form-field-9')),
          'last',
        );
        await t.pump();
        final submit = find.widgetWithText(
          FilledButton,
          dark ? '提交回答' : 'Submit answers',
        );
        expect(t.getRect(submit).bottom, lessThanOrEqualTo(460));
        expect(t.takeException(), isNull);
        await t.tap(submit);
        await t.pumpAndSettle();
        expect(api.formAnswers, {'field-9': 'last'});
      });
    }
  }

  testWidgets(
    'phone keyboard leaves approvals and composer within the viewport',
    (t) async {
      t.view.physicalSize = const Size(390, 740);
      t.view.devicePixelRatio = 1;
      addTearDown(t.view.resetPhysicalSize);
      addTearDown(t.view.resetDevicePixelRatio);
      addTearDown(t.view.resetViewInsets);
      final api = FakeOpenCodeApi()
        ..current = const OpenCodeSnapshot(
          sessionId: 'a',
          status: 'busy',
          permissions: [
            {
              'id': 'p',
              'permission': 'bash',
              'patterns': ['git status'],
            },
          ],
          questions: [
            {'id': 'q', 'questions': []},
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      t.view.viewInsets = const FakeViewPadding(bottom: 300);
      await t.pump();
      expect(t.takeException(), isNull);
      expect(
        t.getRect(find.byKey(const Key('opencode-composer'))).bottom,
        lessThanOrEqualTo(440),
      );
    },
  );
  for (final width in [390.0, 800.0, 1440.0]) {
    for (final dark in [false, true]) {
      testWidgets(
        'tool and unknown parts remain visible at $width dark=$dark',
        (t) async {
          t.view.physicalSize = Size(width, 900);
          t.view.devicePixelRatio = 1;
          addTearDown(t.view.resetPhysicalSize);
          addTearDown(t.view.resetDevicePixelRatio);
          final api = FakeOpenCodeApi()
            ..current = const OpenCodeSnapshot(
              sessionId: 'a',
              messages: [
                {
                  'info': {'id': 'm', 'role': 'assistant'},
                  'parts': [
                    {
                      'id': 'tool',
                      'type': 'tool',
                      'tool': 'bash',
                      'state': {
                        'status': 'completed',
                        'output': 'working tree clean',
                      },
                    },
                    {'id': 'new', 'type': 'future.kind', 'value': 'retained'},
                  ],
                },
              ],
            );
          await t.pumpWidget(host(api, dark: dark, locale: dark ? 'zh' : 'en'));
          await t.enterText(
            find.byKey(const Key('opencode-directory')),
            '/work',
          );
          await t.tap(find.byKey(const Key('opencode-connect')));
          await t.pumpAndSettle();
          await t.tap(find.text('Session A'));
          await t.pumpAndSettle();
          expect(find.text('bash · completed'), findsOneWidget);
          expect(find.text('future.kind'), findsOneWidget);
          expect(t.takeException(), isNull);
          expect(
            t
                .widget<IconButton>(
                  find.byKey(const Key('opencode-disconnect')),
                )
                .onPressed,
            isNotNull,
          );
          await t.tap(find.byKey(const Key('opencode-disconnect')));
          await t.pumpAndSettle();
          expect(find.text('Request failed. Try again.'), findsNothing);
          expect(api.disconnected, ['connection']);
          expect(find.byKey(const Key('opencode-connect')), findsOneWidget);
        },
      );
    }
  }
  testWidgets('search and explicit history pagination stay bounded', (t) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(sessionId: 'a', nextCursor: 'opaque');
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.enterText(find.byKey(const Key('opencode-search')), 'Title');
    await t.testTextInput.receiveAction(TextInputAction.search);
    await t.pumpAndSettle();
    expect(api.search, 'Title');
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    await t.tap(find.text('Load earlier messages'));
    await t.pumpAndSettle();
    expect(find.text('Older message'), findsOneWidget);
    expect(find.text('Load earlier messages'), findsNothing);
    await t.tap(find.byTooltip('New session'));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('opencode-composer')), findsOneWidget);
  });
  testWidgets('questions collect ordered choices and custom text', (t) async {
    final api = FakeOpenCodeApi()
      ..current = const OpenCodeSnapshot(
        sessionId: 'a',
        questions: [
          {
            'id': 'q',
            'questions': [
              {
                'header': 'Language',
                'question': 'Choose languages',
                'multiple': true,
                'custom': false,
                'options': [
                  {'label': 'Dart', 'description': 'UI'},
                  {'label': 'Rust', 'description': 'Host'},
                ],
              },
              {
                'header': 'Name',
                'question': 'Choose a name',
                'custom': true,
                'options': [],
              },
            ],
          },
        ],
      );
    await t.pumpWidget(host(api));
    await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
    await t.tap(find.byKey(const Key('opencode-connect')));
    await t.pumpAndSettle();
    await t.tap(find.text('Session A'));
    await t.pumpAndSettle();
    await t.tap(find.text('Answer questions'));
    await t.pumpAndSettle();
    await t.tap(find.text('Dart'));
    await t.pump();
    await t.tap(find.text('Rust'));
    await t.pump();
    await t.enterText(find.byKey(const Key('opencode-answer-1')), 'Pocket');
    await t.pump();
    await t.tap(find.text('Submit answers'));
    await t.pumpAndSettle();
    expect(api.answers, [
      ['Dart', 'Rust'],
      ['Pocket'],
    ]);
  });
  testWidgets(
    'permission instance scope is explicit and stop only aborts the session',
    (t) async {
      final api = FakeOpenCodeApi()
        ..current = const OpenCodeSnapshot(
          sessionId: 'a',
          status: 'busy',
          permissions: [
            {
              'id': 'p',
              'permission': 'bash',
              'patterns': ['git status'],
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      await t.tap(find.text('Allow for this instance'));
      await t.pumpAndSettle();
      expect(
        find.text(
          'This also applies to other sessions in this OpenCode instance until it restarts.',
        ),
        findsOneWidget,
      );
      await t.tap(find.text('Allow'));
      await t.pumpAndSettle();
      await t.tap(find.byKey(const Key('opencode-abort')));
      await t.pumpAndSettle();
      expect(api.decisions, ['p:always', 'a:abort']);
    },
  );
  testWidgets(
    'unknown send keeps the text and disables send until reconciled',
    (t) async {
      final api = FakeOpenCodeApi()..submission = OpenCodeSubmission.unknown;
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      await t.enterText(find.byKey(const Key('opencode-composer')), 'Continue');
      await t.pump();
      await t.tap(find.byKey(const Key('opencode-send')));
      await t.pumpAndSettle();
      expect(find.text('Continue'), findsOneWidget);
      expect(
        find.text(
          'Submission unknown. Check the session before sending again.',
        ),
        findsOneWidget,
      );
      expect(
        t.widget<IconButton>(find.byKey(const Key('opencode-send'))).onPressed,
        isNull,
      );
      expect(api.sent, ['Continue']);
      await t.tap(find.text('Review submission'));
      await t.pumpAndSettle();
      expect(
        find.text(
          'The message may already be running. Unlocking does not resend it.',
        ),
        findsOneWidget,
      );
      await t.tap(find.text('I checked the session'));
      await t.pumpAndSettle();
      expect(api.sent, ['Continue']);
      expect(
        t.widget<IconButton>(find.byKey(const Key('opencode-send'))).onPressed,
        isNotNull,
      );
    },
  );
  testWidgets(
    'direct connect opens real history and retains no password in form',
    (t) async {
      final api = FakeOpenCodeApi()
        ..current = const OpenCodeSnapshot(
          sessionId: 'a',
          messages: [
            {
              'info': {'id': 'message', 'role': 'assistant'},
              'parts': [
                {'id': 'part', 'type': 'text', 'text': 'A real reply'},
              ],
            },
          ],
        );
      await t.pumpWidget(host(api));
      await t.enterText(find.byKey(const Key('opencode-directory')), '/work');
      await t.enterText(find.byKey(const Key('opencode-password')), 'secret');
      await t.tap(find.byKey(const Key('opencode-connect')));
      await t.pumpAndSettle();
      await t.tap(find.text('Session A'));
      await t.pumpAndSettle();
      expect(find.text('A real reply'), findsOneWidget);
      expect(find.text('secret'), findsNothing);
    },
  );
}
