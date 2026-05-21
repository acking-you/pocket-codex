<p align="center">
  <img src="assets/logo/poster.png" alt="Pocket-Codex poster" width="100%" />
</p>

<h1 align="center">Pocket-Codex</h1>

<p align="center">
  <em>Carry your Codex in your pocket. Drive it natively from any device.</em>
</p>

<p align="center">
  <a href="#status"><img alt="status: work in progress" src="https://img.shields.io/badge/status-WIP-orange"></a>
  <a href="https://www.rust-lang.org"><img alt="rust" src="https://img.shields.io/badge/built%20with-Rust-dea584.svg"></a>
  <a href="https://flutter.dev"><img alt="flutter" src="https://img.shields.io/badge/UI-Flutter-02569B.svg"></a>
  <a href="LICENSE"><img alt="license" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
</p>

> [!WARNING]
> **Pocket-Codex is under active development.** Nothing here is stable
> yet — APIs, on-disk layout, the protocol mapping and even the crate
> boundaries are expected to change without notice. Do **not** depend on
> it for production workloads. Pull requests, design feedback and bug
> reports are very welcome while we hammer out the foundations.

## What is this?

Pocket-Codex is an experiment in turning the upstream
[`codex app-server`](https://github.com/openai/codex) protocol into a
portable, multi-device experience:

- A pure-Rust CLI manages a local `codex app-server` process on the
  machine where Codex is installed.
- The same CLI uses [`pb-mapper`](https://github.com/acking-you/pb-mapper)
  to register that local app-server with a relay, so any other device
  can subscribe and reach it.
- A Flutter front-end (driven through `flutter_rust_bridge`) consumes
  the app-server JSON-RPC protocol directly, giving every platform a
  native UI for Codex without re-implementing model logic.

In short: **the heavy lifting stays on the machine that already has
Codex set up; the UI runs wherever you are.**

## Status

| Area                           | State                                  |
| ------------------------------ | -------------------------------------- |
| Workspace / lints / CI         | bootstrapped                           |
| `pocket-codex` CLI             | `serve`, `connect`, top-level `status`/`stop`, `codex {start,stop,status}`, `pb {register,subscribe,status}`, `remote-hint`, `version` |
| `pb-mapper` register/subscribe | wired through `deps/pb-mapper`         |
| `codex app-server` supervision | spawn/stop/status via PID + state.toml |
| Flutter UI (`apps/flutter`)    | placeholder screen + FRB sample bridge |

The first usable milestone is now the high-level CLI pair:

- `pocket-codex serve --relay <host:port>` starts or reuses the local
  `codex app-server`, registers it with the relay and prints the
  matching client-side command.
- `pocket-codex connect --relay <host:port>` subscribes to the remote
  app-server, exposes it locally and prints the exact
  `codex --remote ...` invocation to start Codex against that listener.

See [`AGENTS.md`](AGENTS.md) for the detailed roadmap and contributor
conventions.

## Repository layout

```
pocket-codex/
├── apps/
│   └── flutter/                 # Flutter UI (FRB-driven, FVM-locked)
├── assets/
│   └── logo/                    # Project artwork (poster, logo)
├── crates/
│   ├── pocket-codex-core        # shared types, config, state, paths
│   ├── pocket-codex-codex       # codex app-server process manager
│   ├── pocket-codex-pb          # pb-mapper register/subscribe glue
│   ├── pocket-codex-cli         # `pocket-codex` binary
│   └── pocket-codex-bridge      # cdylib consumed by flutter_rust_bridge
├── deps/
│   ├── codex/                   # upstream codex (git submodule)
│   ├── pb-mapper/               # upstream pb-mapper (git submodule)
│   ├── kanal/                   # pinned fork transitively used by pb-mapper
│   └── uni-stream/              # pinned fork transitively used by pb-mapper
├── docs/                        # design notes & protocol references
└── skills/                      # contributor / agent skill packs
```

## Getting started

> Heads up: this is bootstrap-quality. CLI flags, on-disk state,
> protocol coverage and UI surface area are all expected to change.

### Rust workspace

```bash
# Clone with all submodules (deps/codex, pb-mapper, kanal, uni-stream).
git clone --recurse-submodules git@github.com:acking-you/pocket-codex.git
cd pocket-codex

# If you cloned without --recurse-submodules:
git submodule update --init --recursive

# Build everything in the workspace.
cargo build --workspace

# Inspect the CLI surface.
cargo run -p pocket-codex-cli -- --help
```

A working `codex` binary is expected to exist on `$PATH`; Pocket-Codex
does **not** vendor a model runtime. The CLI exposes:

```text
pocket-codex serve
pocket-codex connect
pocket-codex status
pocket-codex stop
pocket-codex codex   start | stop | status
pocket-codex pb      register | subscribe | status
pocket-codex remote-hint
pocket-codex version
```

Typical host-side flow:

```bash
pocket-codex serve --relay relay.example.com:7666
```

Typical client-side flow:

```bash
pocket-codex connect --relay relay.example.com:7666
codex --remote ws://127.0.0.1:28080
```

### Flutter front-end

`apps/flutter` is a Flutter app that talks to Rust through
`flutter_rust_bridge`. Flutter is locked at the project level via
[FVM](https://fvm.app/) (`.fvmrc`) and at the language level via
`pubspec.yaml`'s `environment.flutter` field; CI uses
`subosito/flutter-action@v2` against the same pin.

```bash
# One-time: install fvm and the pinned Flutter version.
brew tap leoafarias/fvm && brew install fvm
fvm install 3.44.0 --setup

# Day-to-day:
cd apps/flutter
fvm flutter pub get
fvm flutter analyze
fvm flutter test
```

If you change anything under `crates/pocket-codex-bridge/src/api/`,
re-run the codegen:

```bash
flutter_rust_bridge_codegen generate
```

## License

Pocket-Codex is licensed under the [Apache License 2.0](LICENSE).

The upstream projects under `deps/` keep their own licenses; consult
each submodule for details.
