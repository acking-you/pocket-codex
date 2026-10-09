part of '../app_session_screen.dart';

// Drafts live for this ProviderScope, including navigation away from the screen.
// Hosts have separate stores; new conversations are also scoped by project.
final _composerDraftsProvider =
    ChangeNotifierProvider.family<_ComposerDrafts, String>(
      (ref, service) => _ComposerDrafts(),
    );

class _ComposerDraft {
  _ComposerDraft(this.threadId, this.project);

  String? threadId;
  String? project;
  bool sendPending = false;
  (String?, String?) get key => (threadId, threadId == null ? project : null);
  TextEditingValue value = TextEditingValue.empty;
  final List<_Attachment> attachments = [];
  final List<_Queued> queue = [];

  bool get hasDraft => value.text.isNotEmpty || attachments.isNotEmpty;
}

class _ComposerDrafts extends ChangeNotifier {
  final Map<(String?, String?), _ComposerDraft> _drafts = {};
  final Set<(String?, String?)> _badges = {};
  int nextAttachmentId = 0;
  int nextQueueId = 0;
  bool _disposed = false;

  // Keep even empty drafts stable while a picker or clipboard read is pending.
  _ComposerDraft forThread(String? id, {String? cwd}) {
    final project = cwd == null || cwd.trim().isEmpty ? null : cwd;
    final key = (id, id == null ? project : null);
    return _drafts.putIfAbsent(key, () => _ComposerDraft(id, project));
  }

  // Default discovery must keep pending pickers attached to the same draft.
  // An existing project draft wins; leave the unassigned draft reachable.
  _ComposerDraft? resolveProject(_ComposerDraft draft, String project) {
    final target = _drafts[(null, project)];
    if (target != null && !identical(target, draft)) {
      return draft.hasDraft || draft.queue.isNotEmpty ? null : target;
    }
    _drafts.remove(draft.key);
    _badges.remove(draft.key);
    draft.project = project;
    _drafts[draft.key] = draft;
    if (draft.hasDraft) _badges.add(draft.key);
    return draft;
  }

  Iterable<String> get projects => _drafts.keys
      .where((key) => key.$1 == null && key.$2 != null)
      .map((key) => key.$2!);

  bool hasDraft(String id) => _badges.contains((id, null));

  int queuedCount(String id) => _drafts[(id, null)]?.queue.length ?? 0;

  void save(_ComposerDraft draft, {bool changed = false}) {
    if (_disposed) return;
    final id = draft.key;
    _drafts[draft.key] = draft;
    final badgeChanged = draft.hasDraft ? _badges.add(id) : _badges.remove(id);
    // Keystrokes update the value without rebuilding the transcript.
    if (changed || badgeChanged) notifyListeners();
  }

  void adoptThread(_ComposerDraft draft, String id) {
    _drafts.remove(draft.key);
    _badges.remove(draft.key);
    draft.threadId = id;
    save(draft, changed: true);
  }

  void attachmentChanged(_Attachment attachment) {
    if (_disposed) return;
    if (containsAttachment(attachment)) {
      notifyListeners();
    }
  }

  bool containsAttachment(_Attachment attachment) =>
      !_disposed &&
      _drafts.values.any((d) => d.attachments.contains(attachment));

  @override
  void dispose() {
    _disposed = true;
    _drafts.clear();
    super.dispose();
  }
}
