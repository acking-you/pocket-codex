part of '../../fake_bridge_api.dart';

/// One recorded remote-management mutation.
typedef ManagedCall = ({String op, String key, String name, Object? arg});

/// Scripted remote device management and project settings.
///
/// Remote computers live in [managedDevices] keyed by their `host:` service
/// key; mutations update them like the real host would (revisions,
/// incarnations, stop leaving the entry). [managedLoseNext] performs the next
/// call of an operation and then loses the response, as a dropped tunnel
/// would.
mixin _FakeManagement on _FakeBridgeState {
  DeviceMetadata localMetadata = const DeviceMetadata(
    os: 'macos',
    arch: 'aarch64',
    hostname: 'local',
    instanceId: 'local-process',
  );
  final Map<String, DeviceMetadata> serviceMetadata = {};
  final List<String> metadataReads = [];
  Object? metadataError;
  Completer<void>? metadataGate;

  @override
  DeviceMetadata localDeviceMetadata() => localMetadata;

  @override
  Future<DeviceMetadata?> serviceDeviceMetadata(String serviceKey) async {
    metadataReads.add(serviceKey);
    final value = serviceMetadata[serviceKey];
    if (metadataGate case final gate?) await gate.future;
    if (metadataError case final error?) throw error;
    return value;
  }

  /// Seeded project settings; absent keys derive from [metaProjectConfig] so
  /// tests overriding the legacy call keep working.
  final Map<String, ProjectSettings> projectSettings = {};

  /// Recorded project-settings saves.
  final List<({String key, List<String> roots, String? def, String? rev})>
  projectSettingsSaves = [];

  /// Thrown by the next [metaProjectSettings] (then cleared).
  Object? projectSettingsError;

  /// Holds [metaProjectSettings] answers until completed (late answers).
  Completer<void>? projectSettingsGate;

  /// Holds [metaSaveProjectSettings] answers until completed.
  Completer<void>? projectSaveGate;

  /// Refuses the next [metaSaveProjectSettings] with this error (then
  /// cleared), after recording it.
  Object? projectSaveError;

  /// Holds [deviceManagementSetEnabled] answers until completed.
  Completer<void>? deviceManagementGate;

  /// This computer's management switch.
  DeviceManagement deviceManagement = const DeviceManagement(
    supported: true,
    enabled: false,
    registered: false,
  );

  /// Values passed to [deviceManagementSetEnabled].
  final List<bool> deviceManagementSets = [];

  /// Authoritative restore overrides.
  List<HostRestoreOverride> restoreOverrides = [];

  /// Thrown by [hostRestoreOverrides] when set.
  Object? restoreOverridesError;

  /// Recorded [hostRestoreService] calls.
  final List<({String name, int? revision, HostRuntime? legacy})>
  restoreServiceCalls = [];

  /// Per-name failures of [hostRestoreService].
  final Map<String, Object> restoreServiceErrors = {};

  /// Answers of [hostRestoreProblem]; the `''` entry answers a null name.
  final Map<String, String> restoreProblems = {};

  /// Remote computers by management key.
  final Map<String, ManagedDevice> managedDevices = {};

  /// Thrown by the next [managedRuntimes] (then cleared).
  Object? managedRuntimesError;

  /// Remote runtime discovery by management key.
  final Map<String, List<ManagedRuntimeAvailability>> managedRuntimeLists = {};

  /// Progress answers for [managedClaudePrepare], consumed in order; the
  /// last one repeats.
  final List<AcpClaudeSetup> claudePrepareScript = [];

  /// Every mutation, in order.
  final List<ManagedCall> managedCalls = [];

  /// Perform the next call of an op, then lose its response.
  final Set<String> managedLoseNext = {};

  /// Holds every mutation until completed (pending-state tests).
  Completer<void>? managedGate;

  int _managedIncarnations = 0;

  @override
  Future<ProjectSettings> metaProjectSettings(String serviceKey) async {
    if (projectSettingsGate case final gate?) await gate.future;
    final error = projectSettingsError;
    if (error != null) {
      projectSettingsError = null;
      throw error;
    }
    final seeded = projectSettings[serviceKey];
    if (seeded != null) return seeded;
    final legacy = await metaProjectConfig(serviceKey);
    return ProjectSettings(
      projectRoots: legacy.projectRoots,
      defaultProject: legacy.defaultProject,
      effectiveDefault: legacy.defaultProject,
      exists: legacy.defaultProject != null,
    );
  }

  @override
  Future<ProjectSettings> metaSaveProjectSettings(
    String serviceKey, {
    required List<String> projectRoots,
    String? defaultProject,
    String? expectedRevision,
  }) async {
    projectSettingsSaves.add((
      key: serviceKey,
      roots: projectRoots,
      def: defaultProject,
      rev: expectedRevision,
    ));
    if (projectSaveGate case final gate?) await gate.future;
    if (projectSaveError case final error?) {
      projectSaveError = null;
      throw error;
    }
    final current = projectSettings[serviceKey];
    if (current != null &&
        !current.legacy &&
        current.revision != expectedRevision) {
      throw Exception(
        'project_changed (409): Another device changed the project folder. '
        'Reload before saving.',
      );
    }
    final next = ProjectSettings(
      projectRoots: projectRoots,
      defaultProject: defaultProject,
      revision: current?.revision == null
          ? null
          : '${int.parse(current!.revision!) + 1}',
      effectiveDefault: defaultProject ?? current?.effectiveDefault,
      automatic: defaultProject == null && (current?.automatic ?? false),
      exists: true,
    );
    projectSettings[serviceKey] = next;
    projectConfigs[serviceKey] = ProjectConfig(
      projectRoots: projectRoots,
      defaultProject: defaultProject,
    );
    return next;
  }

  @override
  Future<DeviceManagement> deviceManagementStatus() async => deviceManagement;

  @override
  Future<DeviceManagement> deviceManagementSetEnabled(bool enabled) async {
    deviceManagementSets.add(enabled);
    if (deviceManagementGate case final gate?) await gate.future;
    return deviceManagement = DeviceManagement(
      supported: deviceManagement.supported,
      enabled: enabled,
      registered: enabled,
      serviceKey: enabled ? 'pcx:local:host:local' : null,
    );
  }

  @override
  Future<List<HostRestoreOverride>> hostRestoreOverrides() async {
    final error = restoreOverridesError;
    if (error != null) throw error;
    return restoreOverrides;
  }

  @override
  Future<void> hostRestoreService(
    String name, {
    int? expectedRevision,
    HostRuntime? legacyRuntime,
  }) async {
    restoreServiceCalls.add((
      name: name,
      revision: expectedRevision,
      legacy: legacyRuntime,
    ));
    final error = restoreServiceErrors[name];
    if (error != null) throw error;
    final runtime =
        legacyRuntime ??
        restoreOverrides
            .where((o) => o.name == name && o.revision == expectedRevision)
            .firstOrNull
            ?.runtime;
    if (runtime == null) {
      throw Exception('host restore choice changed; automatic restore skipped');
    }
    // The bridge starts it like any local host would be started.
    switch (runtime.kind) {
      case 'codex':
        await appServeStart(
          port: runtime.port,
          binaryOverride: runtime.binary,
          name: name,
          proxy: runtime.proxy,
          embedded: false,
        );
      case 'acp':
        await appServeStartAcp(name: name, spec: runtime.agent!);
      default:
        final status = _statusFor(name, 'local', runtime, 'restored-$name');
        serveHosts
          ..removeWhere((h) => h.name == name)
          ..add(status);
        if (!_services.any((s) => s.key == status.appServiceKey)) {
          _services.add(
            ServiceEntry(
              device: 'local',
              kind: status.sessionKind,
              name: name,
              key: status.appServiceKey,
            ),
          );
        }
    }
  }

  @override
  Future<String?> hostRestoreProblem({String? name}) async =>
      restoreProblems[name ?? ''];

  @override
  Future<ManagedDevice> managedDevice(String serviceKey) async {
    final device = managedDevices[serviceKey];
    if (device == null) {
      throw Exception('host request failed; refresh its state before retrying');
    }
    return device;
  }

  @override
  Future<ManagedHost> managedHostStart(
    String serviceKey, {
    required String name,
    required ManagedRuntime runtime,
    required HostConfigInput input,
    required HostKeyAction keyAction,
    String? key,
  }) => _mutate(
    'start',
    serviceKey,
    name,
    (runtime: runtime, input: input, keyAction: keyAction, key: key),
    (device) {
      final existing = device.hosts.where((h) => h.name == name).firstOrNull;
      if (existing?.running ?? false) {
        throw Exception(
          'already_running (409): This service is already running. Refresh it '
          'or choose another name.',
        );
      }
      final hostRuntime = _runtimeOf(runtime);
      final host = _running(
        device,
        ManagedHost(
          name: name,
          config: HostConfig(
            source: input.source,
            protocol: input.protocol,
            baseUrl: input.baseUrl,
            model: input.model,
            keySet: keyAction == HostKeyAction.replace,
            revision: (existing?.config.revision ?? 0) + 1,
            runtime: hostRuntime,
          ),
          restore: true,
        ),
        hostRuntime,
      );
      return _replace(serviceKey, device, host);
    },
  );

  @override
  Future<ManagedHost> managedHostSave(
    String serviceKey,
    String name,
    ManagedHostEdit edit,
  ) => _mutate('save', serviceKey, name, edit, (device) {
    final host = _hostOf(device, name);
    if (host.config.revision != edit.expectedRevision) {
      throw Exception(
        'configuration_changed (409): Another device saved this service. '
        'Reload before saving.',
      );
    }
    final c = host.config;
    final saved = ManagedHost(
      name: name,
      status: host.status,
      restore: host.restore,
      baseUrlMasked: edit.keepBaseUrl && host.baseUrlMasked,
      config: HostConfig(
        source: edit.input.source,
        protocol: edit.input.protocol,
        baseUrl: edit.keepBaseUrl ? c.baseUrl : edit.input.baseUrl,
        model: edit.input.model,
        keySet: switch (edit.keyAction) {
          HostKeyAction.keep => c.keySet,
          HostKeyAction.replace => true,
          HostKeyAction.remove => false,
        },
        claudeEnv: c.claudeEnv,
        revision: c.revision + 1,
        runtime: edit.runtime == null ? c.runtime : _runtimeOf(edit.runtime!),
        active: c.active,
      ),
    );
    return _replace(serviceKey, device, saved);
  });

  @override
  Future<HostApplyResult> managedHostApply(
    String serviceKey,
    String name, {
    required String expectedIncarnation,
    required int expectedRevision,
  }) => _mutate(
    'apply',
    serviceKey,
    name,
    (expectedIncarnation, expectedRevision),
    (device) {
      final host = _checkIncarnation(device, name, expectedIncarnation);
      if (host.config.revision != expectedRevision) {
        throw Exception(
          'configuration_changed (409): Another device saved this service. '
          'Reload before saving.',
        );
      }
      final runtime = host.config.runtime ?? host.config.active!.runtime;
      _replace(serviceKey, device, _running(device, host, runtime));
      return HostApplyResult(
        name: name,
        applied: true,
        revision: expectedRevision,
      );
    },
  );

  @override
  Future<void> managedHostStop(
    String serviceKey,
    String name, {
    required String expectedIncarnation,
  }) => _mutate('stop', serviceKey, name, expectedIncarnation, (device) {
    final host = _checkIncarnation(device, name, expectedIncarnation);
    final c = host.config;
    _replace(
      serviceKey,
      device,
      ManagedHost(
        name: name,
        baseUrlMasked: host.baseUrlMasked,
        config: HostConfig(
          source: c.source,
          protocol: c.protocol,
          baseUrl: c.baseUrl,
          model: c.model,
          keySet: c.keySet,
          claudeEnv: c.claudeEnv,
          revision: c.revision,
          runtime: c.runtime ?? c.active?.runtime,
          problem: c.problem,
        ),
      ),
    );
  });

  @override
  Future<ManagedHost> managedHostStartSaved(
    String serviceKey,
    String name, {
    required int expectedRevision,
  }) => _mutate('startSaved', serviceKey, name, expectedRevision, (device) {
    final host = _hostOf(device, name);
    if (host.running) {
      throw Exception(
        'already_running (409): This service is already running. Refresh it '
        'or choose another name.',
      );
    }
    if (host.config.revision != expectedRevision) {
      throw Exception(
        'configuration_changed (409): Another device saved this service. '
        'Reload before saving.',
      );
    }
    final runtime = host.config.runtime;
    if (runtime == null || host.config.problem != null) {
      throw Exception(
        'invalid_configuration (422): Review and save this service\'s '
        'configuration before starting.',
      );
    }
    return _replace(serviceKey, device, _running(device, host, runtime));
  });

  @override
  Future<List<ManagedRuntimeAvailability>> managedRuntimes(
    String serviceKey,
  ) async {
    if (managedRuntimesError case final error?) {
      managedRuntimesError = null;
      throw error;
    }
    return managedRuntimeLists[serviceKey] ?? const [];
  }

  @override
  Future<AcpClaudeSetup> managedClaudePrepare(
    String serviceKey, {
    required bool start,
  }) async {
    if (start) claudePrepareStarts++;
    if (claudePrepareScript.isEmpty) {
      return const AcpClaudeSetup(state: 'idle', stage: 'idle', elapsedMs: 0);
    }
    return claudePrepareScript.length > 1
        ? claudePrepareScript.removeAt(0)
        : claudePrepareScript.single;
  }

  Future<T> _mutate<T>(
    String op,
    String key,
    String name,
    Object? arg,
    T Function(ManagedDevice device) apply,
  ) async {
    managedCalls.add((op: op, key: key, name: name, arg: arg));
    final gate = managedGate;
    if (gate != null) await gate.future;
    final result = apply(await managedDevice(key));
    if (managedLoseNext.remove(op)) {
      throw Exception('host response interrupted; refresh its state');
    }
    return result;
  }

  ManagedHost _hostOf(ManagedDevice device, String name) =>
      device.hosts.where((h) => h.name == name).firstOrNull ??
      (throw Exception(
        'host_stopped (404): This service has stopped. Refresh its status.',
      ));

  ManagedHost _checkIncarnation(
    ManagedDevice device,
    String name,
    String incarnation,
  ) {
    final host = _hostOf(device, name);
    if (!host.running) {
      throw Exception(
        'host_stopped (404): This service has stopped. Refresh its status.',
      );
    }
    if (host.config.active?.incarnation != incarnation) {
      throw Exception(
        'host_changed (409): This service was replaced or restarted. Refresh '
        'before changing it.',
      );
    }
    return host;
  }

  ManagedHost _replace(String key, ManagedDevice device, ManagedHost host) {
    managedDevices[key] = ManagedDevice(
      device: device.device,
      os: device.os,
      metadata: device.metadata,
      appVersion: device.appVersion,
      restoreProblem: device.restoreProblem,
      hosts: [
        for (final h in device.hosts)
          if (h.name != host.name) h,
        host,
      ],
    );
    return host;
  }

  ManagedHost _running(
    ManagedDevice device,
    ManagedHost host,
    HostRuntime runtime,
  ) {
    final incarnation = 'inc-${host.name}-${++_managedIncarnations}';
    final c = host.config;
    return ManagedHost(
      name: host.name,
      restore: true,
      baseUrlMasked: host.baseUrlMasked,
      status: _statusFor(host.name, device.device, runtime, incarnation),
      config: HostConfig(
        source: c.source,
        protocol: c.protocol,
        baseUrl: c.baseUrl,
        model: c.model,
        keySet: c.keySet,
        claudeEnv: c.claudeEnv,
        revision: c.revision,
        runtime: c.runtime ?? runtime,
        active: HostActive(
          incarnation: incarnation,
          runtime: runtime,
          source: c.source,
          revision: c.revision,
        ),
      ),
    );
  }

  /// A live status like the bridge reports for [runtime] on [device].
  static AppServeStatus _statusFor(
    String name,
    String device,
    HostRuntime runtime,
    String incarnation,
  ) {
    final provider = switch (runtime.kind) {
      'opencode' => 'opencode',
      'acp' => 'acp',
      _ => 'codex',
    };
    final kind = switch (provider) {
      'codex' => 'app',
      final other => other,
    };
    return AppServeStatus(
      name: name,
      device: device,
      alive: true,
      appListenAddr: '127.0.0.1:18400',
      appServiceKey: 'pcx:$device:$kind:$name',
      appRegistered: true,
      metaListenAddr: '127.0.0.1:18401',
      metaServiceKey: 'pcx:$device:meta:$name',
      metaRegistered: true,
      provider: provider,
      providerName: switch (provider) {
        'opencode' => 'OpenCode',
        'acp' => runtime.agent?.displayName ?? 'Claude',
        _ => 'Codex',
      },
      profileId: runtime.agent?.profileId,
      incarnation: incarnation,
    );
  }

  static HostRuntime _runtimeOf(ManagedRuntime r) => switch (r.kind) {
    'opencode' => const HostRuntime.opencode(),
    'claude' => const HostRuntime.acp(
      AcpAgentSpec(
        profileId: 'claude',
        displayName: 'Claude',
        program: 'claude-agent-acp',
      ),
    ),
    _ => HostRuntime.codex(port: r.port, proxy: r.proxy),
  };
}
