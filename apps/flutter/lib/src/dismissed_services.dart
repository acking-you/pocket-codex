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
/// Reachable/foreign services are never dismissed here: those are legitimately
/// live, so hiding them would strand a working service off the list. And a
/// dismissed key is pruned once discovery confirms it truly ABSENT (see
/// [restore]), so a later fresh registration under the same key shows again.
class DismissedServices extends AsyncNotifier<Set<String>> {
  File? _cachedFile;

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

  // Persistence is fire-and-forget so the user-facing action never blocks on
  // disk I/O. Reads `state` at write time (not a snapshot), so if two writes
  // race the file still ends up matching the latest in-memory set.
  Future<void> _persist() async {
    try {
      final file = await _file();
      final keys = state.valueOrNull ?? const <String>{};
      await file.writeAsString(jsonEncode(keys.toList()));
    } catch (_) {
      // Best-effort: an unwritable support dir just means the dismissal doesn't
      // outlive this run, not a crash.
    }
  }

  /// Durably hide [key] from the service lists. No-op if already dismissed.
  Future<void> dismiss(String key) async {
    final current = state.valueOrNull ?? const <String>{};
    if (current.contains(key)) return;
    state = AsyncData({...current, key});
    unawaited(_persist());
  }

  /// Stop hiding [keys] — they're gone from discovery, so a fresh registration
  /// under the same key should be visible again. No-op for keys not dismissed.
  Future<void> restore(Iterable<String> keys) async {
    final current = state.valueOrNull;
    if (current == null || current.isEmpty) return;
    final next = current.difference(keys.toSet());
    if (next.length == current.length) return;
    state = AsyncData(next);
    unawaited(_persist());
  }
}

/// The durable dismissed-service-keys set. `loading` until the file is read;
/// consumers treat a missing value as the empty set.
final dismissedServicesProvider =
    AsyncNotifierProvider<DismissedServices, Set<String>>(
      DismissedServices.new,
    );
