# Pocket-Codex Flutter front-end

Native Flutter UI for [Pocket-Codex](../../README.md). Talks to the Rust
workspace through [`flutter_rust_bridge`][frb] so every platform can
drive a `codex app-server` — or the host's Responses API proxy — without
re-implementing the model runtime.

> [!WARNING]
> **Bootstrap-quality.** Today the app only mounts a single screen that
> exercises the Rust ↔ Dart bridge. Real flows (thread management,
> service selection, pb-mapper session control) land with the milestones
> in the project [roadmap](../../AGENTS.md#9-roadmap-rough).

## Toolchain

Flutter is pinned in three lockstep places — bump them together:

- `apps/flutter/.fvmrc` (project-level Flutter version, consumed by
  [FVM](https://fvm.app/))
- `apps/flutter/pubspec.yaml` → `environment.flutter`
- CI `subosito/flutter-action@v2` configuration

Today the pin is **Flutter 3.44.0** with Dart SDK `^3.12.0`.

## Getting started

```bash
# One-time: install FVM and the pinned Flutter version.
brew tap leoafarias/fvm && brew install fvm
fvm install 3.44.0 --setup

# Day-to-day:
cd apps/flutter
fvm flutter pub get
fvm flutter analyze
fvm flutter test
fvm flutter run     # boot the placeholder UI on a connected device
```

The app depends on the `pocket_codex_bridge` Dart package, which wraps
the Rust cdylib built from
[`crates/pocket-codex-bridge`](../../crates/pocket-codex-bridge).
Generated bindings live under `lib/src/rust/` and must not be
hand-edited.

## Regenerating the bridge

After editing anything under `crates/pocket-codex-bridge/src/api/`,
re-run the codegen from the repo root:

```bash
flutter_rust_bridge_codegen generate
```

Codegen configuration lives in
[`flutter_rust_bridge.yaml`](flutter_rust_bridge.yaml).

## See also

- [Project README](../../README.md) — overall positioning and CLI flows
- [`AGENTS.md`](../../AGENTS.md) — engineering principles, verification
  commands, roadmap

[frb]: https://cjycode.com/flutter_rust_bridge/
