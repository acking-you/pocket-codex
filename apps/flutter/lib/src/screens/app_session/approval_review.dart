import 'dart:convert';

/// Display-only interpretation of a Guardian request. Never authorizes a tool.
class ApprovalReviewRequest {
  const ApprovalReviewRequest({
    required this.tool,
    required this.summary,
    required this.details,
    this.cwd,
  });

  final String tool;
  final String summary;
  final String details;
  final String? cwd;

  /// Require Codex's explicit envelope, not arbitrary JSON in a user message.
  static ApprovalReviewRequest? parse(String text) {
    const start = '>>> APPROVAL REQUEST START';
    const end = '>>> APPROVAL REQUEST END';
    final stop = text.lastIndexOf(end);
    if (stop < 0 || text.substring(stop + end.length).trim().isNotEmpty) {
      return null;
    }
    final begin = text.lastIndexOf(start, stop);
    if (begin < 0) return null;
    final envelope = text.substring(begin + start.length, stop);
    final label = RegExp(
      r'(?:Planned action|Network access) JSON:\s*',
    ).firstMatch(envelope);
    if (label == null) return null;
    final value = _object(envelope.substring(label.end));
    if (value == null) return null;
    final tool = value['tool'] is String ? value['tool'] as String : null;
    if (tool == null || tool.isEmpty) return null;
    final command =
        value['command'] ?? value['cmd'] ?? value['argv'] ?? value['input'];
    final detail = command is List
        ? command.whereType<String>().join('\n')
        : command is String
        ? command
        : const JsonEncoder.withIndent('  ').convert(value);
    final reason = value['justification'];
    final summary = reason is String && reason.trim().isNotEmpty
        ? reason.trim()
        : detail.trim();
    return ApprovalReviewRequest(
      tool: tool,
      summary: summary,
      details: detail,
      cwd: value['cwd'] is String ? value['cwd'] as String : null,
    );
  }
}

/// A structured model assessment, kept distinct from a pending approval action.
class ApprovalReviewResult {
  const ApprovalReviewResult({
    required this.outcome,
    required this.risk,
    required this.authorization,
    required this.rationale,
  });
  final String outcome;
  final String risk;
  final String authorization;
  final String rationale;

  /// Full schema only: ordinary prose and unrelated JSON keep their renderer.
  static ApprovalReviewResult? parse(String text) {
    var raw = text.trim();
    if (raw.startsWith('```json\n') && raw.endsWith('```')) {
      raw = raw.substring(8, raw.length - 3).trim();
    }
    if (!raw.startsWith('{') || !raw.endsWith('}')) return null;
    final value = _object(raw);
    if (value == null ||
        !const {'allow', 'deny'}.contains(value['outcome']) ||
        !const {
          'low',
          'medium',
          'high',
          'critical',
        }.contains(value['risk_level']) ||
        !const {
          'unknown',
          'low',
          'medium',
          'high',
        }.contains(value['user_authorization']) ||
        value['rationale'] is! String) {
      return null;
    }
    return ApprovalReviewResult(
      outcome: value['outcome'] as String,
      risk: value['risk_level'] as String,
      authorization: value['user_authorization'] as String,
      rationale: value['rationale'] as String,
    );
  }
}

Map<String, dynamic>? _object(String raw) {
  try {
    final value = jsonDecode(raw);
    return value is Map<String, dynamic> ? value : null;
  } on FormatException {
    return null;
  }
}

/// Native approval lifecycle, kept separate from actionable user approvals.
class AutoApprovalReview {
  const AutoApprovalReview(this.status, this.request, this.result);
  final String status;
  final ApprovalReviewRequest? request;
  final ApprovalReviewResult? result;

  static AutoApprovalReview? parse(String raw) {
    final value = _object(raw);
    final review = value?['review'];
    if (value?['reviewId'] is! String || review is! Map) return null;
    final status = review['status'];
    if (!const {
      'inProgress',
      'approved',
      'denied',
      'timedOut',
      'aborted',
    }.contains(status)) {
      return null;
    }
    final action = value?['action'];
    ApprovalReviewRequest? request;
    if (action is Map<String, dynamic>) {
      final type = action['type'] is String ? action['type'] as String : '';
      final summary =
          action['command'] ??
          action['reason'] ??
          action['host'] ??
          action['toolName'] ??
          action['tool_name'];
      request = ApprovalReviewRequest(
        tool: type,
        summary: summary is String
            ? summary
            : const JsonEncoder.withIndent('  ').convert(action),
        details: const JsonEncoder.withIndent('  ').convert(action),
        cwd: action['cwd'] is String ? action['cwd'] as String : null,
      );
    }
    return AutoApprovalReview(
      status as String,
      request,
      status == 'inProgress'
          ? null
          : ApprovalReviewResult(
              outcome: status == 'approved' ? 'allow' : 'deny',
              risk: review['riskLevel'] is String
                  ? review['riskLevel'] as String
                  : 'unknown',
              authorization: review['userAuthorization'] is String
                  ? review['userAuthorization'] as String
                  : 'unknown',
              rationale: review['rationale'] is String
                  ? review['rationale'] as String
                  : '',
            ),
    );
  }
}
