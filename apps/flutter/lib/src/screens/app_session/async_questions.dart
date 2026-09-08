import 'dart:convert';

import 'package:pocket_codex/src/bridge_api.dart';

/// Nonblocking questions delivered as an agent message, answered with user text.
class AsyncQuestionPrompt {
  AsyncQuestionPrompt(this.id, this.questions);

  final String id;
  final List<({String title, List<String> options})> questions;

  static AsyncQuestionPrompt? fromEvent(AppEvent event) {
    try {
      final params = jsonDecode(event.raw);
      if (params is! Map || params['item'] is! Map) return null;
      return parse(event.itemId ?? '', jsonEncode(params['item']['questions']));
    } catch (_) {
      return null;
    }
  }

  static AsyncQuestionPrompt? parse(String id, String? raw) {
    if (id.isEmpty || raw == null) return null;
    try {
      final value = jsonDecode(raw);
      if (value is! List) return null;
      final questions = <({String title, List<String> options})>[];
      for (final question in value) {
        if (question is! Map || question['title'] is! String) continue;
        final title = (question['title'] as String).trim();
        if (title.isEmpty) continue;
        final options = question['options'];
        questions.add((
          title: title,
          options: options is List ? options.whereType<String>().toList() : [],
        ));
      }
      return questions.isEmpty ? null : AsyncQuestionPrompt(id, questions);
    } catch (_) {
      return null;
    }
  }

  /// Reuse the question editor without inventing a server request id.
  AppEvent get cardEvent => AppEvent(
    kind: 'agentMessage/questions',
    itemId: id,
    raw: jsonEncode({
      'questions': [
        for (var index = 0; index < questions.length; index++)
          {
            'id': '$index',
            'question': questions[index].title,
            'isOther': true,
            'options': [
              for (final option in questions[index].options) {'label': option},
            ],
          },
      ],
    }),
  );

  String answerText(Map<String, List<String>> answers) => [
    for (var index = 0; index < questions.length; index++)
      if (answers['$index'] case final answer?)
        '${questions[index].title}\n${answer.join('\n')}',
  ].join('\n\n');
}
