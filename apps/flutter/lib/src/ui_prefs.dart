import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pocket_codex/src/bridge_api.dart' show AcpAgentSpec;

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
    this.autoHostOpenCode,
    this.acpAgents = const [],
    this.autoHostAcp = const [],
    this.guideSeen = false,
    this.themeMode,
    this.composerHeight,
    this.sidebarOpen,
    this.sidebarWidth,
  });

  /// Custom ACP agents the user saved (program + arguments, in plain text).
  final List<AcpAgentSpec> acpAgents;

  /// The ACP agents the user left hosted, restored on a desktop cold start.
  final List<AutoHostAcpPrefs> autoHostAcp;

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

  /// The OpenCode hosting the user left running, or null when it was stopped
  /// (or never started). Restored alongside [autoHost] on a desktop cold start.
  final AutoHostOpenCodePrefs? autoHostOpenCode;

  /// Whether the first-run welcome guide has been shown on this device. Set
  /// the first time it renders, so signing in ever again skips it.
  final bool guideSeen;

  /// Explicit theme choice: `'light'` / `'dark'`, or null to follow the
  /// system. A string (not the enum) so the on-disk JSON stays readable and
  /// index-shift-proof.
  final String? themeMode;

  /// Preferred minimum input height; null uses the compact default.
  /// Text can grow the input beyond this height without changing the preference.
  final double? composerHeight;

  /// Whether the desktop sidebar was left open; null means the default (open).
  final bool? sidebarOpen;

  /// The desktop sidebar width the user dragged to; null uses the default.
  final double? sidebarWidth;

  /// Copy with the given fields replaced. `clearAutoHost` removes the
  /// auto-host record, `clearAutoHostOpenCode` the OpenCode one; `clearThemeMode` returns to follow-system (a plain
  /// null argument means "keep").
  UiPrefs copyWith({
    String? preferredAppServiceKey,
    String? lastServiceKey,
    Map<String, String>? lastThreadByService,
    AutoHostPrefs? autoHost,
    bool clearAutoHost = false,
    AutoHostOpenCodePrefs? autoHostOpenCode,
    bool clearAutoHostOpenCode = false,
    List<AcpAgentSpec>? acpAgents,
    List<AutoHostAcpPrefs>? autoHostAcp,
    bool? guideSeen,
    String? themeMode,
    bool clearThemeMode = false,
    double? composerHeight,
    bool? sidebarOpen,
    double? sidebarWidth,
  }) => UiPrefs(
    preferredAppServiceKey:
        preferredAppServiceKey ?? this.preferredAppServiceKey,
    lastServiceKey: lastServiceKey ?? this.lastServiceKey,
    lastThreadByService: lastThreadByService ?? this.lastThreadByService,
    autoHost: clearAutoHost ? null : (autoHost ?? this.autoHost),
    autoHostOpenCode: clearAutoHostOpenCode
        ? null
        : (autoHostOpenCode ?? this.autoHostOpenCode),
    acpAgents: acpAgents ?? this.acpAgents,
    autoHostAcp: autoHostAcp ?? this.autoHostAcp,
    guideSeen: guideSeen ?? this.guideSeen,
    themeMode: clearThemeMode ? null : (themeMode ?? this.themeMode),
    composerHeight: composerHeight ?? this.composerHeight,
    sidebarOpen: sidebarOpen ?? this.sidebarOpen,
    sidebarWidth: sidebarWidth ?? this.sidebarWidth,
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
    final rawOpenCode = json['autoHostOpenCode'];
    final rawAgents = json['acpAgents'];
    final rawAcpHosts = json['autoHostAcp'];
    return UiPrefs(
      acpAgents: [
        if (rawAgents is List)
          for (final agent in rawAgents.take(50)) ?AcpAgentSpec.fromJson(agent),
      ],
      autoHostAcp: [
        if (rawAcpHosts is List)
          for (final host in rawAcpHosts.take(16))
            ?AutoHostAcpPrefs.fromJson(host),
      ],
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
      autoHostOpenCode: rawOpenCode is Map<String, dynamic>
          ? AutoHostOpenCodePrefs.fromJson(rawOpenCode)
          : null,
      guideSeen: json['guideSeen'] == true,
      themeMode: json['themeMode'] == 'light' || json['themeMode'] == 'dark'
          ? json['themeMode'] as String
          : null,
      composerHeight: switch (json['composerHeight']) {
        num value when value.isFinite => value.toDouble().clamp(28, 280),
        _ => null,
      },
      sidebarOpen: json['sidebarOpen'] is bool
          ? json['sidebarOpen'] as bool
          : null,
      sidebarWidth: switch (json['sidebarWidth']) {
        num value when value.isFinite => value.toDouble().clamp(200, 520),
        _ => null,
      },
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
    if (autoHostOpenCode != null)
      'autoHostOpenCode': autoHostOpenCode!.toJson(),
    if (acpAgents.isNotEmpty)
      'acpAgents': [for (final agent in acpAgents) agent.toJson()],
    if (autoHostAcp.isNotEmpty)
      'autoHostAcp': [for (final host in autoHostAcp) host.toJson()],
    if (guideSeen) 'guideSeen': true,
    if (themeMode != null) 'themeMode': themeMode,
    if (composerHeight != null) 'composerHeight': composerHeight,
    if (sidebarOpen != null) 'sidebarOpen': sidebarOpen,
    if (sidebarWidth != null) 'sidebarWidth': sidebarWidth,
  };
}

