import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:pocket_codex/src/desktop_theme.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/l10n/gen/app_localizations.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/dismissed_services.dart';
import 'package:pocket_codex/src/error_format.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/ui_prefs.dart';
import 'package:pocket_codex/src/widgets/github_avatar.dart';
import 'package:pocket_codex/src/widgets/loading.dart';
import 'package:pocket_codex/src/widgets/local_host_dialog.dart';
import 'package:pocket_codex/src/widgets/status_dots.dart';
import 'package:pocket_codex/src/widgets/utility_page.dart';

/// Management hub (`/manage`): lists discovered services on the configured
/// relay, plus the Sessions browser and desktop local hosting. The chat-first
/// [HomeScreen] replaced it at `/`; everything here is one tap away from the
/// chat sidebar.
class ServicesScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const ServicesScreen({super.key});

  @override
  ConsumerState<ServicesScreen> createState() => _ServicesScreenState();
}

class _ServicesScreenState extends ConsumerState<ServicesScreen>
    with WidgetsBindingObserver {
  /// Cadence for re-probing every app-server's reachability so a server that
  /// came back online flips from "unreachable" to "online" on its own — the
  /// manual refresh button stays as a fallback. Kept in the same order of
  /// magnitude as the session keepalive while staying gentle enough to avoid
  /// probe churn against the remote app-server.
  static const _reprobeInterval = Duration(seconds: 15);

  Timer? _reprobeTimer;
  String? _selectedDevice;

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _reprobeTimer = Timer.periodic(_reprobeInterval, (_) {
      // Re-run discovery AND re-probe each service's reachability. Discovery
      // (a single /v1/services call) is refreshed too because the desktop
      // auto-hosts on startup — the initial listing is fetched before the
      // host finishes registering, so without a periodic re-fetch the page
      // stays empty even though the services are live on the relay.
      if (mounted) {
        ref.invalidate(servicesProvider);
        ref.invalidate(appReachableProvider);
        ref.invalidate(apiReachableProvider);
        ref.invalidate(appReachableLocalProvider);
        ref.invalidate(apiReachableLocalProvider);
        ref.invalidate(localServeListProvider);
      }
    });
    // Discovery may be cached stale (fetched before the desktop finished
    // auto-hosting), so refresh it once immediately on open instead of waiting
    // for the first periodic tick.
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) ref.invalidate(servicesProvider);
    });
  }

  @override
  void dispose() {
    _reprobeTimer?.cancel();
    WidgetsBinding.instance.removeObserver(this);
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    // Returning to the foreground: re-probe once immediately so a server that
    // recovered while we were backgrounded shows online without waiting a tick.
    if (state == AppLifecycleState.resumed && mounted) {
      ref.invalidate(servicesProvider);
      ref.invalidate(appReachableProvider);
      ref.invalidate(apiReachableProvider);
      ref.invalidate(appReachableLocalProvider);
      ref.invalidate(apiReachableLocalProvider);
      // Refresh local hosts too (a host's codex/tunnels may have changed while
      // backgrounded) — same as the periodic timer + the refresh button.
      ref.invalidate(localServeListProvider);
    }
  }

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final servicesAsync = ref.watch(servicesProvider);
    final config = ref.watch(configProvider).valueOrNull;
    final account = config?.mode == 'account';

    return UtilityPage(
      route: '/manage',
      title: l10n.manageServices,
      actions: [
        IconButton(
          key: const Key('refresh-btn'),
          icon: const Icon(Icons.refresh),
          tooltip: l10n.refreshStatus,
          // Re-discover services, re-read subscription health, and re-probe
          // every app-server's backend reachability, then rebuild so each
          // status re-evaluates.
          onPressed: () {
            ref.invalidate(servicesProvider);
            ref.invalidate(subscriptionsProvider);
            ref.invalidate(appReachableProvider);
            ref.invalidate(apiReachableProvider);
            ref.invalidate(appReachableLocalProvider);
            ref.invalidate(apiReachableLocalProvider);
            ref.invalidate(localServeListProvider);
          },
        ),
        if (_hostingSupported && account)
          Padding(
            padding: const EdgeInsets.symmetric(vertical: 8),
            child: FilledButton.icon(
              key: const Key('host-this-device-btn'),
              onPressed: () => showDialog<void>(
                context: context,
                builder: (_) => const LocalHostDialog(),
              ),
              icon: const Icon(Icons.add, size: 18),
              label: Text(l10n.servicesHostThisDevice),
            ),
          ),
      ],
      body: AnimatedSwitcher(
        duration: const Duration(milliseconds: 250),
        child: servicesAsync.when(
          loading: () =>
              const ListLoadingSkeleton(key: ValueKey('svc-loading')),
          error: (e, _) {
            final detail = friendlyError(e);
            final sessionExpired = isAccountSessionExpired(detail);
            return KeyedSubtree(
              key: const ValueKey('svc-error'),
              child: _ErrorState(
                detail: detail,
                sessionExpired: sessionExpired,
                onRetry: () => ref.invalidate(servicesProvider),
                onSignIn: sessionExpired
                    ? () => context.go('/onboarding?reason=session-expired')
                    : null,
              ),
            );
          },
          data: (services) => KeyedSubtree(
            key: const ValueKey('svc-data'),
            child: _DeviceFirstServices(
              services: services,
              relay: config?.relay,
              accountLogin: account ? config?.accountLogin : null,
              accountId: account ? config?.accountId : null,
              selectedDevice: _selectedDevice,
              onSelectDevice: (device) {
                if (_selectedDevice != device) {
                  setState(() => _selectedDevice = device);
                }
              },
              onClearDevice: () {
                if (_selectedDevice != null) {
                  setState(() => _selectedDevice = null);
                }
              },
            ),
          ),
        ),
      ),
    );
  }
}

/// The service inventory, organized around devices rather than protocol tabs.
/// App/API remain the real discovered services; session sharing is presented
/// as a capability derived from an account-mode app host, because meta is
/// intentionally not returned by service discovery.
///
/// Wide lays the device column beside the selected device's detail. Narrow can't
/// hold both, so it becomes two levels: the device list, then that device's
/// capabilities with the title bar's `Services / <device>` origin leading back.
class _DeviceFirstServices extends ConsumerWidget {
  const _DeviceFirstServices({
    required this.services,
    required this.relay,
    required this.accountLogin,
    required this.accountId,
    required this.selectedDevice,
    required this.onSelectDevice,
    required this.onClearDevice,
  });

