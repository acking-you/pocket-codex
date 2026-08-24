import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';

/// Durable, device-local UI preferences backing the chat-first home screen:
/// which app service the user prefers and last talked to, the last-open thread
/// per service, and the parameters of the local host the user left running (so
/// a desktop restart can bring hosting back up without a trip through the
/// manage page).
///
/// Small JSON file in the app-support dir (same pattern as
/// `dismissed_services.json`): survives restarts, never blocks the UI, and a
/// corrupt/unreadable file degrades to defaults rather than crashing.
class UiPrefs {
  /// Creates a prefs snapshot.
  const UiPrefs({
    this.preferredAppServiceKey,
    this.lastServiceKey,
    this.lastThreadByService = const {},
    this.autoHost,
    this.guideSeen = false,
    this.themeMode,
  });

  /// Full relay key of the app service explicitly chosen as the default host.
  final String? preferredAppServiceKey;

  /// Full relay key of the app service the user last chatted on.
  final String? lastServiceKey;

  /// Last-open thread id, keyed by service key.
  final Map<String, String> lastThreadByService;

  /// Parameters of the hosting the user left running, or null when the user
  /// stopped hosting (or never hosted). Used to auto-restore hosting on a
  /// desktop cold start.
  final AutoHostPrefs? autoHost;

  /// Whether the first-run welcome guide has been shown on this device. Set
  /// the first time it renders, so signing in ever again skips it.
  final bool guideSeen;

  /// Explicit theme choice: `'light'` / `'dark'`, or null to follow the
  /// system. A string (not the enum) so the on-disk JSON stays readable and
  /// index-shift-proof.
  final String? themeMode;

  /// Copy with the given fields replaced. `clearAutoHost` removes the
  /// auto-host record; `clearThemeMode` returns to follow-system (a plain
  /// null argument means "keep").
  UiPrefs copyWith({
    String? preferredAppServiceKey,
    String? lastServiceKey,
    Map<String, String>? lastThreadByService,
    AutoHostPrefs? autoHost,
    bool clearAutoHost = false,
    bool? guideSeen,
    String? themeMode,
    bool clearThemeMode = false,
  }) => UiPrefs(
    preferredAppServiceKey:
        preferredAppServiceKey ?? this.preferredAppServiceKey,
    lastServiceKey: lastServiceKey ?? this.lastServiceKey,
    lastThreadByService: lastThreadByService ?? this.lastThreadByService,
    autoHost: clearAutoHost ? null : (autoHost ?? this.autoHost),
    guideSeen: guideSeen ?? this.guideSeen,
    themeMode: clearThemeMode ? null : (themeMode ?? this.themeMode),
  );

  /// Parse from JSON; any shape surprise degrades to defaults.
  factory UiPrefs.fromJson(Map<String, dynamic> json) {
    final threads = <String, String>{};
    final rawThreads = json['lastThreadByService'];
    if (rawThreads is Map) {
      rawThreads.forEach((k, v) {
        if (k is String && v is String) threads[k] = v;
      });
    }
    final rawHost = json['autoHost'];
    return UiPrefs(
      preferredAppServiceKey: json['preferredAppServiceKey'] is String
          ? json['preferredAppServiceKey'] as String
          : null,
      lastServiceKey: json['lastServiceKey'] is String
          ? json['lastServiceKey'] as String
          : null,
      lastThreadByService: threads,
      autoHost: rawHost is Map<String, dynamic>
          ? AutoHostPrefs.fromJson(rawHost)
          : null,
      guideSeen: json['guideSeen'] == true,
      themeMode: json['themeMode'] == 'light' || json['themeMode'] == 'dark'
          ? json['themeMode'] as String
          : null,
    );
  }

  /// JSON for persistence.
  Map<String, dynamic> toJson() => {
    if (preferredAppServiceKey != null)
      'preferredAppServiceKey': preferredAppServiceKey,
    if (lastServiceKey != null) 'lastServiceKey': lastServiceKey,
    if (lastThreadByService.isNotEmpty)
      'lastThreadByService': lastThreadByService,
    if (autoHost != null) 'autoHost': autoHost!.toJson(),
    if (guideSeen) 'guideSeen': true,
    if (themeMode != null) 'themeMode': themeMode,
  };
}

/// The `appServeStart` parameters of the last hosting the user started, so a
/// cold start can re-run it verbatim.
class AutoHostPrefs {
  /// Creates an auto-host record.
  const AutoHostPrefs({
    required this.port,
    required this.name,
    this.proxy,
    this.embedded = false,
    this.binaryOverride,
  });

  /// Listen port passed to `appServeStart` (0 = ephemeral).
  final int port;

  /// Instance name.
  final String name;

  /// Upstream proxy URL, or null when the user turned the proxy off.
  final String? proxy;

  /// Whether the in-process (built-in) codex was used.
  final bool embedded;

  /// Explicit codex binary path, when the user customized it.
  final String? binaryOverride;

  /// Parse from JSON; defaults on shape surprises.
  factory AutoHostPrefs.fromJson(Map<String, dynamic> json) => AutoHostPrefs(
    port: json['port'] is int ? json['port'] as int : 0,
    name: json['name'] is String ? json['name'] as String : 'default',
    proxy: json['proxy'] is String ? json['proxy'] as String : null,
    embedded: json['embedded'] == true,
    binaryOverride: json['binaryOverride'] is String
        ? json['binaryOverride'] as String
        : null,
  );

