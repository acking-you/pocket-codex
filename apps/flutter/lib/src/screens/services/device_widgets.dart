part of '../services_screen.dart';

class _DeviceColumn extends StatelessWidget {
  const _DeviceColumn({
    required this.relay,
    required this.accountLogin,
    required this.accountId,
    required this.devices,
    required this.activeDevice,
    required this.localDevices,
    required this.localMetadata,
    required this.selectedMetadata,
    required this.capabilityCount,
    required this.onSelect,
    required this.filled,
    this.relayDown = false,
  });

  final String relay;
  final String? accountLogin;
  final String? accountId;
  final List<String> devices;
  final String? activeDevice;
  final Set<String> localDevices;
  final DeviceMetadata? localMetadata;
  final DeviceMetadata? selectedMetadata;
  final int Function(String device) capabilityCount;
  final ValueChanged<String> onSelect;

  /// Whether to take the height it is given and scroll the device list inside.
  /// False sizes to the list instead, for a column that is the whole page.
  final bool filled;

  /// The relay could not be reached.
  final bool relayDown;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    final scheme = Theme.of(context).colorScheme;
    final online = successColor(scheme);
    final tiles = <Widget>[
      for (final device in devices)
        _DeviceTile(
          key: Key('device-$device'),
          device: device == _thisDevice
              ? (localMetadata?.hostname ?? l10n.servicesLocalDevice)
              : deviceLabel(device),
          subtitle: [
            if (localDevices.contains(device)) l10n.servicesLocalDevice,
            if (localDevices.contains(device) && localMetadata != null)
              localMetadata!.instanceLabel,
            if (!localDevices.contains(device) &&
                device == activeDevice &&
                selectedMetadata != null)
              selectedMetadata!.instanceLabel,
            l10n.servicesDeviceCapabilityCount(capabilityCount(device)),
          ].join(' · '),
          selected: device == activeDevice,
          onTap: () => onSelect(device),
        ),
    ];
    // The relay (or account) this list comes from, as a small raised card;
    // the devices below it are rows on the sheet itself, the chosen one
    // lifted into a card of its own — one level of plates, not a slab.
    final identity = DecoratedBox(
      decoration: BoxDecoration(
        borderRadius: BorderRadius.circular(kCardRadius),
        boxShadow: raisedCardShadows(scheme),
      ),
      child: Card(
        clipBehavior: Clip.antiAlias,
        child: Tooltip(
          message: l10n.settingsTitle,
          child: InkWell(
            key: const Key('identity-open-settings'),
            mouseCursor: clickable,
            onTap: () => context.push('/settings'),
            child: LayoutBuilder(
              builder: (context, constraints) {
                final identity = Text(
                  accountLogin == null ? relay : '@$accountLogin',
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(fontWeight: FontWeight.w600),
                );
                final label = relayDown
                    ? l10n.statusUnreachable
                    : l10n.statusOnline;
                final status = StatusChip(
                  color: relayDown ? scheme.error : online,
                  label: label,
                  filled: true,
                );
                // The status sits beside the identity while both stay
                // readable there, otherwise under it.
                final beside =
                    constraints.maxWidth >= _identityRowWidth(context, label);
                return Padding(
                  padding: const EdgeInsets.all(_identityPadding),
                  child: Row(
                    children: [
                      GitHubAvatar(
                        accountId: accountId,
                        fallbackIcon: accountLogin == null
                            ? AppIcons.lan_outlined
                            : AppIcons.person_outline,
                        size: _avatarSize,
                      ),
                      const SizedBox(width: _avatarGap),
                      if (beside) ...[
                        Expanded(child: identity),
                        status,
                      ] else
                        Expanded(
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              identity,
                              const SizedBox(height: 4),
                              status,
                            ],
                          ),
                        ),
                      Icon(
                        AppIcons.chevron_right,
                        size: _chevronSize,
                        color: scheme.outline,
                      ),
                    ],
                  ),
                );
              },
            ),
          ),
        ),
      ),
    );
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      mainAxisSize: filled ? MainAxisSize.max : MainAxisSize.min,
      children: [
        identity,
        Padding(
          padding: const EdgeInsets.fromLTRB(14, 20, 14, 8),
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
            child: ListView(padding: EdgeInsets.zero, children: tiles),
          )
        else
          Column(children: tiles),
      ],
    );
  }
}