  final List<ServiceEntry> services;
  final String? relay;
  final String? accountLogin;
  final String? accountId;

  /// The device whose detail is shown. On narrow this doubles as the level:
  /// null is the device list, set is that device's capabilities.
  final String? selectedDevice;
  final ValueChanged<String> onSelectDevice;

  /// Return to the device list (narrow only).
  final VoidCallback onClearDevice;

  /// Below this the device column and the detail can't sit side by side. Matches
  /// the conversation's own document-layout breakpoint so the whole app changes
  /// idiom at one width rather than each screen picking its own.
  static const double _splitWidth = 720;

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final account = accountLogin != null;
    final pending = ref.watch(pendingRemovalProvider);
    final dismissed =
        ref.watch(dismissedServicesProvider).valueOrNull ?? const <String>{};
    final localHosts =
        ref.watch(localServeListProvider).valueOrNull ??
        const <AppServeStatus>[];
    ref.listen(servicesProvider, (_, next) {
      final data = next.valueOrNull;
      if (data == null) return;
      final present = {for (final service in data) service.key};
      final current = ref.read(pendingRemovalProvider);
      final stillHidden = current.intersection(present);
      if (stillHidden.length != current.length) {
        ref.read(pendingRemovalProvider.notifier).state = stillHidden;
      }
    });

    final visible = <ServiceEntry>[
      for (final service in services)
        if (!pending.contains(service.key) && !dismissed.contains(service.key))
          service,
    ];
    // A freshly-started host can precede the next relay discovery refresh.
    // Synthesize only its actually-published app/API entries so the device and
    // its capabilities appear immediately without inventing remote services.
    for (final host in localHosts) {
      if (host.appRegistered &&
          !pending.contains(host.appServiceKey) &&
          !visible.any((service) => service.key == host.appServiceKey)) {
        visible.add(
          ServiceEntry(
            device: host.device,
            kind: 'app',
            name: host.name,
            key: host.appServiceKey,
          ),
        );
      }
      if (host.apiRegistered &&
          !pending.contains(host.apiServiceKey) &&
          !visible.any((service) => service.key == host.apiServiceKey)) {
        visible.add(
          ServiceEntry(
            device: host.device,
            kind: 'api',
            name: host.name,
            key: host.apiServiceKey,
          ),
        );
      }
    }

    // A dismissed orphan should reappear if its backend later recovers. This
    // mirrors the compact layout's recovery contract.
    if (dismissed.isNotEmpty) {
      final recovered = <String>[
        for (final service in services)
          if (dismissed.contains(service.key) &&
              (service.kind == 'app'
                          ? ref.watch(appReachableProvider(service.key))
                          : ref.watch(apiReachableProvider(service.key)))
                      .valueOrNull ==
                  true)
            service.key,
      ];
      if (recovered.isNotEmpty) {
        final notifier = ref.read(dismissedServicesProvider.notifier);
        Future.microtask(() => notifier.restore(recovered));
      }
    }

    final devices = <String>{
      for (final service in visible) service.device,
      for (final host in localHosts) host.device,
    }.toList()..sort();
    final localDevices = {for (final host in localHosts) host.device};
    final preferredKey = ref.watch(
      uiPrefsProvider.select(
        (prefs) => prefs.valueOrNull?.preferredAppServiceKey,
      ),
    );
    final preferredDevice = visible
        .where((service) => service.key == preferredKey)
        .firstOrNull
        ?.device;
    devices.sort((a, b) {
      int rank(String device) {
        if (device == preferredDevice) return 0;
        if (localDevices.contains(device)) return 1;
        return 2;
      }

      final byRank = rank(a).compareTo(rank(b));
      return byRank == 0 ? a.compareTo(b) : byRank;
    });
    final split = MediaQuery.sizeOf(context).width >= _splitWidth;
    // Wide always has a device in the detail pane — an empty pane beside a
    // populated column reads as broken. Narrow shows the list first and only
    // resolves a device once one is picked, so nothing is chosen on the user's
    // behalf on a screen that can only show one level at a time.
    final activeDevice = devices.contains(selectedDevice)
        ? selectedDevice
        : !split
        ? null
        : preferredDevice != null && devices.contains(preferredDevice)
        ? preferredDevice
        : devices.firstOrNull;
    final deviceEntries =
        visible.where((service) => service.device == activeDevice).toList()
          ..sort((a, b) {
            final byName = a.name.compareTo(b.name);
            return byName == 0 ? a.kind.compareTo(b.kind) : byName;
          });
    final apps = deviceEntries
        .where((service) => service.kind == 'app')
        .toList();
    final apis = deviceEntries
        .where((service) => service.kind == 'api')
        .toList();
    final capabilityCount =
        apps.length + apis.length + (account ? apps.length : 0);

    final localAppAddr = <String, String>{
      for (final host in localHosts) host.appServiceKey: host.appListenAddr,
    };
    final localApiAddr = <String, String>{
      for (final host in localHosts) host.apiServiceKey: host.apiListenAddr,
    };
    final localTunnels = <String, ({String name, String kind})>{
      for (final host in localHosts) ...{
        host.appServiceKey: (name: host.name, kind: 'app'),
        host.apiServiceKey: (name: host.name, kind: 'api'),
      },
    };
    final subscriptions = {
      for (final sub
          in ref.watch(subscriptionsProvider).valueOrNull ?? const <SubInfo>[])
        sub.key: sub,
    };
    final bridge = ref.watch(bridgeApiProvider);
    final observedDown = ref.watch(observedDisconnectedProvider);
    final online = successColor(scheme);

    // A capability's status pill, plus the reason when it is unreachable: the
    // relay registration can outlive the backend it forwards to, and "offline"
    // alone leaves the user unable to tell which half is at fault.
    ({Widget chip, String? reason}) status({
      required bool unreachable,
      required String label,
      required Color color,
      required String reasonText,
    }) => (
      chip: StatusChip(color: color, label: label, filled: true),
      reason: unreachable ? reasonText : null,
    );