  /// JSON for persistence.
  Map<String, dynamic> toJson() => {
    'port': port,
    'name': name,
    if (proxy != null) 'proxy': proxy,
    'embedded': embedded,
    if (binaryOverride != null) 'binaryOverride': binaryOverride,
  };
}

/// Store notifier: load-once, serial best-effort writes (mirrors the
/// robustness contract of [DismissedServices]).
class UiPrefsStore extends AsyncNotifier<UiPrefs> {
  File? _cachedFile;
  bool _loaded = false;
  Future<void> _writes = Future<void>.value();

  Future<File> _file() async {
    final cached = _cachedFile;
    if (cached != null) return cached;
    final dir = await getApplicationSupportDirectory();
    final handle = File('${dir.path}/ui_state.json');
    _cachedFile = handle;
    return handle;
  }

  @override
  Future<UiPrefs> build() async {
    final loaded = await _load();
    // A mutation that raced the initial load computed its snapshot from EMPTY
    // defaults (state had no value yet), so adopting it wholesale would wipe
    // everything already on disk. Mutations during the race window only ever
    // SET fields (the clear-style ops early-return on a default snapshot), so
    // null/absent in the raced snapshot means "untouched" and a field-wise
    // merge onto the loaded file is lossless.
    final raced = state.valueOrNull;
    _loaded = true;
    if (raced != null) {
      final merged = UiPrefs(
        preferredAppServiceKey:
            raced.preferredAppServiceKey ?? loaded.preferredAppServiceKey,
        lastServiceKey: raced.lastServiceKey ?? loaded.lastServiceKey,
        lastThreadByService: {
          ...loaded.lastThreadByService,
          ...raced.lastThreadByService,
        },
        autoHost: raced.autoHost ?? loaded.autoHost,
        // Only ever flips false→true, so OR-merging is lossless.
        guideSeen: raced.guideSeen || loaded.guideSeen,
        themeMode: raced.themeMode ?? loaded.themeMode,
      );
      _enqueueWrite(merged);
      return merged;
    }
    return loaded;
  }

  Future<UiPrefs> _load() async {
    try {
      final file = await _file();
      if (!await file.exists()) return const UiPrefs();
      final raw = await file.readAsString();
      if (raw.trim().isEmpty) return const UiPrefs();
      final decoded = jsonDecode(raw);
      if (decoded is Map<String, dynamic>) return UiPrefs.fromJson(decoded);
      return const UiPrefs();
    } catch (_) {
      // Corrupt or unreadable prefs must never block the home screen.
      return const UiPrefs();
    }
  }

  void _enqueueWrite(UiPrefs prefs) {
    if (!_loaded) return;
    _writes = _writes.then((_) async {
      try {
        final file = await _file();
        await file.writeAsString(jsonEncode(prefs.toJson()));
      } catch (_) {
        // Best-effort: an unwritable dir just loses the pref, not the app.
      }
    });
  }

  UiPrefs get _current => state.valueOrNull ?? const UiPrefs();

  /// Choose the app service that chat-first home should try before all others.
  void setPreferredAppService(String serviceKey) {
    if (_current.preferredAppServiceKey == serviceKey) return;
    final next = _current.copyWith(preferredAppServiceKey: serviceKey);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Record the app service the user is chatting on.
  void setLastService(String serviceKey) {
    final next = _current.copyWith(lastServiceKey: serviceKey);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Record the last-open thread for [serviceKey] (null clears it, e.g. when
  /// the user starts a fresh conversation that has no id yet).
  void setLastThread(String serviceKey, String? threadId) {
    final threads = {..._current.lastThreadByService};
    if (threadId == null) {
      if (threads.remove(serviceKey) == null) return;
    } else {
      if (threads[serviceKey] == threadId) return;
      threads[serviceKey] = threadId;
    }
    final next = _current.copyWith(lastThreadByService: threads);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Record that the first-run welcome guide has been shown on this device.
  void markGuideSeen() {
    if (_current.guideSeen) return;
    final next = _current.copyWith(guideSeen: true);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Remember the hosting the user just started, for cold-start restore.
  void setAutoHost(AutoHostPrefs host) {
    final next = _current.copyWith(autoHost: host);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Set the theme: `'light'` / `'dark'`, or null to follow the system.
  void setThemeMode(String? mode) {
    if (_current.themeMode == mode) return;
    final next = mode == null
        ? _current.copyWith(clearThemeMode: true)
        : _current.copyWith(themeMode: mode);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Forget the auto-host record (the user stopped hosting on purpose).
  void clearAutoHost() {
    if (_current.autoHost == null) return;
    final next = _current.copyWith(clearAutoHost: true);
    state = AsyncData(next);
    _enqueueWrite(next);
  }
}

/// The durable UI prefs. `loading` until the file is read; consumers treat a
/// missing value as defaults.
final uiPrefsProvider = AsyncNotifierProvider<UiPrefsStore, UiPrefs>(
  UiPrefsStore.new,
);