// The identity row's fixed parts.
const double _identityPadding = 12;
const double _avatarSize = 36;
const double _avatarGap = 10;
const double _chevronSize = 18;

/// The width the identity row needs to show the whole [status] chip beside
/// at least three glyphs of the identity, at the current text scale. The chip's
/// own geometry (10 px padding a side, an 8 px dot and a 6 px gap) is that of
/// a filled [StatusChip].
double _identityRowWidth(BuildContext context, String status) {
  final scaler = MediaQuery.textScalerOf(context);
  final label = measureTextLine(
    context,
    status,
    const TextStyle(
      fontSize: 11.5,
      fontWeight: FontWeight.w600,
      letterSpacing: 0.1,
    ),
  );
  final chip = 10 + 8 + 6 + label.width + 10;
  return 2 * _identityPadding +
      _avatarSize +
      _avatarGap +
      scaler.scale(14) * 3 +
      chip +
      _chevronSize;
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
        // The chosen device is lifted into a card of its own; the rest are
        // rows on the sheet.
        color: selected ? raisedCardColor(scheme) : Colors.transparent,
        elevation: selected ? 1 : 0,
        shadowColor: sidebarSelectionShadow(scheme),
        animationDuration: Motion.of(context, Motion.fast),
        shape: RoundedRectangleBorder(
          borderRadius: BorderRadius.circular(kRowRadius),
          side: selected
              ? BorderSide(color: raisedCardEdge(scheme))
              : BorderSide.none,
        ),
        child: InkWell(
          mouseCursor: clickable,
          onTap: onTap,
          borderRadius: BorderRadius.circular(kRowRadius),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 10),
            child: Row(
              children: [
                Container(
                  width: 32,
                  height: 32,
                  decoration: BoxDecoration(
                    color: softWash(scheme),
                    shape: BoxShape.circle,
                  ),
                  child: Icon(
                    AppIcons.computer,
                    size: 17,
                    color: selected
                        ? scheme.onSurface
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
                          color: scheme.onSurface,
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
    this.platform,
  });

  final String? device;
  final bool local;
  final bool isDefault;
  final VoidCallback? onClean;
  final String? platform;

  @override
  Widget build(BuildContext context) {
    final l10n = AppLocalizations.of(context);
    return Padding(
      padding: const EdgeInsets.only(bottom: 12),
      child: _rowOrStacked(
        context,
        // The device list beside it already carries the device's glyph;
        // the heading is its name alone.
        leading: const [SizedBox(width: 4)],
        body: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              device ?? l10n.servicesDevices,
              style: Theme.of(
                context,
              ).textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w600),
            ),
            if (device != null)
              Text(
                platform ?? l10n.servicesPlatformUnknown,
                key: const Key('device-platform'),
                style: Theme.of(context).textTheme.bodySmall?.copyWith(
                  color: Theme.of(context).colorScheme.onSurfaceVariant,
                ),
              ),
          ],
        ),
        trailing: [
          if (local) _CountPill(label: l10n.servicesLocalDevice),
          if (local && isDefault) const SizedBox(width: 8),
          if (isDefault) _CountPill(label: l10n.servicesDefault, accent: true),
          if (onClean != null) ...[
            const SizedBox(width: 8),
            TextButton.icon(
              key: const Key('device-clean-unreachable'),
              onPressed: onClean,
              icon: const Icon(AppIcons.cleaning_services_outlined, size: 17),
              label: Text(l10n.batchRemoveEnter),
            ),
          ],
        ],
      ),
    );
  }
}