    ({Widget chip, String? reason}) appStatus(ServiceEntry service) {
      if (!observedDown.contains(service.key) &&
          bridge.appIsConnected(service.key)) {
        return status(
          unreachable: false,
          label: l10n.statusConnected,
          color: online,
          reasonText: l10n.unreachableReason,
        );
      }
      if (observedDown.contains(service.key)) {
        return status(
          unreachable: true,
          label: l10n.statusUnreachable,
          color: scheme.error,
          reasonText: l10n.unreachableReason,
        );
      }
      final localAddr = localAppAddr[service.key];
      final reach = localAddr == null
          ? ref.watch(appReachableProvider(service.key))
          : ref.watch(appReachableLocalProvider(localAddr));
      return reach.when(
        data: (ok) => status(
          unreachable: !ok,
          label: ok ? l10n.statusOnline : l10n.statusUnreachable,
          color: ok ? online : scheme.error,
          reasonText: l10n.unreachableReason,
        ),
        loading: () => status(
          unreachable: false,
          label: l10n.statusChecking,
          color: scheme.outline,
          reasonText: l10n.unreachableReason,
        ),
        error: (_, _) => status(
          unreachable: true,
          label: l10n.statusUnreachable,
          color: scheme.error,
          reasonText: l10n.unreachableReason,
        ),
      );
    }

    ({Widget chip, String? reason}) apiStatus(ServiceEntry service) {
      final sub = subscriptions[service.key];
      if (sub != null) {
        return status(
          unreachable: !sub.alive,
          label: sub.alive ? l10n.subscribedAlive : l10n.subscribedDead,
          color: sub.alive ? online : scheme.error,
          reasonText: l10n.apiUnreachableReason,
        );
      }
      final localAddr = localApiAddr[service.key];
      final reach = localAddr == null
          ? ref.watch(apiReachableProvider(service.key))
          : ref.watch(apiReachableLocalProvider(localAddr));
      return reach.when(
        data: (ok) => status(
          unreachable: !ok,
          label: ok ? l10n.statusOnline : l10n.statusUnreachable,
          color: ok ? online : scheme.error,
          reasonText: l10n.apiUnreachableReason,
        ),
        loading: () => status(
          unreachable: false,
          label: l10n.statusChecking,
          color: scheme.outline,
          reasonText: l10n.apiUnreachableReason,
        ),
        error: (_, _) => status(
          unreachable: true,
          label: l10n.statusUnreachable,
          color: scheme.error,
          reasonText: l10n.apiUnreachableReason,
        ),
      );
    }

    bool unreachable(ServiceEntry service) {
      final local = localTunnels.containsKey(service.key);
      if (local) return false;
      if (service.kind == 'app' && observedDown.contains(service.key)) {
        return true;
      }
      final probe = service.kind == 'app'
          ? ref.watch(appReachableProvider(service.key))
          : ref.watch(apiReachableProvider(service.key));
      return probe.hasError || probe.valueOrNull == false;
    }

    final unreachableEntries = deviceEntries
        .where(unreachable)
        .toList(growable: false);
    // Resolve each status once — the builders watch providers, so calling one
    // twice per row would double the watch registrations.
    final appStates = {for (final s in apps) s.key: appStatus(s)};
    final apiStates = {for (final s in apis) s.key: apiStatus(s)};
    final capabilityRows = <Widget>[
      for (final service in apps)
        _CapabilityRow(
          key: Key('device-capability-${service.key}'),
          icon: Icons.chat_bubble_outline,
          title: l10n.servicesChatCapability,
          protocol: 'App-server · ${service.name}',
          status: appStates[service.key]!.chip,
          reason: appStates[service.key]!.reason,
          isDefault: service.key == preferredKey,
          onSetDefault: () => ref
              .read(uiPrefsProvider.notifier)
              .setPreferredAppService(service.key),
          actionLabel: l10n.servicesOpen,
          onAction: () =>
              context.push('/app/${Uri.encodeComponent(service.key)}'),
          onDeregister: () => _confirmDeregister(
            context,
            ref,
            service,
            localTunnel: localTunnels[service.key],
            unreachable: unreachableEntries.any(
              (entry) => entry.key == service.key,
            ),
          ),
        ),
      for (final service in apis)
        _CapabilityRow(
          key: Key('device-capability-${service.key}'),
          icon: Icons.bolt_outlined,
          title: l10n.servicesApiCapability,
          protocol: 'API · ${service.name}',
          status: apiStates[service.key]!.chip,
          reason: apiStates[service.key]!.reason,
          actionLabel: l10n.servicesManage,
          onAction: () =>
              context.push('/api/${Uri.encodeComponent(service.key)}'),
          onDeregister: () => _confirmDeregister(
            context,
            ref,
            service,
            localTunnel: localTunnels[service.key],
            unreachable: unreachableEntries.any(
              (entry) => entry.key == service.key,
            ),
          ),
        ),
      if (account)
        for (final service in apps)
          _CapabilityRow(
            key: Key('device-capability-meta-${service.key}'),
            icon: Icons.forum_outlined,
            title: l10n.servicesSessionsCapability,
            protocol: 'Meta · ${service.name}',
            actionLabel: l10n.servicesBrowse,
            // Pushed, like the chat and API rows beside it, so the breadcrumb
            // back to this device's capabilities actually has somewhere to go.
            onAction: () => context.push(
              Uri(
                path: '/sessions',
                queryParameters: {'svc': service.key},
              ).toString(),
            ),
          ),
    ];

    int countFor(String device) {
      final entries = visible
          .where((service) => service.device == device)
          .toList();
      final appCount = entries.where((service) => service.kind == 'app').length;
      return entries.length + (account ? appCount : 0);
    }

    final deviceColumn = _DeviceColumn(
      relay: relay ?? l10n.relayNotConfigured,
      accountLogin: accountLogin,
      accountId: accountId,
      devices: devices,
      activeDevice: activeDevice,
      localDevices: localDevices,
      capabilityCount: countFor,
      onSelect: onSelectDevice,
      // Beside a detail pane the column owns the full height and scrolls its
      // own list; stacked it is the page's only content and scrolls with it.
      filled: split,
    );

