import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/ui_prefs.dart';

/// Which saved hosting to bring back on a desktop cold start.
class HostRestorePlan {
  /// Creates a plan.
  const HostRestorePlan({this.codex, this.openCode, this.acp = const []});

  /// Codex hosting to restore.
  final AutoHostPrefs? codex;

  /// OpenCode service hosting to restore.
  final AutoHostOpenCodePrefs? openCode;

  /// ACP agents to restore, each under its own instance name.
  final List<AutoHostAcpPrefs> acp;

  /// Whether nothing needs restoring.
  bool get isEmpty => codex == null && openCode == null && acp.isEmpty;
}

/// Plan the restore: each saved record at most once per run (callers track
/// the attempts), and only what this device does not host already — a Codex
/// record by provider (it has one default host), the OpenCode service by
/// provider, and ACP agents by instance name. Providers are compared by
/// what hosts say they are, never inferred from what they are not.
HostRestorePlan planHostRestore(
  UiPrefs prefs,
  List<AppServeStatus> local, {
  required bool codexAttempted,
  required bool openCodeAttempted,
  required Set<String> acpAttempted,
  required bool acpSupported,
}) {
  final codex = codexAttempted || local.any((h) => h.isCodex)
      ? null
      : prefs.autoHost;
  final openCode = openCodeAttempted || local.any((h) => h.isOpenCode)
      ? null
      : prefs.autoHostOpenCode;
  final hostedNames = {for (final h in local) h.name};
  final acp = !acpSupported
      ? const <AutoHostAcpPrefs>[]
      : [
          for (final saved in prefs.autoHostAcp)
            if (!acpAttempted.contains(saved.name) &&
                !hostedNames.contains(saved.name))
              saved,
        ];
  return HostRestorePlan(codex: codex, openCode: openCode, acp: acp);
}

/// Run [plan]; a failed restore is left to the start-hosting fallback.
/// Returns whether anything was restored (discovery is then stale).
Future<bool> runHostRestore(BridgeApi api, HostRestorePlan plan) async {
  var restored = false;
  final codex = plan.codex;
  if (codex != null) {
    try {
      await api.appServeStart(
        port: codex.port,
        binaryOverride: codex.binaryOverride,
        name: codex.name,
        proxy: codex.proxy,
        // Legacy built-in hosts are restored through the external binary.
        embedded: false,
      );
      restored = true;
    } catch (_) {
      // The hero (with its start-hosting action) is the fallback.
    }
  }
  final openCode = plan.openCode;
  if (openCode != null) {
    try {
      await api.appServeStartOpencode(
        name: openCode.name,
        binaryOverride: openCode.binaryOverride,
      );
      restored = true;
    } catch (_) {
      // OpenCode may simply not be installed any more.
    }
  }
  for (final saved in plan.acp) {
    try {
      await api.appServeStartAcp(name: saved.name, spec: saved.spec);
      restored = true;
    } catch (_) {
      // The agent may have been uninstalled; its record stays for next time.
    }
  }
  return restored;
}
