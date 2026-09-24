import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/screens/app_session/async_questions.dart';

void main() {
  test('historical answers hide transport ids and preserve all answers', () {
    const raw =
        '<send_user_message_question_reply>\n'
        '[{"question":"平台？","answer":"安卓","questionItemId":"opaque"},'
        '{"question":"安装类型？","answer":"首次安装"}]\n'
        '</send_user_message_question_reply>';
    expect(displayAsyncQuestionReply(raw), '平台？\n安卓\n\n安装类型？\n首次安装');
  });
  test('ordinary messages and incomplete envelopes remain verbatim', () {
    for (final raw in [
      'Keep <send_user_message_question_reply> as documentation',
      '<send_user_message_question_reply>[]</send_user_message_question_reply>',
      '<send_user_message_question_reply>[{"answer":4}]</send_user_message_question_reply>',
      '<send_user_message_question_reply>broken</send_user_message_question_reply>',
    ]) {
      expect(displayAsyncQuestionReply(raw), raw);
    }
  });
}