    final detail = <Widget>[
      _DeviceDetailHeader(
        device: activeDevice,
        local: activeDevice != null && localDevices.contains(activeDevice),
        isDefault: activeDevice != null && activeDevice == preferredDevice,
        onClean: unreachableEntries.isEmpty
            ? null
            : () => _batchRemove(context, ref, unreachableEntries),
      ),
      Card(
        clipBehavior: Clip.antiAlias,
        child: Column(
          children: [
            _CardHeading(
              title: l10n.servicesCapabilities,
              trailing: _CountPill(
                label: l10n.servicesDeviceCapabilityCount(capabilityCount),
              ),
            ),
            if (capabilityRows.isEmpty)
              Padding(
                padding: const EdgeInsets.all(32),
                child: Text(
                  l10n.servicesNoCapabilities,
                  style: TextStyle(color: scheme.onSurfaceVariant),
                ),
              )
            else
              ...capabilityRows,
          ],
        ),
      ),
      if (_hostingSupported && account) ...[
        const SizedBox(height: 12),
        Card(
          clipBehavior: Clip.antiAlias,
          child: Column(
            children: [
              _CardHeading(
                title: l10n.localHostingSection,
                trailing: localHosts.isEmpty
                    ? null
                    : StatusChip(
                        color: online,
                        label: l10n.localHostRunning,
                        filled: true,
                      ),
              ),
              Padding(
                padding: const EdgeInsets.fromLTRB(12, 8, 12, 12),
                child: Column(
                  children: [
                    for (final host in localHosts)
                      _LocalHostCard(
                        key: Key('local-host-${host.name}'),
                        host: host,
                      ),
                    const _AddLocalHostCard(),
                  ],
                ),
              ),
            ],
          ),
        ),
      ],
    ];

    // Narrow: one level at a time. The device list stands alone until a device
    // is picked; the detail then replaces it, with a back affordance in the
    // header rather than a second column squeezed alongside.
    if (!split) {
      return _NarrowServices(
        // With no devices at all there is no list worth showing — and the empty
        // state and the "host this machine" card both live in the detail, so
        // going straight there is the only way to reach them.
        showDetail: activeDevice != null || devices.isEmpty,
        canGoBack: devices.isNotEmpty,
        onBack: onClearDevice,
        deviceColumn: deviceColumn,
        detail: detail,
      );
    }

    return Align(
      alignment: Alignment.topCenter,
      child: ConstrainedBox(
        constraints: const BoxConstraints(maxWidth: 1160),
        child: Padding(
          padding: const EdgeInsets.fromLTRB(24, 18, 24, 24),
          child: Row(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              SizedBox(width: 260, child: deviceColumn),
              const SizedBox(width: 14),
              Expanded(child: ListView(children: detail)),
            ],
          ),
        ),
      ),
    );
  }
}

/// The compact two-level form of [_DeviceFirstServices]: the device list, or one
/// device's capabilities with a back row above them.
class _NarrowServices extends StatelessWidget {
  const _NarrowServices({
    required this.showDetail,
    required this.canGoBack,
    required this.onBack,
    required this.deviceColumn,
    required this.detail,
  });

  final bool showDetail;

  /// Whether there is a device list to return to. False when the detail is the
  /// only level (no devices discovered), so no back affordance is offered.
  final bool canGoBack;

  final VoidCallback onBack;
  final Widget deviceColumn;
  final List<Widget> detail;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    // A level change is a navigation, so it moves rather than cross-fades:
    // forward slides in from the trailing edge, back from the leading one.
    final switcher = AnimatedSwitcher(
      duration: const Duration(milliseconds: 220),
      switchInCurve: Curves.easeOutCubic,
      switchOutCurve: Curves.easeOutCubic,
      transitionBuilder: (child, animation) => SlideTransition(
        position: Tween<Offset>(
          begin: Offset(showDetail ? 0.06 : -0.06, 0),
          end: Offset.zero,
        ).animate(animation),
        child: FadeTransition(opacity: animation, child: child),
      ),
      child: showDetail
          ? ListView(
              key: const ValueKey('svc-narrow-detail'),
              padding: const EdgeInsets.fromLTRB(12, 8, 12, 16),
              children: [
                if (canGoBack) ...[
                  Align(
                    alignment: Alignment.centerLeft,
                    child: TextButton.icon(
                      key: const Key('device-back'),
                      onPressed: onBack,
                      icon: const Icon(Icons.chevron_left, size: 18),
                      label: Text(l10n.servicesDevices),
                    ),
                  ),
                  const SizedBox(height: 4),
                ],
                ...detail,
              ],
            )
          : Padding(
              key: const ValueKey('svc-narrow-devices'),
              padding: const EdgeInsets.fromLTRB(12, 8, 12, 16),
              child: deviceColumn,
            ),
    );
    // Drilling into a device is a level, not a route, so the platform's back
    // gesture has to be told: without this it would leave Services entirely
    // from the detail, skipping the device list the user came through.
    return PopScope(
      canPop: !showDetail || !canGoBack,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) onBack();
      },
      child: switcher,
    );
  }
}

class _DeviceColumn extends StatelessWidget {
  const _DeviceColumn({
    required this.relay,
    required this.accountLogin,
    required this.accountId,
    required this.devices,
    required this.activeDevice,
    required this.localDevices,
    required this.capabilityCount,
    required this.onSelect,
    required this.filled,
  });

  final String relay;
  final String? accountLogin;
  final String? accountId;
  final List<String> devices;
  final String? activeDevice;
  final Set<String> localDevices;
  final int Function(String device) capabilityCount;
  final ValueChanged<String> onSelect;

  /// Whether to take the height it is given and scroll the device list inside.
  /// False sizes to the list instead, for a column that is the whole page.
  final bool filled;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final online = successColor(scheme);
    final tiles = <Widget>[
      for (final device in devices)
        _DeviceTile(
          key: Key('device-$device'),
          device: device,
          subtitle: [
            if (localDevices.contains(device)) l10n.servicesLocalDevice,
            l10n.servicesDeviceCapabilityCount(capabilityCount(device)),
          ].join(' · '),
          selected: device == activeDevice,
          onTap: () => onSelect(device),
        ),
    ];
    return Card(
      clipBehavior: Clip.antiAlias,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        mainAxisSize: filled ? MainAxisSize.max : MainAxisSize.min,
        children: [
          Padding(
            padding: const EdgeInsets.all(12),
            child: Row(
              children: [
                GitHubAvatar(
                  accountId: accountId,
                  fallbackIcon: accountLogin == null
                      ? Icons.dns_outlined
                      : Icons.person_outline,
                  size: 36,
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Text(
                    accountLogin == null ? relay : '@$accountLogin',
                    maxLines: 1,
                    overflow: TextOverflow.ellipsis,
                    style: const TextStyle(fontWeight: FontWeight.w600),
                  ),
                ),
                StatusChip(
                  color: online,
                  label: l10n.statusOnline,
                  filled: true,
                ),
              ],
            ),
          ),
          Divider(height: 1, color: scheme.outlineVariant),
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 13, 14, 7),
            child: Text(
              l10n.servicesDevices.toUpperCase(),
              style: Theme.of(context).textTheme.labelSmall?.copyWith(
                color: scheme.onSurfaceVariant,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          if (filled)
            Expanded(
              child: ListView(
                padding: const EdgeInsets.symmetric(horizontal: 8),
                children: tiles,
              ),
            )
          else
            Padding(
              padding: const EdgeInsets.fromLTRB(8, 0, 8, 8),
              child: Column(children: tiles),
            ),
        ],
      ),
    );
  }
}

