/// This is copied from Cargokit (which is the official way to use it currently)
/// Details: https://fzyzcjy.github.io/flutter_rust_bridge/manual/integrate/builtin

import 'dart:io';

import 'package:collection/collection.dart';
import 'package:logging/logging.dart';
import 'package:path/path.dart' as path;

import 'android_environment.dart';
import 'cargo.dart';
import 'environment.dart';
import 'options.dart';
import 'rustup.dart';
import 'target.dart';
import 'util.dart';

final _log = Logger('builder');

/// (pocket-codex patch) Reads the `channel` from the nearest `rust-toolchain.toml`
/// (or `rust-toolchain`), walking up from [startDir], so cargokit builds with the
/// repo-pinned toolchain that `rustup` auto-installs. Returns null if none found.
String? _rustToolchainChannel(String startDir) {
  final channelRe = RegExp(r'channel\s*=\s*"([^"]+)"');
  var dir = Directory(startDir).absolute;
  while (true) {
    for (final name in const ['rust-toolchain.toml', 'rust-toolchain']) {
      final file = File(path.join(dir.path, name));
      if (file.existsSync()) {
        final match = channelRe.firstMatch(file.readAsStringSync());
        if (match != null) {
          return match.group(1);
        }
      }
    }
    final parent = dir.parent;
    if (parent.path == dir.path) return null; // reached the filesystem root
    dir = parent;
  }
}

enum BuildConfiguration {
  debug,
  release,
  profile,
}

extension on BuildConfiguration {
  bool get isDebug => this == BuildConfiguration.debug;
  String get rustName => switch (this) {
        BuildConfiguration.debug => 'debug',
        BuildConfiguration.release => 'release',
        BuildConfiguration.profile => 'release',
      };
}

class BuildException implements Exception {
  final String message;

  BuildException(this.message);

  @override
  String toString() {
    return 'BuildException: $message';
  }
}

class BuildEnvironment {
  final BuildConfiguration configuration;
  final CargokitCrateOptions crateOptions;
  final String targetTempDir;
  final String manifestDir;
  final CrateInfo crateInfo;

  final bool isAndroid;
  final String? androidSdkPath;
  final String? androidNdkVersion;
  final int? androidMinSdkVersion;
  final String? javaHome;

  final String? glibcVersion;

  BuildEnvironment({
    required this.configuration,
    required this.crateOptions,
    required this.targetTempDir,
    required this.manifestDir,
    required this.crateInfo,
    required this.isAndroid,
    this.androidSdkPath,
    this.androidNdkVersion,
    this.androidMinSdkVersion,
    this.javaHome,
    this.glibcVersion,
  });

  static BuildConfiguration parseBuildConfiguration(String value) {
    // XCode configuration adds the flavor to configuration name.
    final firstSegment = value.split('-').first;
    final buildConfiguration = BuildConfiguration.values.firstWhereOrNull(
      (e) => e.name == firstSegment,
    );
    if (buildConfiguration == null) {
      _log.warning('Unknown build configuraiton $value, will assume release');
      return BuildConfiguration.release;
    }
    return buildConfiguration;
  }

  static BuildEnvironment fromEnvironment({
    required bool isAndroid,
  }) {
    final buildConfiguration =
        parseBuildConfiguration(Environment.configuration);
    final manifestDir = Environment.manifestDir;
    final crateOptions = CargokitCrateOptions.load(
      manifestDir: manifestDir,
    );
    final crateInfo = CrateInfo.load(manifestDir);
    return BuildEnvironment(
      configuration: buildConfiguration,
      crateOptions: crateOptions,
      targetTempDir: Environment.targetTempDir,
      manifestDir: manifestDir,
      crateInfo: crateInfo,
      isAndroid: isAndroid,
      androidSdkPath: isAndroid ? Environment.sdkPath : null,
      androidNdkVersion: isAndroid ? Environment.ndkVersion : null,
      androidMinSdkVersion:
          isAndroid ? int.parse(Environment.minSdkVersion) : null,
      javaHome: isAndroid ? Environment.javaHome : null,
    );
  }
}

