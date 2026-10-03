import 'package:flutter/foundation.dart';

/// Whether this platform can host services (spawning local agent processes):
/// desktop only, never the web.
bool hostingSupportedPlatform() =>
    !kIsWeb &&
    (defaultTargetPlatform == TargetPlatform.windows ||
        defaultTargetPlatform == TargetPlatform.macOS ||
        defaultTargetPlatform == TargetPlatform.linux);