class _DeviceTile extends StatelessWidget {
  const _DeviceTile({
    super.key,
    required this.device,
    required this.subtitle,
    required this.selected,
    required this.onTap,
  });

  final String device;
  final String subtitle;
  final bool selected;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(bottom: 4),
      child: Material(
        color: selected ? scheme.primaryContainer : Colors.transparent,
        borderRadius: BorderRadius.circular(kControlRadius),
        child: InkWell(
          mouseCursor: clickable,
          onTap: onTap,
          borderRadius: BorderRadius.circular(kControlRadius),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 9),
            child: Row(
              children: [
                Container(
                  width: 32,
                  height: 32,
                  decoration: BoxDecoration(
                    color: surfacePanel(scheme),
                    borderRadius: BorderRadius.circular(8),
                    border: Border.all(color: scheme.outlineVariant),
                  ),
                  child: Icon(
                    Icons.dns_outlined,
                    size: 17,
                    color: selected
                        ? scheme.onPrimaryContainer
                        : scheme.onSurfaceVariant,
                  ),
                ),
                const SizedBox(width: 10),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        device,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: TextStyle(
                          fontWeight: FontWeight.w600,
                          color: selected
                              ? scheme.onPrimaryContainer
                              : scheme.onSurface,
                        ),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        subtitle,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: Theme.of(context).textTheme.bodySmall?.copyWith(
                          color: scheme.onSurfaceVariant,
                        ),
                      ),
                    ],
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _DeviceDetailHeader extends StatelessWidget {
  const _DeviceDetailHeader({
    required this.device,
    required this.local,
    required this.isDefault,
    required this.onClean,
  });

  final String? device;
  final bool local;
  final bool isDefault;
  final VoidCallback? onClean;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: Row(
        children: [
          Container(
            width: 42,
            height: 42,
            decoration: BoxDecoration(
              color: scheme.primaryContainer,
              borderRadius: BorderRadius.circular(11),
            ),
            child: Icon(Icons.dns_outlined, color: scheme.onPrimaryContainer),
          ),
          const SizedBox(width: 12),
          Expanded(
            child: Text(
              device ?? l10n.servicesDevices,
              style: Theme.of(
                context,
              ).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w600),
            ),
          ),
          if (local) _CountPill(label: l10n.servicesLocalDevice),
          if (local && isDefault) const SizedBox(width: 8),
          if (isDefault) _CountPill(label: l10n.servicesDefault, accent: true),
          if (onClean != null) ...[
            const SizedBox(width: 8),
            TextButton.icon(
              key: const Key('device-clean-unreachable'),
              onPressed: onClean,
              icon: const Icon(Icons.cleaning_services_outlined, size: 17),
              label: Text(l10n.batchRemoveEnter),
            ),
          ],
        ],
      ),
    );
  }
}

class _CardHeading extends StatelessWidget {
  const _CardHeading({required this.title, this.trailing});

  final String title;
  final Widget? trailing;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      constraints: const BoxConstraints(minHeight: 46),
      padding: const EdgeInsets.symmetric(horizontal: 14, vertical: 8),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: scheme.outlineVariant)),
      ),
      child: Row(
        children: [
          Expanded(
            child: Text(
              title,
              style: const TextStyle(fontWeight: FontWeight.w600),
            ),
          ),
          ?trailing,
        ],
      ),
    );
  }
}

class _CapabilityRow extends StatelessWidget {
  const _CapabilityRow({
    super.key,
    required this.icon,
    required this.title,
    required this.protocol,
    required this.actionLabel,
    required this.onAction,
    this.status,
    this.reason,
    this.isDefault = false,
    this.onSetDefault,
    this.onDeregister,
  });

  final IconData icon;
  final String title;
  final String protocol;
  final String actionLabel;
  final VoidCallback onAction;
  final Widget? status;

  /// Why this capability is unavailable, when it is. A bare "unreachable" leaves
  /// the user guessing whether the relay or the backend is at fault, so the
  /// status pill is followed by the explanation.
  final String? reason;
  final bool isDefault;
  final VoidCallback? onSetDefault;
  final VoidCallback? onDeregister;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    return Container(
      constraints: const BoxConstraints(minHeight: 64),
      padding: const EdgeInsets.fromLTRB(12, 8, 7, 8),
      decoration: BoxDecoration(
        border: Border(bottom: BorderSide(color: scheme.outlineVariant)),
      ),
      child: Row(
        children: [
          Container(
            width: 36,
            height: 36,
            decoration: BoxDecoration(
              color: scheme.primaryContainer,
              borderRadius: BorderRadius.circular(kControlRadius),
            ),
            child: Icon(icon, size: 18, color: scheme.onPrimaryContainer),
          ),
          const SizedBox(width: 11),
          Expanded(
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              mainAxisAlignment: MainAxisAlignment.center,
              children: [
                Text(
                  title,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                ),
                Text(
                  protocol,
                  style: Theme.of(context).textTheme.bodySmall?.copyWith(
                    color: scheme.onSurfaceVariant,
                  ),
                ),
                if (reason != null) ...[
                  const SizedBox(height: 3),
                  Text(
                    reason!,
                    style: Theme.of(context).textTheme.bodySmall?.copyWith(
                      color: scheme.error,
                      height: 1.35,
                    ),
                  ),
                ],
              ],
            ),
          ),
          if (status != null) ...[status!, const SizedBox(width: 8)],
          if (isDefault)
            _CountPill(label: l10n.servicesDefault, accent: true)
          else if (onSetDefault != null)
            OutlinedButton(
              onPressed: onSetDefault,
              child: Text(l10n.servicesSetDefault),
            ),
          const SizedBox(width: 6),
          TextButton(onPressed: onAction, child: Text(actionLabel)),
          if (onDeregister != null)
            PopupMenuButton<String>(
              // Keyed because the page menu in the title bar carries the same
              // glyph; a test reaching for "the overflow" needs to say which.
              key: const Key('capability-menu'),
              tooltip: l10n.moreActions,
              icon: const Icon(Icons.more_horiz),
              onSelected: (_) => onDeregister!(),
              itemBuilder: (_) => [
                PopupMenuItem<String>(
                  value: 'deregister',
                  child: Text(
                    l10n.deregister,
                    style: TextStyle(color: scheme.error),
                  ),
                ),
              ],
            ),
        ],
      ),
    );
  }
}

