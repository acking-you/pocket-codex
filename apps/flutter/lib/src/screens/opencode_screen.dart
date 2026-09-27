import 'dart:convert';
import 'package:flutter/material.dart';
import '../opencode_api.dart';
import '../opencode_controller.dart';
import '../widgets/opencode_form_dialog.dart';
import '../widgets/utility_page.dart';

/// An independent OpenCode conversation surface.
class OpenCodeScreen extends StatefulWidget {
  const OpenCodeScreen({super.key, required this.api, this.serviceKey});
  final OpenCodeApi api;
  final String? serviceKey;
  @override
  State<OpenCodeScreen> createState() => _OpenCodeScreenState();
}

class _OpenCodeScreenState extends State<OpenCodeScreen> {
  late final controller = OpenCodeController(widget.api)..addListener(_changed);
  final url = TextEditingController(text: 'http://127.0.0.1:4096');
  final directory = TextEditingController();
  final username = TextEditingController(text: 'opencode');
  final password = TextEditingController();
  final composer = TextEditingController();
  final scroll = ScrollController();
  bool working = false;
  bool showSessions = true;
  String? failure;
  String text(String en, String zh) =>
      Localizations.localeOf(context).languageCode == 'zh' ? zh : en;

  void _changed() {
    if (mounted) setState(() {});
  }

  Future<void> run(Future<void> Function() action) async {
    if (working) return;
    setState(() {
      working = true;
      failure = null;
    });
    try {
      await action();
    } catch (error) {
      if (mounted) setState(() => failure = _failureText(error));
    } finally {
      if (mounted) setState(() => working = false);
    }
  }

  String _failureText(Object error) {
    final code = RegExp(
      r'PCX_OPENCODE_(AUTH|PROTOCOL|NETWORK|LOCAL_DISCOVERY)\b',
    ).firstMatch(error.toString())?.group(1);
    return switch (code) {
      'AUTH' => text(
        'Authentication failed. Check the OpenCode service credentials.',
        '认证失败，请检查 OpenCode 服务凭据。',
      ),
      'PROTOCOL' => text(
        'Unsupported or invalid OpenCode service protocol.',
        'OpenCode 服务协议不受支持或响应无效。',
      ),
      'NETWORK' => text(
        'Cannot reach the OpenCode service. Check its address and status.',
        '无法连接 OpenCode 服务，请检查地址和运行状态。',
      ),
      'LOCAL_DISCOVERY' => text(
        'No verified local OpenCode service was found.',
        '未发现通过验证的本机 OpenCode 服务。',
      ),
      _ => text('Request failed. Try again.', '请求失败，请重试。'),
    };
  }

