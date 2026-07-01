import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

/// Durable set of service keys the user has explicitly 注销'd that we CANNOT
/// force off the relay — an unreachable, orphaned/hollow registration.
///
/// The relay only drops a key when the register connection holding it closes,
/// and there is no subscriber-side force-drop: a still-running host just
/// re-registers, and an orphan with no live local holder can't be cancelled by
/// the backend (`deregister_key` needs a live in-memory session). So for such a
/// dead entry the honest action is to hide it from THIS device's list durably,
/// which is what this store provides. Backed by a small JSON file in the
/// app-support dir so the dismissal survives an app restart (an in-memory hide
/// would let the stale entry reappear — the user's original complaint).
///
/// This notifier is pure storage: it does NOT know about discovery or
/// reachability. The un-hide POLICY lives at the call site (the services list),
/// which [restore]s a dismissed key once it is REACHABLE again — the correct
/// signal, because a hollow orphan stays relay-registered the whole time, so
/// keying un-hide on discovery-absence would strand a recovered service forever.
/// Reachable/foreign services are therefore never left dismissed once they come
/// back to life, and a live service is never stranded off the list.
class DismissedServices extends AsyncNotifier<Set<String>> {
  File? _cachedFile;

  /// False until the initial file load has landed. While false, mutations do
  /// NOT write to disk — a write then would race the in-flight read AND persist
  /// a partial set (state hasn't merged the file yet), silently clobbering
  /// already-persisted keys on the next restart. [build] persists the
  /// reconciled union once the load completes, so a mutation that raced the
  /// load still reaches disk.
  bool _loaded = false;

  /// Serial write chain: each persist runs after the previous completes, so
  /// concurrent dismiss/restore calls can't interleave (corrupting the JSON) or
  /// land out of order (leaving stale content). Each link carries its own
  /// snapshot, so the last enqueued — the latest state — is written last.
  Future<void> _writes = Future<void>.value();

  Future<File> _file() async {
    final cached = _cachedFile;
    if (cached != null) return cached;
    final dir = await getApplicationSupportDirectory();
    final handle = File('${dir.path}/dismissed_services.json');
    _cachedFile = handle;
    return handle;
  }

  @override
  Future<Set<String>> build() async {
    final loaded = await _load();
    // Union in any dismissal that landed WHILE the file was loading — it set
    // state to AsyncData before this returned, so merge it rather than let this
    // return silently clobber it (the init-race the review flagged). Its write
    // was deferred (see [_loaded]), so persist the reconciled set here — else
    // the raced dismissal (or the file's prior keys) would be lost on restart.
    final union = {...loaded, ...?state.valueOrNull};
    _loaded = true;
    if (union.length != loaded.length) _enqueueWrite(union);
    return union;
  }

  Future<Set<String>> _load() async {
    try {
      final file = await _file();
      if (!await file.exists()) return <String>{};
      final raw = await file.readAsString();
      if (raw.trim().isEmpty) return <String>{};
      final decoded = jsonDecode(raw);
      // Tolerate a legacy/corrupt shape: only a JSON array of strings is valid.
      if (decoded is List) return decoded.whereType<String>().toSet();
      return <String>{};
    } catch (_) {
      // A corrupt or unreadable file must never block the services list.
      return <String>{};
    }
  }

  /// Persist [keys] on the serial write chain. Fire-and-forget (the caller
  /// never blocks on disk I/O); best-effort (an unwritable dir just means the
  /// dismissal doesn't outlive this run, not a crash). Deferred while the
  /// initial load is still in flight (see [_loaded]); [build] persists the
  /// reconciled set instead.
  void _enqueueWrite(Set<String> keys) {
    if (!_loaded) return;
    _writes = _writes.then((_) async {
      try {
        final file = await _file();
        await file.writeAsString(jsonEncode(keys.toList()));
      } catch (_) {
        // swallow — see doc above.
      }
    });
  }

  /// Durably hide [key] from the service lists. No-op if already dismissed. A
  /// dismissal that races the initial load is preserved by build()'s union, so
  /// it isn't clobbered when the load lands.
  void dismiss(String key) {
    final current = state.valueOrNull ?? const <String>{};
    if (current.contains(key)) return;
    final next = {...current, key};
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Stop hiding [keys] — the service is reachable again (recovered in place, or
  /// a fresh reachable registration under the same key), so it should show. No-op
  /// for keys not currently dismissed.
  void restore(Iterable<String> keys) {
    final current = state.valueOrNull ?? const <String>{};
    if (current.isEmpty) return;
    final next = current.difference(keys.toSet());
    if (next.length == current.length) return;
    state = AsyncData(next);
    _enqueueWrite(next);
  }
}

/// The durable dismissed-service-keys set. `loading` until the file is read;
/// consumers treat a missing value as the empty set.
final dismissedServicesProvider =
    AsyncNotifierProvider<DismissedServices, Set<String>>(
      DismissedServices.new,
    );