class RustBuilder {
  final Target target;
  final BuildEnvironment environment;

  RustBuilder({
    required this.target,
    required this.environment,
  });

  void prepare(
    Rustup rustup,
  ) {
    final toolchain = _toolchain;
    if (rustup.installedTargets(toolchain) == null) {
      rustup.installToolchain(toolchain);
    }
    if (toolchain.startsWith('nightly')) {
      rustup.installRustSrcForNightly();
    }
    if (!rustup.installedTargets(toolchain)!.contains(target.rust)) {
      rustup.installTarget(target.rust, toolchain: toolchain);
    }
    if (environment.glibcVersion != null) {
      rustup.installZigBuild(toolchain);
    }
  }

  CargoBuildOptions? get _buildOptions =>
      environment.crateOptions.cargo[environment.configuration];

  /// The toolchain cargokit builds the crate with. Precedence:
  ///   1. an explicit `toolchain:` in cargokit_options.yaml, else
  ///   2. the `channel` pinned in the nearest `rust-toolchain.toml` (walking up
  ///      from the crate), else
  ///   3. `stable`.
  ///
  /// (pocket-codex patch) Upstream cargokit always runs `rustup run stable`,
  /// which silently ignores `rust-toolchain.toml` and uses whatever `stable` the
  /// developer happens to have installed. Our embedded codex pins a minimum
  /// rustc (`deps/codex/codex-rs/rust-toolchain.toml`), so honouring the repo's
  /// pinned channel here lets `rustup` auto-install the exact toolchain — a
  /// one-click `flutter build` — instead of failing on a stale `stable`.
  String get _toolchain =>
      _buildOptions?.toolchain.name ??
      _rustToolchainChannel(environment.manifestDir) ??
      'stable';

  /// Returns the path of directory containing build artifacts.
  Future<String> build() async {
    final extraArgs = _buildOptions?.flags ?? [];
    final manifestPath = path.join(environment.manifestDir, 'Cargo.toml');
    runCommand(
      'rustup',
      [
        'run',
        _toolchain,
        'cargo',
        (target.android == null && environment.glibcVersion != null)
            ? 'zigbuild'
            : 'build',
        ...extraArgs,
        '--manifest-path',
        manifestPath,
        '-p',
        environment.crateInfo.packageName,
        if (!environment.configuration.isDebug) '--release',
        '--target',
        target.rust +
            ((target.android == null && environment.glibcVersion != null)
                ? '.${environment.glibcVersion!}'
                : ""),
        '--target-dir',
        environment.targetTempDir,
      ],
      environment: await _buildEnvironment(),
    );
    return path.join(
      environment.targetTempDir,
      target.rust,
      environment.configuration.rustName,
    );
  }

  Future<Map<String, String>> _buildEnvironment() async {
    if (target.android == null) {
      return {};
    } else {
      final sdkPath = environment.androidSdkPath;
      final ndkVersion = environment.androidNdkVersion;
      final minSdkVersion = environment.androidMinSdkVersion;
      if (sdkPath == null) {
        throw BuildException('androidSdkPath is not set');
      }
      if (ndkVersion == null) {
        throw BuildException('androidNdkVersion is not set');
      }
      if (minSdkVersion == null) {
        throw BuildException('androidMinSdkVersion is not set');
      }
      final env = AndroidEnvironment(
        sdkPath: sdkPath,
        ndkVersion: ndkVersion,
        minSdkVersion: minSdkVersion,
        targetTempDir: environment.targetTempDir,
        target: target,
      );
      if (!env.ndkIsInstalled() && environment.javaHome != null) {
        env.installNdk(javaHome: environment.javaHome!);
      }
      return env.buildEnvironment();
    }
  }
}