/// The `appServeStart` parameters of the last hosting the user started, so a
/// cold start can restore external hosting.
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

  /// Legacy field retained for old records; restored hosts use external Codex.
  final bool embedded;

  /// Explicit codex binary path, when the user customized it.
  final String? binaryOverride;

  /// Parse from JSON; defaults on shape surprises.
  factory AutoHostPrefs.fromJson(Map<String, dynamic> json) => AutoHostPrefs(
    port: json['port'] is int ? json['port'] as int : 0,
    name: json['name'] is String ? json['name'] as String : 'default',
    proxy: json['proxy'] is String ? json['proxy'] as String : null,
    embedded: false,
    binaryOverride: json['binaryOverride'] is String
        ? json['binaryOverride'] as String
        : null,
  );

  /// JSON for persistence.
  Map<String, dynamic> toJson() => {
    'port': port,
    'name': name,
    if (proxy != null) 'proxy': proxy,
    'embedded': false,
    if (binaryOverride != null) 'binaryOverride': binaryOverride,
  };
}

/// The `appServeStartOpencode` parameters of the last OpenCode hosting the user
/// started, so a cold start can re-attach to the local OpenCode service.
class AutoHostOpenCodePrefs {
  /// Creates an OpenCode auto-host record.
  const AutoHostOpenCodePrefs({required this.name, this.binaryOverride});

  /// Instance name.
  final String name;

  /// Explicit opencode binary path, when the user customized it.
  final String? binaryOverride;

  /// Parse from JSON; defaults on shape surprises.
  factory AutoHostOpenCodePrefs.fromJson(Map<String, dynamic> json) =>
      AutoHostOpenCodePrefs(
        name: json['name'] is String && (json['name'] as String).isNotEmpty
            ? json['name'] as String
            : 'opencode',
        binaryOverride: json['binaryOverride'] is String
            ? json['binaryOverride'] as String
            : null,
      );

  /// JSON for persistence.
  Map<String, dynamic> toJson() => {
    'name': name,
    if (binaryOverride != null) 'binaryOverride': binaryOverride,
  };
}

/// An ACP agent the user left hosted under [name], restored on cold start.
class AutoHostAcpPrefs {
  /// Creates an ACP auto-host record.
  const AutoHostAcpPrefs({required this.name, required this.spec});

  /// Instance name.
  final String name;

  /// What was hosted.
  final AcpAgentSpec spec;