class _CountPill extends StatelessWidget {
  const _CountPill({required this.label, this.accent = false});

  final String label;
  final bool accent;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    return Container(
      padding: const EdgeInsets.symmetric(horizontal: 9, vertical: 4),
      decoration: BoxDecoration(
        color: accent ? scheme.primaryContainer : scheme.surfaceContainer,
        borderRadius: BorderRadius.circular(999),
        border: Border.all(color: scheme.outlineVariant),
      ),
      child: Text(
        label,
        style: Theme.of(context).textTheme.labelSmall?.copyWith(
          color: accent ? scheme.onPrimaryContainer : scheme.onSurfaceVariant,
          fontWeight: accent ? FontWeight.w600 : FontWeight.w400,
        ),
      ),
    );
  }
}

/// The Sessions tab: pick a connected host, then browse that host's CODEX_HOME
/// sessions over its meta tunnel (loopback when this app hosts it, broker when
/// remote). Read-only transcripts + force-resume per session, via an embedded
/// [LocalSessionsScreen] in remote mode.
Future<void> _batchRemove(
  BuildContext context,
  WidgetRef ref,
  List<ServiceEntry> entries,
) async {
  final l10n = AppLocalizations.of(context);
  final scheme = Theme.of(context).colorScheme;
  if (entries.isEmpty) return;
  final ok = await showDialog<bool>(
    context: context,
    builder: (_) => AlertDialog(
      key: const Key('batch-remove-dialog'),
      title: Text(l10n.batchRemoveTitle),
      content: Text(l10n.batchRemoveWarning(entries.length)),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(l10n.cancel),
        ),
        FilledButton(
          key: const Key('batch-remove-confirm-btn'),
          style: FilledButton.styleFrom(backgroundColor: scheme.error),
          onPressed: () => Navigator.of(context).pop(true),
          child: Text(l10n.remove),
        ),
      ],
    ),
  );
  if (ok != true) return;
  final dismiss = ref.read(dismissedServicesProvider.notifier);
  final bridge = ref.read(bridgeApiProvider);
  var removed = 0;
  for (final s in entries) {
    // Re-check reachability at confirm time (probes refresh every 15s): a key
    // that recovered while the dialog was open must not be dismissed — that
    // would strand a live service off the list.
    final reachableNow =
        (s.kind == 'app'
                ? ref.read(appReachableProvider(s.key))
                : ref.read(apiReachableProvider(s.key)))
            .valueOrNull ==
        true;
    if (reachableNow) continue;
    dismiss.dismiss(s.key);
    removed++;
    try {
      await bridge.accountDeregisterService(
        device: s.device,
        kind: s.kind,
        name: s.name,
      );
    } catch (_) {
      // Best-effort — the entry is already hidden from the list.
    }
  }
  // Refresh discovery so the list reflects the removals.
  ref.invalidate(servicesProvider);
  if (context.mounted && removed > 0) {
    ScaffoldMessenger.of(
      context,
    ).showSnackBar(SnackBar(content: Text(l10n.batchRemovedSnack(removed))));
  }
}

/// Local hosting spawns a local `codex` binary + child processes — desktop only.
bool get _hostingSupported =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.linux);

