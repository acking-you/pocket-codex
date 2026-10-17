part of '../bridge_api.dart';

/// This computer's remote-management switch, from
/// [BridgeApi.deviceManagementStatus].
class DeviceManagement {
  /// Creates a management status.
  const DeviceManagement({
    required this.supported,
    required this.enabled,
    required this.registered,
    this.serviceKey,
    this.problem,
  });

  /// This platform can host services (desktop only).
  final bool supported;

  /// Local opt-in; never changed by another device.
  final bool enabled;

  /// The `host` relay registration is live.
  final bool registered;

  /// This computer's management key, once started.
  final String? serviceKey;

  /// Safe startup or registration failure.
  final String? problem;
}

/// The authoritative boot-restore choice for one local host, from
/// [BridgeApi.hostRestoreOverrides]. It replaces any legacy UI preference of
/// the same name: a null [runtime] means the host was stopped and must not be
/// restored.
class HostRestoreOverride {
  /// Creates an override.
  const HostRestoreOverride({
    required this.name,
    required this.revision,
    this.runtime,
  });

  /// Canonical instance name.
  final String name;

  /// Revision to pass back to [BridgeApi.hostRestoreService].
  final int revision;

  /// Runtime to restore; null suppresses the name.
  final HostRuntime? runtime;
}

/// A remote computer reachable through its `host` management service.
class ManagedDevice {
  /// Creates a managed device.
  const ManagedDevice({
    required this.device,
    required this.os,
    required this.appVersion,
    this.metadata,
    this.hosts = const [],
    this.restoreProblem,
  });

  /// Relay device identity.
  final String device;

  /// The computer's operating system.
  final String os;

  /// Environment and publisher identity; absent on older hosts.
  final DeviceMetadata? metadata;

  /// Pocket version on the computer.
  final String appVersion;

  /// Its running and stopped services; may be empty.
  final List<ManagedHost> hosts;

  /// Automatic-restore warning, separate from any operation result.
  final String? restoreProblem;
}

/// One service owned by a managed computer.
class ManagedHost {
  /// Creates a managed host.
  const ManagedHost({
    required this.name,
    required this.config,
    this.status,
    this.restore = false,
    this.restoreProblem,
    this.baseUrlMasked = false,
  });

  /// Canonical instance name.
  final String name;

  /// Saved configuration and current launch. Keys are never returned; query
  /// credentials in [HostConfig.baseUrl] may be masked ([baseUrlMasked]).
  final HostConfig config;

  /// Live status, null when stopped.
  final AppServeStatus? status;

  /// Restored on the computer's next launch.
  final bool restore;

  /// Restore warning; a running service is still running.
  final String? restoreProblem;

  /// [HostConfig.baseUrl] is a display value; unchanged saves must keep the
  /// stored URL.
  final bool baseUrlMasked;

  /// Whether a live process is running.
  bool get running => status != null;

  /// The engine kind (`codex`, `opencode`, `acp`), from the live or saved
  /// runtime; empty when neither is known.
  String get engine =>
      status?.provider ??
      config.active?.runtime.kind ??
      config.runtime?.kind ??
      '';
}

/// A restricted runtime choice for a remote start or save. No executable path
/// or argv crosses this boundary.
class ManagedRuntime {
  /// Creates a runtime choice.
  const ManagedRuntime({
    required this.kind,
    this.port = 0,
    this.proxy,
    this.keepProxy = false,
  });

  /// `codex`, `opencode`, `claude` or `saved`.
  final String kind;

  /// Codex port, 0 for automatic. Codex only.
  final int port;

  /// Codex proxy; null inherits the host's environment. Codex only.
  final String? proxy;

  /// Keep the stored Codex proxy while changing the port.
  final bool keepProxy;
}

/// A complete remote edit of the revision read into the editor.
class ManagedHostEdit {
  /// Creates an edit.
  const ManagedHostEdit({
    required this.input,
    required this.keyAction,
    this.key,
    required this.expectedRevision,
    this.runtime,
    this.keepBaseUrl = false,
  });

  /// Model service and Claude overrides (null values keep stored ones).
  final HostConfigInput input;

  /// Write-only key handling.
  final HostKeyAction keyAction;

  /// Replacement key, only with [HostKeyAction.replace].
  final String? key;

  /// Revision read before editing.
  final int expectedRevision;

  /// Null keeps the saved runtime, including private paths and proxy.
  final ManagedRuntime? runtime;

  /// Keep the stored URL behind a masked display value.
  final bool keepBaseUrl;
}

/// A runtime a managed computer can start, from [BridgeApi.managedRuntimes].
class ManagedRuntimeAvailability {
  /// Creates an availability row.
  const ManagedRuntimeAvailability({
    required this.kind,
    required this.ready,
    this.path,
    this.version,
    this.needsHostLogin = false,
    this.canPrepare = false,
    this.problem,
  });

  /// `codex`, `opencode` or `claude`.
  final String kind;

  /// Can start now (provider sign-in aside).
  final bool ready;

  /// Detected executable on the host, read-only.
  final String? path;

  /// Detected version.
  final String? version;

  /// The inherited provider needs sign-in on the host computer.
  final bool needsHostLogin;

  /// The Claude adapter can be prepared explicitly.
  final bool canPrepare;

  /// Safe explanation when not ready.
  final String? problem;
}

/// The host's project settings, from [BridgeApi.metaProjectSettings].
class ProjectSettings {
  /// Creates project settings.
  const ProjectSettings({
    this.projectRoots = const [],
    this.defaultProject,
    this.revision,
    this.effectiveDefault,
    this.automatic = false,
    this.exists = false,
  });

  /// Explicit browsing roots on the host.
  final List<String> projectRoots;

  /// The user's saved default; null selects the automatic folder on new hosts.
  final String? defaultProject;

  /// Compare-and-save token; null identifies a legacy host.
  final String? revision;

  /// The folder new conversations actually use, when known.
  final String? effectiveDefault;

  /// [effectiveDefault] is the host OS's automatic folder.
  final bool automatic;

  /// [effectiveDefault] exists now (reading it never creates it).
  final bool exists;

  /// A host without the default-project protocol.
  bool get legacy => revision == null;

  /// The legacy view of these settings.
  ProjectConfig get config =>
      ProjectConfig(projectRoots: projectRoots, defaultProject: defaultProject);
}