  /// From persisted JSON; `null` when malformed.
  static AutoHostAcpPrefs? fromJson(Object? json) {
    if (json is! Map) return null;
    final name = json['name'];
    final spec = AcpAgentSpec.fromJson(json['spec']);
    if (name is! String || name.isEmpty || spec == null) return null;
    return AutoHostAcpPrefs(name: name, spec: spec);
  }

  /// JSON for persistence.
  Map<String, dynamic> toJson() => {'name': name, 'spec': spec.toJson()};
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
        autoHostOpenCode: raced.autoHostOpenCode ?? loaded.autoHostOpenCode,
        acpAgents: raced.acpAgents.isEmpty ? loaded.acpAgents : raced.acpAgents,
        autoHostAcp: raced.autoHostAcp.isEmpty
            ? loaded.autoHostAcp
            : raced.autoHostAcp,
        // Only ever flips false→true, so OR-merging is lossless.
        guideSeen: raced.guideSeen || loaded.guideSeen,
        themeMode: raced.themeMode ?? loaded.themeMode,
        composerHeight: raced.composerHeight ?? loaded.composerHeight,
        sidebarOpen: raced.sidebarOpen ?? loaded.sidebarOpen,
        sidebarWidth: raced.sidebarWidth ?? loaded.sidebarWidth,
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

  /// Persist the preferred input height within the supported range.
  void setComposerHeight(double height) {
    if (!height.isFinite) return;
    final next = _current.copyWith(composerHeight: height.clamp(28, 280));
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Remember whether the desktop sidebar is open.
  void setSidebarOpen(bool open) {
    if (_current.sidebarOpen == open) return;
    final next = _current.copyWith(sidebarOpen: open);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Persist the desktop sidebar width within the supported range.
  void setSidebarWidth(double width) {
    if (!width.isFinite) return;
    final next = _current.copyWith(sidebarWidth: width.clamp(200, 520));
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

  /// Remember the OpenCode hosting the user just started.
  void setAutoHostOpenCode(AutoHostOpenCodePrefs host) {
    final next = _current.copyWith(autoHostOpenCode: host);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Forget the OpenCode auto-host record (stopped on purpose).
  void clearAutoHostOpenCode() {
    if (_current.autoHostOpenCode == null) return;
    final next = _current.copyWith(clearAutoHostOpenCode: true);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Save (or replace, by profile id) a custom ACP agent.
  void saveAcpAgent(AcpAgentSpec agent) {
    final agents = [
      for (final saved in _current.acpAgents)
        if (saved.profileId != agent.profileId) saved,
      agent,
    ];
    final next = _current.copyWith(acpAgents: agents);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Forget a saved custom ACP agent.
  void removeAcpAgent(String profileId) {
    final agents = [
      for (final saved in _current.acpAgents)
        if (saved.profileId != profileId) saved,
    ];
    if (agents.length == _current.acpAgents.length) return;
    final next = _current.copyWith(acpAgents: agents);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Remember an ACP agent the user just hosted under `host.name`.
  void setAutoHostAcp(AutoHostAcpPrefs host) {
    final hosts = [
      for (final saved in _current.autoHostAcp)
        if (saved.name != host.name) saved,
      host,
    ];
    final next = _current.copyWith(autoHostAcp: hosts);
    state = AsyncData(next);
    _enqueueWrite(next);
  }

  /// Forget the ACP auto-host record of [name] (stopped on purpose).
  void clearAutoHostAcp(String name) {
    final hosts = [
      for (final saved in _current.autoHostAcp)
        if (saved.name != name) saved,
    ];
    if (hosts.length == _current.autoHostAcp.length) return;
    final next = _current.copyWith(autoHostAcp: hosts);
    state = AsyncData(next);
    _enqueueWrite(next);
  }
}

/// The durable UI prefs. `loading` until the file is read; consumers treat a
/// missing value as defaults.
final uiPrefsProvider = AsyncNotifierProvider<UiPrefsStore, UiPrefs>(
  UiPrefsStore.new,
);