  @override
  void dispose() {
    controller.removeListener(_changed);
    controller.dispose();
    for (final field in [url, directory, username, password, composer]) {
      field.dispose();
    }
    scroll.dispose();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) => UtilityPage(
    route: '/opencode',
    title: 'OpenCode',
    actions: [
      if (controller.connectionId != null)
        IconButton(
          key: const Key('opencode-disconnect'),
          tooltip: text('Disconnect', '断开连接'),
          icon: const Icon(Icons.link_off),
          onPressed: working
              ? null
              : () => run(() async {
                  await controller.disconnect();
                  if (mounted) {
                    composer.clear();
                    setState(() => showSessions = true);
                  }
                }),
        ),
    ],
    body: Column(
      children: [
        if (failure != null)
          Padding(
            padding: const EdgeInsets.all(12),
            child: Text(
              failure!,
              style: TextStyle(color: Theme.of(context).colorScheme.error),
            ),
          ),
        if (working)
          Padding(
            padding: const EdgeInsets.all(4),
            child: Text(text('Loading...', '加载中...')),
          ),
        Expanded(
          child: controller.connectionId == null
              ? _connection()
              : LayoutBuilder(
                  builder: (context, size) {
                    final wide = size.maxWidth >= 720;
                    return Row(
                      children: [
                        if (wide || showSessions)
                          SizedBox(
                            width: wide ? 260 : size.maxWidth,
                            child: _sessions(),
                          ),
                        if (wide) const VerticalDivider(width: 1),
                        if (wide || !showSessions)
                          Expanded(child: _conversation()),
                      ],
                    );
                  },
                ),
        ),
      ],
    ),
  );
  Widget _connection() => Center(
    child: ConstrainedBox(
      constraints: const BoxConstraints(maxWidth: 520),
      child: ListView(
        padding: const EdgeInsets.all(24),
        shrinkWrap: true,
        children: [
          if (widget.serviceKey == null)
            TextField(
              key: const Key('opencode-url'),
              controller: url,
              decoration: InputDecoration(
                labelText: text('Server URL', '服务地址'),
              ),
            )
          else
            SelectableText(widget.serviceKey!),
          const SizedBox(height: 12),
          TextField(
            key: const Key('opencode-directory'),
            controller: directory,
            decoration: InputDecoration(
              labelText: text('Host directory', '主机目录'),
            ),
          ),
          if (widget.serviceKey == null) ...[
            const SizedBox(height: 12),
            TextField(
              controller: username,
              decoration: InputDecoration(labelText: text('Username', '用户名')),
            ),
            const SizedBox(height: 12),
            TextField(
              key: const Key('opencode-password'),
              controller: password,
              obscureText: true,
              enableSuggestions: false,
              autocorrect: false,
              decoration: InputDecoration(
                labelText: text('Password (not saved)', '密码（不保存）'),
              ),
            ),
          ],
          const SizedBox(height: 20),
          FilledButton.icon(
            key: const Key('opencode-connect'),
            icon: const Icon(Icons.link),
            label: Text(text('Connect', '连接')),
            onPressed: working
                ? null
                : () => run(() async {
                    if (directory.text.trim().isEmpty) {
                      throw const FormatException();
                    }
                    final secret = password.text;
                    password.clear();
                    await controller.connect(
                      baseUrl: widget.serviceKey == null
                          ? url.text.trim()
                          : null,
                      serviceKey: widget.serviceKey,
                      directory: directory.text.trim(),
                      username: username.text,
                      password: secret.isEmpty ? null : secret,
                    );
                  }),
          ),
          if (widget.serviceKey == null) ...[
            const SizedBox(height: 12),
            OutlinedButton.icon(
              key: const Key('opencode-connect-local'),
              icon: const Icon(Icons.computer),
              label: Text(text('Connect local OpenCode', '连接本机 OpenCode')),
              onPressed: working
                  ? null
                  : () => run(() async {
                      if (directory.text.trim().isEmpty) {
                        throw const FormatException();
                      }
                      password.clear();
                      await controller.connect(
                        directory: directory.text.trim(),
                      );
                    }),
            ),
          ],
        ],
      ),
    ),
  );
  Widget _sessions() => Column(
    children: [
      Padding(
        padding: const EdgeInsets.all(12),
        child: Row(
          children: [
            Expanded(child: Text(text('Recent 100 sessions', '最近 100 个会话'))),
            _newSession(),
          ],
        ),
      ),
      Padding(
        padding: const EdgeInsets.symmetric(horizontal: 12),
        child: TextField(
          key: const Key('opencode-search'),
          textInputAction: TextInputAction.search,
          decoration: InputDecoration(
            labelText: text('Search titles on server', '搜索服务端标题'),
            prefixIcon: const Icon(Icons.search),
          ),
          onSubmitted: (query) => run(() => controller.search(query)),
        ),
      ),
      Expanded(
        child: ListView.builder(
          itemCount: controller.sessions.length,
          itemBuilder: (context, index) {
            final session = controller.sessions[index];
            return ListTile(
              title: Text(
                session.title,
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
              ),
              selected: controller.snapshot?.sessionId == session.id,
              onTap: working
                  ? null
                  : () => run(() async {
                      await controller.select(session.id);
                      if (mounted) {
                        setState(() {
                          showSessions = false;
                          composer.text = controller.draft;
                        });
                      }
                    }),
            );
          },
        ),
      ),
    ],
  );
  Widget _conversation() {
    final snapshot = controller.snapshot;
    if (snapshot == null) {
      return Center(child: Text(text('Select a session', '选择会话')));
    }
    return Column(
      children: [
        Row(
          children: [
            IconButton(
              tooltip: text('Sessions', '会话'),
              icon: const Icon(Icons.list),
              onPressed: () => setState(() => showSessions = true),
            ),
            Expanded(
              child: Text(snapshot.status, overflow: TextOverflow.ellipsis),
            ),
            if (MediaQuery.sizeOf(context).width < 720) _newSession(),
            if (snapshot.busy)
              IconButton(
                key: const Key('opencode-abort'),
                tooltip: text('Stop execution', '停止执行'),
                icon: const Icon(Icons.stop),
                onPressed: !snapshot.writable || working
                    ? null
                    : () => run(
                        () => widget.api.abort(
                          controller.connectionId!,
                          snapshot.sessionId,
                        ),
                      ),
              ),
          ],
        ),
        if (snapshot.permissions.isNotEmpty)
          Flexible(
            flex: 3,
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 180),
              child: ListView(
                shrinkWrap: true,
                children: [
                  for (final permission in snapshot.permissions)
                    _permission(permission, snapshot.writable),
                ],
              ),
            ),
          ),
        if (snapshot.questions.isNotEmpty)
          Flexible(
            child: ConstrainedBox(
              constraints: const BoxConstraints(maxHeight: 100),
              child: ListView(
                shrinkWrap: true,
                children: [
                  for (final question in snapshot.questions)
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      child: Wrap(
                        spacing: 8,
                        children: [
                          TextButton.icon(
                            icon: const Icon(Icons.question_answer_outlined),
                            label: Text(
                              question['fields'] is List
                                  ? text('Answer form', '回答表单')
                                  : text('Answer questions', '回答问题'),
                            ),
                            onPressed:
                                !snapshot.writable ||
                                    working ||
                                    !_pendingQuestion(question)
                                ? null
                                : () => question['fields'] is List
                                      ? _form(question)
                                      : _questions(question),
                          ),
                          TextButton(
                            onPressed:
                                !snapshot.writable ||
                                    working ||
                                    !_pendingQuestion(question)
                                ? null
                                : () => run(
                                    () => widget.api.questionReject(
                                      controller.connectionId!,
                                      '${question['id']}',
                                    ),
                                  ),
                            child: Text(
                              question['fields'] is List
                                  ? text('Cancel form', '取消表单')
                                  : text('Reject question', '拒绝问题'),
                            ),
                          ),
                        ],
                      ),
                    ),
                ],
              ),
            ),
          ),
        if (snapshot.nextCursor != null)
          TextButton(
            onPressed: working ? null : () => run(controller.older),
            child: Text(text('Load earlier messages', '加载更早消息')),
          ),
        Expanded(
          flex: 4,
          child: ListView.builder(
            controller: scroll,
            reverse: true,
            padding: const EdgeInsets.all(16),
            itemCount: snapshot.messages.length,
            itemBuilder: (context, index) {
              final message =
                  snapshot.messages[snapshot.messages.length - 1 - index];
              final info = message['info'] is Map
                  ? message['info'] as Map
                  : null;
              final content = info != null
                  ? message['parts']
                  : message['content'];
              final parts = content is List ? content : const [];
              return Padding(
                key: ValueKey(info?['id'] ?? message['id']),
                padding: const EdgeInsets.only(bottom: 20),
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.start,
                  children: [
                    Text(
                      '${info?['role'] ?? message['type'] ?? 'unknown'}',
                      style: Theme.of(context).textTheme.labelMedium,
                    ),
                    if (info == null && message['text'] is String)
                      SelectableText(message['text'] as String),
                    for (var i = 0; i < parts.length; i++)
                      if (parts[i] is Map)
                        _part(parts[i] as Map, partKey: ValueKey(i)),
                    if (info == null && message['type'] == 'user')
                      for (final field in ['files', 'agents', 'skills'])
                        if (message[field] is List &&
                            (message[field] as List).isNotEmpty)
                          _detail(field, message[field]),
                    if (info == null && message['type'] == 'assistant')
                      for (final field in ['error', 'retry'])
                        if (message[field] != null)
                          _detail(field, message[field]),
                    if (info == null &&
                        ![
                          'user',
                          'assistant',
                          'system',
                          'synthetic',
                          'skill',
                        ].contains(message['type']))
                      _detail('${message['type'] ?? 'unknown'}', message),
                  ],
                ),
              );
            },
          ),
        ),
        if (!snapshot.writable)
          Padding(
            padding: const EdgeInsets.all(8),
            child: Text(
              text('Offline or synchronizing. Read only.', '离线或同步中，只读。'),
            ),
          ),
        if (controller.submissionUnknown)
          Padding(
            padding: const EdgeInsets.all(8),
            child: Text(
              text(
                'Submission unknown. Check the session before sending again.',
                '提交结果未知，请核对会话后再发送。',
              ),
            ),
          ),
        if (controller.submissionUnknown)
          TextButton(
            onPressed: working
                ? null
                : () async {
                    final sessionId = snapshot.sessionId;
                    final confirmed = await showDialog<bool>(
                      context: context,
                      builder: (context) => AlertDialog(
                        title: Text(text('Review submission', '核对提交结果')),
                        content: Text(
                          text(
                            'The message may already be running. Unlocking does not resend it.',
                            '消息可能已在执行。解除锁定不会重新发送。',
                          ),
                        ),
                        actions: [
                          TextButton(
                            onPressed: () => Navigator.pop(context, false),
                            child: Text(text('Cancel', '取消')),
                          ),
                          FilledButton(
                            onPressed: () => Navigator.pop(context, true),
                            child: Text(
                              text('I checked the session', '我已核对会话'),
                            ),
                          ),
                        ],
                      ),
                    );
                    if (confirmed == true && mounted) {
                      controller.acknowledgeUnknown(sessionId);
                    }
                  },
            child: Text(text('Review submission', '核对提交结果')),
          ),
        SafeArea(
          top: false,
          child: Padding(
            padding: const EdgeInsets.all(12),
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.end,
              children: [
                Expanded(
                  child: TextField(
                    key: const Key('opencode-composer'),
                    controller: composer,
                    minLines: 1,
                    maxLines: 5,
                    onChanged: (value) =>
                        setState(() => controller.draft = value),
                    decoration: InputDecoration(
                      hintText: snapshot.busy
                          ? text('Draft (session is running)', '草稿（会话正在执行）')
                          : text('Message', '消息'),
                    ),
                  ),
                ),
                IconButton(
                  key: const Key('opencode-send'),
                  tooltip: text('Send', '发送'),
                  icon: const Icon(Icons.arrow_upward),
                  onPressed:
                      working ||
                          !snapshot.writable ||
                          snapshot.busy ||
                          controller.submitting ||
                          controller.submissionUnknown ||
                          controller.draft.trim().isEmpty
                      ? null
                      : () => run(() async {
                          await controller.send();
                          if (mounted) composer.text = controller.draft;
                        }),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }

  Widget _part(Map part, {Key? partKey}) {
    final type = '${part['type'] ?? 'unknown'}';
    if (type == 'text') return SelectableText('${part['text'] ?? ''}');
    final state = part['state'] is Map ? part['state'] as Map : const {};
    final title = type == 'tool'
        ? '${part['name'] ?? part['tool']} · ${state['status'] ?? ''}'
        : type;
    return ExpansionTile(
      key: partKey ?? (part['id'] == null ? null : ValueKey(part['id'])),
      tilePadding: EdgeInsets.zero,
      title: Text(title),
      children: [
        ConstrainedBox(
          constraints: const BoxConstraints(maxHeight: 240),
          child: SingleChildScrollView(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                if (type == 'tool' && state['content'] is List)
                  for (final content
                      in (state['content'] as List).whereType<Map>())
                    if (content['type'] == 'text')
                      SelectableText('${content['text'] ?? ''}')
                    else
                      SelectableText(
                        const JsonEncoder.withIndent('  ').convert(content),
                      )
                else
                  SelectableText(
                    type == 'tool'
                        ? _displayValue(
                            state['output'] ?? state['error'] ?? state['input'],
                          )
                        : type == 'reasoning'
                        ? '${part['text'] ?? ''}'
                        : const JsonEncoder.withIndent('  ').convert(part),
                  ),
                if (type == 'tool' &&
                    state['content'] is List &&
                    state['error'] != null)
                  SelectableText(_displayValue(state['error'])),
              ],
            ),
          ),
        ),
      ],
    );
  }

  String _displayValue(Object? value) => value is String
      ? value
      : value == null
      ? ''
      : const JsonEncoder.withIndent('  ').convert(value);

  Widget _detail(String title, Object? value) => ExpansionTile(
    tilePadding: EdgeInsets.zero,
    title: Text(title),
    children: [
      ConstrainedBox(
        constraints: const BoxConstraints(maxHeight: 240),
        child: SingleChildScrollView(
          child: SelectableText(_displayValue(value)),
        ),
      ),
    ],
  );

  Widget _newSession() => IconButton(
    tooltip: text('New session', '新建会话'),
    icon: const Icon(Icons.add),
    onPressed: working
        ? null
        : () => run(() async {
            await controller.create();
            if (mounted) {
              setState(() {
                showSessions = false;
                composer.clear();
              });
            }
          }),
  );
  Widget _permission(Map<String, dynamic> permission, bool writable) {
    final native = permission['action'] is String;
    final saved = permission['save'] is List
        ? (permission['save'] as List).whereType<String>().toList()
        : const <String>[];
    final alwaysLabel = native
        ? text('Always allow for this project', '对此项目始终允许')
        : text('Allow for this instance', '允许此实例');
    return Padding(
      padding: const EdgeInsets.all(12),
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text(
            '${permission['action'] ?? permission['permission'] ?? text('Permission', '权限')}',
          ),
          SelectableText(
            ((native ? permission['resources'] : permission['patterns'])
                        as List? ??
                    [])
                .join('\n'),
          ),
          Wrap(
            spacing: 8,
            children: [
              for (final reply in [
                'once',
                if (!native || saved.isNotEmpty) 'always',
                'reject',
              ])
                TextButton(
                  onPressed: !writable || working
                      ? null
                      : () => run(() async {
                          if (reply == 'always') {
                            final accepted = await showDialog<bool>(
                              context: context,
                              builder: (context) => AlertDialog(
                                title: Text(alwaysLabel),
                                content: Column(
                                  mainAxisSize: MainAxisSize.min,
                                  crossAxisAlignment: CrossAxisAlignment.start,
                                  children: [
                                    Text(
                                      native
                                          ? text(
                                              'OpenCode will save this permission for the project, including future sessions and service restarts.',
                                              'OpenCode 将为项目持久保存此权限，对后续会话及服务重启后仍有效。',
                                            )
                                          : text(
                                              'This also applies to other sessions in this OpenCode instance until it restarts.',
                                              '本次允许也会影响同一 OpenCode 实例中的其他会话，直到实例重启。',
                                            ),
                                    ),
                                    if (native)
                                      SelectableText(saved.join('\n')),
                                  ],
                                ),
                                actions: [
                                  TextButton(
                                    onPressed: () =>
                                        Navigator.pop(context, false),
                                    child: Text(text('Cancel', '取消')),
                                  ),
                                  FilledButton(
                                    onPressed: () =>
                                        Navigator.pop(context, true),
                                    child: Text(text('Allow', '允许')),
                                  ),
                                ],
                              ),
                            );
                            if (accepted != true || !mounted) return;
                          }
                          if (controller.snapshot?.writable != true ||
                              !controller.snapshot!.permissions.any(
                                (pending) => pending['id'] == permission['id'],
                              )) {
                            return;
                          }
                          await widget.api.permissionReply(
                            controller.connectionId!,
                            '${permission['id']}',
                            reply,
                          );
                        }),
                  child: Text(switch (reply) {
                    'once' => text('Allow once', '允许一次'),
                    'always' => alwaysLabel,
                    _ => text('Reject', '拒绝'),
                  }),
                ),
            ],
          ),
        ],
      ),
    );
  }

  Future<void> _questions(Map<String, dynamic> request) async {
    final questions = (request['questions'] as List? ?? [])
        .whereType<Map>()
        .toList();
    final selected = [for (final _ in questions) <String>{}];
    final custom = [for (final _ in questions) TextEditingController()];
    final route = DialogRoute<List<List<String>>>(
      context: context,
      builder: (context) => StatefulBuilder(
        builder: (context, change) => AlertDialog(
          title: Text(text('Answer questions', '回答问题')),
          content: SizedBox(
            width: 440,
            child: SingleChildScrollView(
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  for (var i = 0; i < questions.length; i++) ...[
                    Text(
                      '${questions[i]['header'] ?? ''}',
                      style: Theme.of(context).textTheme.titleSmall,
                    ),
                    Text('${questions[i]['question'] ?? ''}'),
                    if (questions[i]['multiple'] != true)
                      RadioGroup<String>(
                        groupValue: selected[i].firstOrNull,
                        onChanged: (value) => change(() {
                          selected[i].clear();
                          custom[i].clear();
                          if (value != null) selected[i].add(value);
                        }),
                        child: Column(
                          children: [
                            for (final option
                                in (questions[i]['options'] as List? ?? [])
                                    .whereType<Map>())
                              RadioListTile<String>(
                                contentPadding: EdgeInsets.zero,
                                value: '${option['label']}',
                                title: Text('${option['label']}'),
                                subtitle: Text(
                                  '${option['description'] ?? ''}',
                                ),
                              ),
                          ],
                        ),
                      ),
                    if (questions[i]['multiple'] == true)
                      for (final option
                          in (questions[i]['options'] as List? ?? [])
                              .whereType<Map>())
                        CheckboxListTile(
                          contentPadding: EdgeInsets.zero,
                          title: Text('${option['label']}'),
                          subtitle: Text('${option['description'] ?? ''}'),
                          value: selected[i].contains(option['label']),
                          onChanged: (checked) => change(() {
                            if (checked == true) {
                              selected[i].add('${option['label']}');
                            } else {
                              selected[i].remove(option['label']);
                            }
                          }),
                        ),
                    if (questions[i]['custom'] != false)
                      TextField(
                        key: Key('opencode-answer-$i'),
                        controller: custom[i],
                        decoration: InputDecoration(
                          labelText: text('Custom answer', '自定义回答'),
                        ),
                        onChanged: (_) => change(() {
                          if (questions[i]['multiple'] != true) {
                            selected[i].clear();
                          }
                        }),
                      ),
                    const SizedBox(height: 16),
                  ],
                ],
              ),
            ),
          ),
          actions: [
            TextButton(
              onPressed: () => Navigator.pop(context),
              child: Text(text('Cancel', '取消')),
            ),
            FilledButton(
              onPressed:
                  questions.isEmpty ||
                      List.generate(
                        questions.length,
                        (i) =>
                            selected[i].isNotEmpty ||
                            custom[i].text.trim().isNotEmpty,
                      ).contains(false)
                  ? null
                  : () => Navigator.pop(context, [
                      for (var i = 0; i < questions.length; i++)
                        [
                          ...selected[i],
                          if (custom[i].text.trim().isNotEmpty)
                            custom[i].text.trim(),
                        ],
                    ]),
              child: Text(text('Submit answers', '提交回答')),
            ),
          ],
        ),
      ),
    );
    final answers = await Navigator.of(context).push(route);
    await route.completed;
    for (final field in custom) {
      field.dispose();
    }
    if (answers != null &&
        mounted &&
        controller.snapshot?.writable == true &&
        controller.snapshot!.questions.any(
          (pending) => pending['id'] == request['id'],
        )) {
      await run(
        () => widget.api.questionReply(
          controller.connectionId!,
          '${request['id']}',
          answers,
        ),
      );
    }
  }

  Future<void> _form(Map<String, dynamic> request) async {
    final connectionId = controller.connectionId;
    final sessionId = controller.snapshot?.sessionId;
    final answers = await showDialog<Map<String, dynamic>>(
      context: context,
      builder: (context) => OpenCodeFormDialog(request: request),
    );
    if (answers == null ||
        !mounted ||
        connectionId == null ||
        connectionId != controller.connectionId ||
        sessionId != controller.snapshot?.sessionId ||
        controller.snapshot?.writable != true ||
        !controller.snapshot!.questions.any(
          (pending) =>
              pending['id'] == request['id'] &&
              _pendingQuestion(pending) &&
              jsonEncode(pending['fields']) == jsonEncode(request['fields']),
        )) {
      return;
    }
    await run(
      () => widget.api.formReply(connectionId, '${request['id']}', answers),
    );
  }

  bool _pendingQuestion(Map<String, dynamic> request) {
    final state = request['state'];
    return state == null || state is Map && state['status'] == 'pending';
  }
}
