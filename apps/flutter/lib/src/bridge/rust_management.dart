part of '../bridge_api_rust.dart';

mixin _RustManagement implements BridgeApi {
  @override
  DeviceMetadata localDeviceMetadata() =>
      _deviceMetadata(frb.localDeviceMetadata());

  @override
  Future<DeviceMetadata?> serviceDeviceMetadata(String serviceKey) async {
    final value = await frb.serviceDeviceMetadata(serviceKey: serviceKey);
    return value == null ? null : _deviceMetadata(value);
  }

  @override
  Future<ProjectSettings> metaProjectSettings(String serviceKey) async =>
      _projectSettings(await frb.metaProjectSettings(serviceKey: serviceKey));

  @override
  Future<ProjectSettings> metaSaveProjectSettings(
    String serviceKey, {
    required List<String> projectRoots,
    String? defaultProject,
    String? expectedRevision,
  }) async => _projectSettings(
    await frb.metaSaveProjectSettings(
      serviceKey: serviceKey,
      projectRoots: projectRoots,
      defaultProject: defaultProject,
      expectedRevision: expectedRevision,
    ),
  );

  @override
  Future<DeviceManagement> deviceManagementStatus() async =>
      _deviceManagement(await frb.deviceManagementStatus());

  @override
  Future<DeviceManagement> deviceManagementSetEnabled(bool enabled) async =>
      _deviceManagement(await frb.deviceManagementSetEnabled(enabled: enabled));

  @override
  Future<List<HostRestoreOverride>> hostRestoreOverrides() async => [
    for (final o in await frb.hostRestoreOverrides())
      HostRestoreOverride(
        name: o.name,
        revision: o.revision.toInt(),
        runtime: o.runtime == null ? null : _runtime(o.runtime!),
      ),
  ];

  @override
  Future<void> hostRestoreService(
    String name, {
    int? expectedRevision,
    HostRuntime? legacyRuntime,
  }) => frb.hostRestoreService(
    name: name,
    expectedRevision: expectedRevision == null
        ? null
        : BigInt.from(expectedRevision),
    legacyRuntime: legacyRuntime == null ? null : _runtimeDto(legacyRuntime),
  );

  @override
  Future<String?> hostRestoreProblem({String? name}) =>
      frb.hostRestoreProblem(name: name);

  @override
  Future<ManagedDevice> managedDevice(String serviceKey) async {
    final d = await frb.managedDevice(serviceKey: serviceKey);
    return ManagedDevice(
      device: d.device,
      os: d.os,
      metadata: d.metadata == null ? null : _deviceMetadata(d.metadata!),
      appVersion: d.appVersion,
      hosts: d.hosts.map(_managedHost).toList(growable: false),
      restoreProblem: d.restoreProblem,
    );
  }

  @override
  Future<ManagedHost> managedHostStart(
    String serviceKey, {
    required String name,
    required ManagedRuntime runtime,
    required HostConfigInput input,
    required HostKeyAction keyAction,
    String? key,
  }) async => _managedHost(
    await frb.managedHostStart(
      serviceKey: serviceKey,
      name: name,
      runtime: _managedRuntimeDto(runtime),
      input: _inputDto(input),
      keyAction: keyAction.name,
      key: key,
    ),
  );

  @override
  Future<ManagedHost> managedHostSave(
    String serviceKey,
    String name,
    ManagedHostEdit edit,
  ) async => _managedHost(
    await frb.managedHostSave(
      serviceKey: serviceKey,
      name: name,
      edit: frb.ManagedHostSaveDto(
        input: _inputDto(edit.input),
        keyAction: edit.keyAction.name,
        key: edit.key,
        expectedRevision: BigInt.from(edit.expectedRevision),
        runtime: edit.runtime == null
            ? null
            : _managedRuntimeDto(edit.runtime!),
        keepBaseUrl: edit.keepBaseUrl,
      ),
    ),
  );

  @override
  Future<HostApplyResult> managedHostApply(
    String serviceKey,
    String name, {
    required String expectedIncarnation,
    required int expectedRevision,
  }) async {
    final r = await frb.managedHostApply(
      serviceKey: serviceKey,
      name: name,
      expectedIncarnation: expectedIncarnation,
      expectedRevision: BigInt.from(expectedRevision),
    );
    return HostApplyResult(
      name: r.name,
      applied: r.applied,
      restoredPrevious: r.restoredPrevious,
      error: r.error,
      restoreError: r.restoreError,
      revision: r.revision?.toInt(),
    );
  }

  @override
  Future<void> managedHostStop(
    String serviceKey,
    String name, {
    required String expectedIncarnation,
  }) => frb.managedHostStop(
    serviceKey: serviceKey,
    name: name,
    expectedIncarnation: expectedIncarnation,
  );

  @override
  Future<ManagedHost> managedHostStartSaved(
    String serviceKey,
    String name, {
    required int expectedRevision,
  }) async => _managedHost(
    await frb.managedHostStartSaved(
      serviceKey: serviceKey,
      name: name,
      expectedRevision: BigInt.from(expectedRevision),
    ),
  );

  @override
  Future<List<ManagedRuntimeAvailability>> managedRuntimes(
    String serviceKey,
  ) async => [
    for (final r in await frb.managedRuntimes(serviceKey: serviceKey))
      ManagedRuntimeAvailability(
        kind: r.kind,
        ready: r.ready,
        path: r.path,
        version: r.version,
        needsHostLogin: r.needsHostLogin,
        canPrepare: r.canPrepare,
        problem: r.problem,
      ),
  ];

  @override
  Future<AcpClaudeSetup> managedClaudePrepare(
    String serviceKey, {
    required bool start,
  }) async => _claudeSetup(
    await frb.managedClaudePrepare(serviceKey: serviceKey, start: start),
  );
}

ProjectSettings _projectSettings(frb.ProjectSettingsDto s) => ProjectSettings(
  projectRoots: s.projectRoots,
  defaultProject: s.defaultProject,
  revision: s.revision,
  effectiveDefault: s.effectiveDefault,
  automatic: s.automatic,
  exists: s.exists,
);

DeviceMetadata _deviceMetadata(frb.DeviceMetadataDto d) => DeviceMetadata(
  os: d.os,
  arch: d.arch,
  environment: d.environment,
  hostname: d.hostname,
  hostId: d.hostId,
  sourceName: d.sourceName,
  instanceId: d.instanceId,
);

DeviceManagement _deviceManagement(frb.DeviceManagementDto d) =>
    DeviceManagement(
      supported: d.supported,
      enabled: d.enabled,
      registered: d.registered,
      serviceKey: d.serviceKey,
      problem: d.problem,
    );

ManagedHost _managedHost(frb.ManagedHostDto h) => ManagedHost(
  name: h.name,
  config: _hostConfig(h.config),
  status: h.status == null ? null : _serveStatus(h.status!),
  restore: h.restore,
  restoreProblem: h.restoreProblem,
  baseUrlMasked: h.baseUrlMasked,
);

frb.ManagedRuntimeDto _managedRuntimeDto(ManagedRuntime r) =>
    frb.ManagedRuntimeDto(
      kind: r.kind,
      port: r.port,
      proxy: r.proxy,
      keepProxy: r.keepProxy,
    );
