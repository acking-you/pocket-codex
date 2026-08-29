import 'package:flutter/material.dart';

/// When an account id's avatar fetch last failed. Flutter's image cache does not
/// remember failures, so without this a blocked or offline fetch is re-requested
/// on every rebuild — and the identity row rebuilds with every services refresh.
/// Entries expire because the app runs on phones and flaky networks: one bad
/// moment must not strand the avatar for the whole session.
final Map<String, DateTime> _avatarMisses = {};
const _avatarRetryAfter = Duration(minutes: 2);

/// Forget every recorded failure. For tests: the map outlives a widget tree, so
/// without this one test's failed id changes what a later test renders.
@visibleForTesting
void debugResetAvatarMisses() => _avatarMisses.clear();

/// A signed-in user's GitHub avatar, falling back to [fallbackIcon] whenever it
/// can't be shown.
///
/// The URL is derived from the numeric account id
/// (`avatars.githubusercontent.com/u/<id>`) rather than fetched as a profile
/// field — the id is already persisted with the session, so the avatar costs no
/// extra round trip to our own backend. It does mean one request to GitHub; the
/// fallback covers a blocked network, and nothing waits on it.
class GitHubAvatar extends StatelessWidget {
  /// Creates an avatar for [accountId], or a bare [fallbackIcon] without one.
  const GitHubAvatar({
    super.key,
    required this.accountId,
    required this.fallbackIcon,
    this.size = 36,
  });

  /// GitHub numeric account id. Null (or blank) renders the fallback: a
  /// self-hosted user has no GitHub identity to show.
  final String? accountId;

  /// Shown until the image arrives, and kept if it never does.
  final IconData fallbackIcon;

  /// Diameter in logical pixels. The glyph and the requested image are sized
  /// from it, so one number keeps a 32px row and a 42px header in step.
  final double size;

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    final id = accountId?.trim() ?? '';
    final fallback = Icon(
      fallbackIcon,
      size: size * _glyphRatio,
      color: scheme.onPrimaryContainer,
    );

    Widget content = fallback;
    if (id.isNotEmpty && !_recentlyFailed(id)) {
      // Ask GitHub for the size actually being drawn, and bound the decode to
      // the same box: the source is a 460px PNG, and holding that in the image
      // cache for a 36px slot wastes most of a megabyte per account.
      final pixels = (size * MediaQuery.devicePixelRatioOf(context)).round();
      content = Image.network(
        'https://avatars.githubusercontent.com/u/$id?s=$pixels',
        width: size,
        height: size,
        fit: BoxFit.cover,
        cacheWidth: pixels,
        errorBuilder: (context, error, stack) {
          _avatarMisses[id] = DateTime.now();
          return fallback;
        },
        // The glyph holds the slot until a frame is ready, so a slow fetch never
        // leaves a hole where the identity should be.
        frameBuilder: (context, child, frame, wasSynchronouslyLoaded) =>
            frame == null && !wasSynchronouslyLoaded ? fallback : child,
      );
    }

    return Container(
      width: size,
      height: size,
      clipBehavior: Clip.antiAlias,
      alignment: Alignment.center,
      decoration: BoxDecoration(
        color: scheme.primaryContainer,
        shape: BoxShape.circle,
      ),
      child: content,
    );
  }

  /// The glyph sits at half the circle, matching the icon-in-a-tile proportion
  /// the service rows already use (18 in a 36 box).
  static const double _glyphRatio = 0.5;

  static bool _recentlyFailed(String id) {
    final missedAt = _avatarMisses[id];
    if (missedAt == null) return false;
    if (DateTime.now().difference(missedAt) < _avatarRetryAfter) return true;
    _avatarMisses.remove(id);
    return false;
  }
}