/// Confirm, then take a service's tunnel off the relay. For one of *our* local
/// hosts ([localTunnel] set) this is a reversible unpublish — the codex / API
/// proxy keep running and the 本地托管 card can re-register it. For someone
/// else's service it asks the backend to force-drop the relay key (best-effort —
/// a still-running host re-registers). The key is hidden at once via
/// [pendingRemovalProvider].
///
/// [unreachable] marks a non-local entry whose backend isn't responding — an
/// orphaned/hollow registration lingering on the relay. The backend can't drop
/// such a key (nothing live holds it to cancel), so we ALSO durably dismiss it
/// via [dismissedServicesProvider], making "注销" actually remove it from this
/// device's list and keep it gone across restarts.
Future<void> _confirmDeregister(
  BuildContext context,
  WidgetRef ref,
  ServiceEntry s, {
  ({String name, String kind})? localTunnel,
  bool unreachable = false,
}) async {
  final l10n = AppLocalizations.of(context);
  final scheme = Theme.of(context).colorScheme;
  final isLocal = localTunnel != null;
  // A non-local entry that isn't responding is orphaned: use the honest
  // "remove from your list" wording instead of the "stop that host" wording,
  // which doesn't apply when no reachable host exists.
  final isOrphan = unreachable && !isLocal;
  final ok = await showDialog<bool>(
    context: context,
    builder: (_) => AlertDialog(
      key: const Key('deregister-dialog'),
      title: Text(isOrphan ? l10n.deregisterOrphanTitle : l10n.deregisterTitle),
      content: Text(
        isLocal
            ? l10n.deregisterLocalWarning(s.name)
            : isOrphan
            ? l10n.deregisterOrphanWarning(s.name)
            : l10n.deregisterWarning(s.name),
      ),
      actions: [
        TextButton(
          onPressed: () => Navigator.of(context).pop(false),
          child: Text(l10n.cancel),
        ),
        FilledButton(
          key: const Key('deregister-confirm-btn'),
          style: FilledButton.styleFrom(backgroundColor: scheme.error),
          onPressed: () => Navigator.of(context).pop(true),
          child: Text(isOrphan ? l10n.remove : l10n.deregister),
        ),
      ],
    ),
  );
  if (ok != true) return;
  try {
    if (localTunnel != null) {
      // Reversible: stop this tunnel's register task (codex/proxy keep running);
      // serve_deregister also best-effort force-drops the relay key. Our own
      // tunnel reliably leaves discovery, so optimistically hide it at once.
      await ref
          .read(bridgeApiProvider)
          .appServeDeregister(name: localTunnel.name, kind: localTunnel.kind);
      ref
          .read(pendingRemovalProvider.notifier)
          .update((set) => {...set, s.key});
      ref.invalidate(localServeListProvider);
    } else if (isOrphan) {
      // Orphaned/hollow: nothing live holds the relay key, so the backend can't
      // drop it. Durably dismiss it so it leaves this device's list and stays
      // gone; still best-effort ask the backend to drop it (swallow errors — the
      // dismissal already achieved the user-visible removal).
      //
      // Re-check reachability at confirm time: it may have recovered while the
      // dialog was open. If it's live again, don't hide it — only best-effort
      // drop — so a now-working service isn't stranded off the list.
      final reachableNow =
          (s.kind == 'app'
                  ? ref.read(appReachableProvider(s.key))
                  : ref.read(apiReachableProvider(s.key)))
              .valueOrNull ==
          true;
      if (!reachableNow) {
        ref.read(dismissedServicesProvider.notifier).dismiss(s.key);
      }
      try {
        await ref
            .read(bridgeApiProvider)
            .accountDeregisterService(
              device: s.device,
              kind: s.kind,
              name: s.name,
            );
      } catch (_) {
        // Best-effort — the entry is already hidden from the list.
      }
    } else {
      // Someone else's LIVE service: best-effort ask the backend to drop the
      // relay key. Do NOT durably hide it — a still-running host re-registers
      // within seconds, and hiding would strand a live service off the list.
      await ref
          .read(bridgeApiProvider)
          .accountDeregisterService(
            device: s.device,
            kind: s.kind,
            name: s.name,
          );
    }
    ref.invalidate(servicesProvider);
  } catch (e) {
    if (context.mounted) {
      ScaffoldMessenger.of(context).showSnackBar(
        SnackBar(
          content: Text('${l10n.deregisterFailed}: ${friendlyError(e)}'),
        ),
      );
    }
  }
}

/// The connection identity: in account mode the signed-in GitHub login; in
/// self-host mode the configured relay. Status is implicitly online — this
/// only renders once discovery (which needs a valid session/relay) succeeded.
/// A quiet outlined panel, not a hero: status colour is the only accent.
class _IconBadge extends StatelessWidget {
  const _IconBadge({required this.icon, required this.bg, required this.fg});
  final IconData icon;
  final Color bg;
  final Color fg;

  @override
  Widget build(BuildContext context) => Container(
    width: 40,
    height: 40,
    decoration: BoxDecoration(
      color: bg,
      borderRadius: BorderRadius.circular(10),
    ),
    child: Icon(icon, size: 20, color: fg),
  );
}

/// One locally-hosted host: a codex app-server + an in-app API proxy, each
/// published through its own tunnel. The card shows codex's liveness and both
/// tunnels' publish state, with a per-tunnel 注销 / 重新注册 toggle. Tapping the
/// header opens [LocalHostDialog] for the full 停止托管 + details.
class _LocalHostCard extends ConsumerWidget {
  const _LocalHostCard({super.key, required this.host});

  final AppServeStatus host;

  /// Synthesize the discovery entry for one of this host's tunnels, so the
  /// shared [_confirmDeregister] flow (confirm + optimistic hide) can run.
  String _keyFor(String kind) => switch (kind) {
    'api' => host.apiServiceKey,
    'meta' => host.metaServiceKey,
    _ => host.appServiceKey,
  };

  ServiceEntry _entry(String kind) => ServiceEntry(
    device: host.device,
    kind: kind,
    name: host.name,
    key: _keyFor(kind),
  );

