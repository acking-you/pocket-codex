part of '../app_session_screen.dart';

// Drafts live for this ProviderScope, including navigation away from the screen.
// Hosts have separate stores; the null thread is that host's new conversation.
final _composerDraftsProvider =
    ChangeNotifierProvider.family<_ComposerDrafts, String>(
      (ref, service) => _ComposerDrafts(),
    );

class _ComposerDraft {
  _ComposerDraft(this.threadId);

  String? threadId;
  TextEditingValue value = TextEditingValue.empty;
  final List<_Attachment> attachments = [];
  final List<_Queued> queue = [];

  bool get hasDraft => value.text.isNotEmpty || attachments.isNotEmpty;
}

class _ComposerDrafts extends ChangeNotifier {
  final Map<String?, _ComposerDraft> _drafts = {};
  final Set<String?> _badges = {};
  int nextAttachmentId = 0;
  int nextQueueId = 0;
  bool _disposed = false;

  // Keep even empty drafts stable while a picker or clipboard read is pending.
  _ComposerDraft forThread(String? id) =>
      _drafts.putIfAbsent(id, () => _ComposerDraft(id));

  bool hasDraft(String id) => _badges.contains(id);

  int queuedCount(String id) => _drafts[id]?.queue.length ?? 0;

  void save(_ComposerDraft draft, {bool changed = false}) {
    if (_disposed) return;
    final id = draft.threadId;
    _drafts[id] = draft;
    final badgeChanged = draft.hasDraft ? _badges.add(id) : _badges.remove(id);
    // Keystrokes update the value without rebuilding the transcript.
    if (changed || badgeChanged) notifyListeners();
  }

  void adoptThread(_ComposerDraft draft, String id) {
    _drafts.remove(draft.threadId);
    _badges.remove(draft.threadId);
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