  Future<void> _reregister(
    BuildContext context,
    WidgetRef ref,
    String kind,
  ) async {
    final key = _keyFor(kind);
    final l10n = AppLocalizations.of(context);
    final messenger = ScaffoldMessenger.of(context);
    try {
      await ref
          .read(bridgeApiProvider)
          .appServeReregister(name: host.name, kind: kind);
    } catch (e) {
      // Surfaces a duplicate-name refusal (another live instance took the
      // name while this tunnel was down) as guidance instead of silence.
      final raw = friendlyError(e);
      messenger.showSnackBar(
        SnackBar(
          content: Text(isHostNameConflict(raw) ? l10n.hostNameConflict : raw),
        ),
      );
      return;
    }
    // Make sure it isn't still optimistically hidden, then re-discover.
    ref
        .read(pendingRemovalProvider.notifier)
        .update((s) => s.difference({key}));
    ref.invalidate(localServeListProvider);
    ref.invalidate(servicesProvider);
  }

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final online = successColor(scheme);
    // Honest host health. `host.alive` is only port-open (the listener bound),
    // which stays true even when the embedded codex has wedged / gone half-open
    // (still accept()ing but never answering RPC) — a false "hosting". So once
    // the listener is up, the real signal is a loopback `initialize` handshake
    // (appReachableLocalProvider, no relay hop): answers → 托管中, listening but
    // silent → 无响应. Before the port is even open it is genuinely starting.
    final StatusChip codexChip;
    if (!host.alive) {
      codexChip = StatusChip(
        color: scheme.tertiary,
        label: l10n.localHostStarting,
        filled: true,
      );
    } else {
      codexChip = ref
          .watch(appReachableLocalProvider(host.appListenAddr))
          .when(
            data: (ok) => ok
                ? StatusChip(
                    color: online,
                    label: l10n.localHostRunning,
                    filled: true,
                  )
                : StatusChip(
                    color: scheme.error,
                    label: l10n.localHostUnresponsive,
                    filled: true,
                  ),
            loading: () => StatusChip(
              color: scheme.tertiary,
              label: l10n.localHostStarting,
              filled: true,
            ),
            error: (_, _) => StatusChip(
              color: scheme.error,
              label: l10n.localHostUnresponsive,
              filled: true,
            ),
          );
    }
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: Material(
        color: surfacePanel(scheme),
        borderRadius: BorderRadius.circular(12),
        // The whole card opens the host dialog (stop + details); the per-tunnel
        // buttons inside absorb their own taps.
        child: InkWell(
          mouseCursor: clickable,
          borderRadius: BorderRadius.circular(12),
          onTap: () => showDialog<void>(
            context: context,
            builder: (_) => LocalHostDialog(existing: host),
          ),
          child: Container(
            decoration: BoxDecoration(
              borderRadius: BorderRadius.circular(12),
              border: Border.all(color: scheme.outlineVariant),
            ),
            child: Column(
              children: [
                Padding(
                  padding: const EdgeInsets.all(12),
                  child: Row(
                    children: [
                      _IconBadge(
                        icon: Icons.dns,
                        bg: scheme.tertiaryContainer,
                        fg: scheme.onTertiaryContainer,
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              host.name,
                              style: Theme.of(context).textTheme.titleSmall,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                            const SizedBox(height: 2),
                            Text(
                              host.device,
                              style: Theme.of(context).textTheme.bodySmall
                                  ?.copyWith(color: scheme.onSurfaceVariant),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                          ],
                        ),
                      ),
                      const SizedBox(width: 8),
                      codexChip,
                      const SizedBox(width: 2),
                      Icon(Icons.chevron_right, color: scheme.outline),
                    ],
                  ),
                ),
                Divider(height: 1, color: scheme.outlineVariant),
                _TunnelRow(
                  label: l10n.tunnelAppLabel,
                  addr: host.appListenAddr,
                  registered: host.appRegistered,
                  onDeregister: () => _confirmDeregister(
                    context,
                    ref,
                    _entry('app'),
                    localTunnel: (name: host.name, kind: 'app'),
                  ),
                  onReregister: () => _reregister(context, ref, 'app'),
                ),
                Divider(height: 1, color: scheme.outlineVariant),
                _TunnelRow(
                  label: l10n.tunnelApiLabel,
                  addr: host.apiListenAddr,
                  registered: host.apiRegistered,
                  onDeregister: () => _confirmDeregister(
                    context,
                    ref,
                    _entry('api'),
                    localTunnel: (name: host.name, kind: 'api'),
                  ),
                  onReregister: () => _reregister(context, ref, 'api'),
                ),
                Divider(height: 1, color: scheme.outlineVariant),
                _TunnelRow(
                  label: l10n.tunnelMetaLabel,
                  addr: host.metaListenAddr,
                  registered: host.metaRegistered,
                  onDeregister: () => _confirmDeregister(
                    context,
                    ref,
                    _entry('meta'),
                    localTunnel: (name: host.name, kind: 'meta'),
                  ),
                  onReregister: () => _reregister(context, ref, 'meta'),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// One tunnel row inside a [_LocalHostCard]: kind label, listen address, a
/// published/offline pill, and a 注销 (when published) / 重新注册 (when offline)
/// toggle.
class _TunnelRow extends StatelessWidget {
  const _TunnelRow({
    required this.label,
    required this.addr,
    required this.registered,
    required this.onDeregister,
    required this.onReregister,
  });

  final String label;
  final String addr;
  final bool registered;
  final VoidCallback onDeregister;
  final VoidCallback onReregister;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final online = successColor(scheme);
    return Padding(
      padding: const EdgeInsets.fromLTRB(12, 6, 6, 6),
      child: Row(
        children: [
          Icon(
            registered ? Icons.cloud_done_outlined : Icons.cloud_off_outlined,
            size: 18,
            color: registered ? online : scheme.outline,
          ),
          const SizedBox(width: 8),
          SizedBox(
            width: 78,
            child: Text(
              label,
              style: Theme.of(context).textTheme.labelMedium,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          Expanded(
            child: Text(
              registered ? addr : l10n.tunnelOffline,
              style: Theme.of(context).textTheme.bodySmall?.copyWith(
                color: registered ? scheme.onSurfaceVariant : scheme.outline,
              ),
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
            ),
          ),
          if (registered)
            TextButton(
              key: Key('tunnel-deregister-$label'),
              onPressed: onDeregister,
              style: TextButton.styleFrom(foregroundColor: scheme.error),
              child: Text(l10n.deregister),
            )
          else
            TextButton(
              key: Key('tunnel-reregister-$label'),
              onPressed: onReregister,
              child: Text(l10n.reregister),
            ),
        ],
      ),
    );
  }
}

/// The "+ host another" entry that opens [LocalHostDialog] in new-host mode.
class _AddLocalHostCard extends StatelessWidget {
  const _AddLocalHostCard();

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 4),
      child: OutlinedButton.icon(
        key: const Key('add-local-host-card'),
        onPressed: () => showDialog<void>(
          context: context,
          builder: (_) => const LocalHostDialog(),
        ),
        icon: const Icon(Icons.add),
        label: Text(l10n.addLocalHost),
        style: OutlinedButton.styleFrom(
          minimumSize: const Size.fromHeight(44),
          alignment: Alignment.centerLeft,
        ),
      ),
    );
  }
}

class _ErrorState extends StatelessWidget {
  const _ErrorState({
    required this.detail,
    required this.sessionExpired,
    required this.onRetry,
    this.onSignIn,
  });

  /// Raw engine error string, shown only for retryable failures.
  final String detail;
  final bool sessionExpired;
  final VoidCallback onRetry;
  final VoidCallback? onSignIn;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Center(
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          Text(
            sessionExpired
                ? l10n.accountSessionExpiredTitle
                : l10n.discoverFailed,
            key: const Key('services-error'),
            textAlign: TextAlign.center,
          ),
          const SizedBox(height: 4),
          Padding(
            padding: const EdgeInsets.symmetric(horizontal: 24),
            child: Text(
              sessionExpired ? l10n.accountSessionExpiredMessage : detail,
              textAlign: TextAlign.center,
              style: Theme.of(context).textTheme.bodySmall,
            ),
          ),
          const SizedBox(height: 12),
          if (sessionExpired)
            FilledButton.icon(
              key: const Key('services-sign-in-again'),
              onPressed: onSignIn,
              icon: const Icon(Icons.login),
              label: Text(l10n.accountSignInAgain),
            )
          else
            FilledButton(onPressed: onRetry, child: Text(l10n.retry)),
        ],
      ),
    );
  }
}
