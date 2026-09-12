# Pocket-Codex Agent Guide

> This document is the contract between human contributors, AI coding
> agents (Claude Code, Codex CLI, etc.) and the project itself. Read it
> before touching code.

## 1. Project intent

Pocket-Codex turns the upstream
[`codex app-server`](https://github.com/openai/codex) protocol into a
portable, multi-device experience, and additionally exposes the host's
Codex login as a relay-reachable Responses API endpoint for any device:

- A **pure-Rust CLI** (`pocket-codex`) supervises a local
  `codex app-server` process on the machine that already has Codex
  installed.
- The same CLI ships an in-process **Responses API proxy** that reuses
  the host's `codex login` (ChatGPT account or `CODEX_ACCESS_TOKEN`)
  to serve OpenAI-compatible `/v1/responses` HTTP + WebSocket traffic,
  letting devices *without* Codex installed drive the same model
  through the relay.
- The CLI uses [`pb-mapper`](https://github.com/acking-you/pb-mapper)
  to **register** either service on a relay under
  `pcx:<device>:<kind>:<name>` keys, or to **subscribe** to remote
  ones, materialising them as local TCP endpoints.
- A **Flutter front-end** (under `apps/flutter`, driven through
  `flutter_rust_bridge`) consumes the app-server JSON-RPC protocol
  directly to give every platform a native UI without re-implementing
  the model runtime.

Two ways to wire devices together, both first-class:

- **Self-host** — every device shares one relay address plus a 32-byte
  `MSG_HEADER_KEY`, and talks to the relay directly under `pcx:…` keys.
  Selected by an explicit `--relay`.
- **Hosted account** — the optional `pocket-codex-backend` runs once on a
  server; devices sign in with GitHub, and the backend hands each account a
  short-lived relay credential confined to its own `pcxu:<user>:…`
  namespace. Devices then talk to the relay **directly**: the relay's
  administrator key never reaches a client, accounts stay isolated from
  each other, and the backend is not on the data path.

The repository deliberately does **not** vendor a model runtime; the
user-supplied `codex` binary (and its login state) is the source of
truth.

## 2. Repository layout

```
apps/flutter/              # Flutter UI (FRB-driven, FVM-locked at 3.44.0)
assets/logo/               # Project artwork (poster.png, logo.png)
crates/                    # all first-party Rust crates; see §3 for who owns what
deploy/                    # hosted-backend deployment unit + config examples
deps/
  codex/                   # acking-you/codex fork, branch `pocket-codex`
                           # (git submodule) = upstream openai/codex main +
                           # our adaptations; see §8
docs/                      # design notes, protocol references, CLI verification
scripts/                   # install scripts, local CI, CI affected-surface gate
```

`Cargo.toml` is a workspace root; every crate under `crates/` is a
workspace member (see the `members` list for the canonical set).
The Codex submodule under `deps/` is kept **out** of the workspace via the
`exclude` list and retains its upstream lints/profiles. Cargo fetches pb-mapper
from its dedicated `pocket-codex` Git branch; `Cargo.lock` pins the exact commit
and its registry dependencies. No pb-mapper, kanal, or uni-stream submodules
or local dependency patches are needed.

## 3. Crate responsibilities

Shared / host side:

| Crate                       | Owns                                                                                           |
| --------------------------- | ---------------------------------------------------------------------------------------------- |
| `pocket-codex-core`         | configuration schema, on-disk `state.toml`, well-known paths, error types, `service::{ServiceId, ServiceKind, sanitize_component, default_device_id}` for `pcx:<device>:<kind>:<name>` relay keys — small, dependency-light |
| `pocket-codex-codex`        | spawning / supervising / inspecting the external `codex app-server` child process, upstream protocol types and compatible JSON-RPC envelopes |
| `pocket-codex-pb`           | async wrappers around the Git-pinned `pb-mapper` client SDK: `RelaySession` (address + credential), register / subscribe / status, `publish` (and the one name-conflict failure a caller must not retry), admin credential issuance, and credential keep-alive |
| `pocket-codex-api-proxy`    | local Responses API proxy: forwards `/v1/responses` (HTTP + WS) to ChatGPT's Codex backend, reusing the host's `codex login`; shared by the CLI worker and the in-app host |
| `pocket-codex-host-svc`     | host-side meta service — remote-viewable codex sessions, per-thread config, attachment upload — published on the relay as a third `meta:<name>` service |
| `pocket-codex-cli`          | user-facing `pocket-codex` binary; account (`login` / `logout` / `account`), setup (`init`), high-level `serve` / `connect` / `api {serve,connect}` / `services {list,default set}` / `status` / `stop`, low-level `codex {start,stop,status}`, `pb {register,subscribe,status}`, `remote-hint`, `version` |
| `pocket_codex_bridge`       | `cdylib + staticlib` consumed by Flutter via `flutter_rust_bridge`; auto-generated bindings live in `lib/src/rust` of the Flutter app |

Hosted-account mode (all optional — self-host never touches these):

| Crate                        | Owns                                                                                           |
| ---------------------------- | ---------------------------------------------------------------------------------------------- |
| `pocket-codex-account-proto` | wire types and `pcxu:<user>:…` key namespacing shared between the backend and its CLI/app clients |
| `pocket-codex-auth`          | GitHub device-flow + web auth-code login, session JWTs, refresh tokens                            |
| `pocket-codex-store`         | SQLite persistence (users, refresh tokens, device flows) for the backend                          |
| `pocket-codex-backend`       | the deployable binary: GitHub-login HTTP API + `/v1/relay` credential vending; see [`deploy/`](deploy/README.md) |

When in doubt, prefer adding a new module to an existing crate over
introducing a new crate. Crates are free; *boundaries* are not.

## 4. Engineering principles

We follow Linus Torvalds–style engineering. In short:

1. **Don't break userspace.** Once a CLI flag, on-disk layout or
   wire-protocol field is documented, it is part of the contract. Add,
   don't mutate. If a breaking change is unavoidable, version it
   explicitly and write a migration note.
2. **KISS / YAGNI.** Avoid speculative abstractions. Add a trait when
   there are at least two real implementations. Add a config knob when
   there is at least one real user who needs it.
3. **Critique code, not people.** Be technical, be direct, be kind.
4. **Faithful upstream behaviour > local heuristics.** If the upstream
   `codex` or `pb-mapper` does something a particular way, mirror it
   instead of layering on top a fragile compatibility shim.

## 5. Code-editing rules

- Comments are written in **English**. Add a comment only when intent is
  non-obvious; obvious code does not need narration.
- Public items are documented (`missing_docs = "deny"` is on at the
  workspace level). When you add a public function, write a doc comment.
- No `unwrap()` / `expect()` in non-test code without a `// reason: ...`
  follow-up. `clippy::unwrap_used` is `warn` and we treat it as `deny` in
  reviews.
- `unsafe` is forbidden by default — crate roots carry
  `#![forbid(unsafe_code)]`. The sanctioned exception is
  `pocket_codex_bridge`, whose `flutter_rust_bridge`-generated code emits
  `unsafe`. If you really need it elsewhere, justify it in review and gate
  it behind a Cargo feature.
- Keep functions short and modules shallow. Refactor when nesting
  goes past three levels.
- Prefer `tracing` over `println!`/`eprintln!` for anything that is not
  CLI output the user explicitly asked for.
- File paths in handoff messages and change descriptions follow `path:line`
  citations (e.g. `crates/pocket-codex-cli/src/main.rs:42`).

## 6. Workflow checklist

Use this as the default loop for any non-trivial change:

1. **Intake.** Restate the task in your own words. Confirm the problem
   exists. Note any potential for breaking userspace.
2. **Context gathering.** Locate the files that need to change. Stop as
   soon as you can name them; aim for ~5–8 tool calls in the first pass.
3. **Exploration.** When ≥3 steps or multiple files are involved, walk
   dependencies, surface assumptions, and write down the output
   contract (files changed, expected behaviour, tests touched).
4. **Plan.** Produce a multi-step plan that references concrete files
   and functions before you edit anything.
5. **Execute.** Make the change. On failure, diagnose and adjust; if
   blocked, ask the user.
6. **Verify.** Run the verification commands below and reflect:
   maintainability, tests, performance, security, backward
   compatibility. Fix issues before handoff.
7. **Hand off.** Summarise the change, cite `path:line`, list
   assumptions, state risks and next steps.

## 7. Verification commands

Run these (the full set) before claiming a task is done. CI runs the
same commands but **scopes them to the surfaces your change touches**: a
gate (`scripts/ci_affected.py`, stdlib-only) computes which crates are
affected from the diff and runs fmt/clippy/test only when a first-party
crate changed (clippy on the changed crates, test on them plus their
workspace dependents; a cross-cutting change — root manifest, lockfile,
`rustfmt.toml`, `.cargo/`, `deps/`, toolchain — falls back to the whole
workspace), and the Flutter job only when `apps/flutter/` changed. A
change under `.github/` or `scripts/` forces the full Rust suite plus
Flutter. So locally you always run everything below; CI may legitimately
skip jobs your change does not touch.
The upstream/submodule code under `deps/` is deliberately outside this
workspace's formatting and linting contract: do not run rustfmt,
clippy or other rewrite/lint commands against `deps/` unless the task is
an intentional submodule bump or upstream contribution. In particular,
do **not** run `cargo fmt --all`; use the explicit first-party package
list below so path/patch dependencies under `deps/` are never rewritten.

```bash
# Rust workspace — the -p list must cover every workspace member in
# `Cargo.toml`; add new crates here (and in ci.yml) when you create them.
cargo fmt --check \
  -p pocket-codex-core -p pocket-codex-codex -p pocket-codex-pb \
  -p pocket-codex-api-proxy -p pocket-codex-host-svc -p pocket-codex-cli \
  -p pocket_codex_bridge -p pocket-codex-account-proto -p pocket-codex-store \
  -p pocket-codex-auth -p pocket-codex-backend
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked

# Flutter front-end (requires `fvm install 3.44.0 --setup` once)
cd apps/flutter
fvm flutter pub get
dart format --output=none --set-exit-if-changed lib test integration_test
fvm flutter analyze
fvm flutter test
```

Optional but encouraged for crates touching FFI or the protocol layer:

```bash
cargo doc --workspace --no-deps
flutter_rust_bridge_codegen generate    # after editing crates/pocket-codex-bridge/src/api/
```

### Fonts (desktop-only Noto Sans SC)

Chinese renders thin and unevenly weighted on Windows with Flutter's default
fonts, so **desktop** builds bundle **Noto Sans SC** (`apps/flutter/assets/fonts/`,
SIL OFL 1.1). To keep the ~17 MB out of the Android/iOS artifacts, the font is
declared only in `apps/flutter/pubspec-desktop.yaml`, **not** the default
`pubspec.yaml`:

- `pubspec.yaml` — the default used by mobile, `flutter test`, and local dev.
  No font. Source of truth for dependencies.
- `pubspec-desktop.yaml` — a copy of `pubspec.yaml` **plus** a `fonts:` block.
  The desktop release jobs (`app-windows` / `app-linux` / `app-macos` in
  `.github/workflows/release.yml`) `cp pubspec-desktop.yaml pubspec.yaml` before
  building. **Keep its dependencies in sync with `pubspec.yaml`.**

`lib/src/fonts.dart` sets the primary family to `Noto Sans SC` only on desktop
(gated on `defaultTargetPlatform`, so `flutter test` — forced to android — takes
the mobile branch); mobile/web use the OS CJK font (PingFang SC / Noto Sans CJK).
To preview the bundled font locally on desktop:

```bash
cp apps/flutter/pubspec-desktop.yaml apps/flutter/pubspec.yaml
(cd apps/flutter && fvm flutter pub get && fvm flutter run -d windows)
git checkout -- apps/flutter/pubspec.yaml   # restore before committing
```

## 8. Working with submodules

`deps/codex` is a git submodule pinned to a specific commit — the only one
left. `deps/pb-mapper` (plus the `deps/kanal` and `deps/uni-stream` forks it
pulled in transitively) is gone. pb-mapper is a Cargo Git dependency on
`https://github.com/acking-you/pb-mapper`, branch `pocket-codex`; `Cargo.lock`
pins its exact commit. Push SDK fixes to that branch, then run
`cargo update -p pb-mapper` here and commit the resulting lockfile after
verification. Normal builds use `--locked` and do not automatically follow
branch updates. After pulling this repo, materialise the Codex submodule with:

```bash
git submodule update --init --recursive
```

### 8.1 External Codex only; built-in engine is unimplemented

**All platforms must run Codex as an external executable. Do not compile,
link, bundle, or launch the Codex model runtime in Pocket-Codex.** The UI
reserves a disabled **Built-in engine / 内置引擎** option marked **Not
implemented yet / 暂未实现**. Do not re-enable it through a Cargo feature,
target-specific dependency, build script, or release packaging step without
an explicit change to this product requirement.

- First-party crates may depend directly on upstream **protocol crates only**
  (`codex-app-server-protocol` / `codex-protocol`). No direct dependencies on
  `codex-app-server`, `codex-core`, `codex-arg0`, `codex-config`, runtime
  extensions, or executable/helper crates, even as optional dependencies.
- Upstream protocol crates currently have their own transitive support
  dependencies. Audit the resolved graph on every submodule bump; do not
  claim these are a standalone, dependency-free schema. The graph must not
  include `codex-app-server`, `codex-core`, `codex-arg0`, `codex-exec`,
  `codex-tui`, `codex-windows-sandbox`, or `codex-goal-extension`.
- Keep the external spawn/supervision/readiness/logging path. Resolve the
  binary from an explicit path, saved configuration, then `PATH`. Missing
  Codex must produce an installation/path error; never fall back to a bundled
  runtime.
- Keep the existing bridge `embedded` field/argument for compatibility:
  status reports false, explicit true starts fail before side effects, and
  legacy auto-host preferences restore through external Codex. Users who
  previously relied on embedded hosting must install Codex or select its path.
- Do not build or package upstream Windows sandbox helpers. The installed
  external Codex owns its runtime, sandbox helpers, and version.
- Keep the standalone Responses API proxy functional: both HTTP POST and
  WebSocket GET on `/v1/responses` must forward requests, host authentication,
  streaming events, and upstream errors without requiring an embedded engine.
  Verify both transports when changing Codex dependencies or hosting.
- Avoid parallel duplicate Rust builds and monitor disk space during local
  verification. Prefer reusing build caches; only remove reproducible build
  artifacts when cleanup is needed, never user data or source directories.

### 8.2 Updating the protocol submodule

`deps/codex` remains pinned to the `pocket-codex` branch of
`git@github.com:acking-you/codex.git`. Its historical host-integration shims
are not built into this application. Do not add new runtime shims for this
project. Continue to merge upstream into the shared fork branch rather than
rebasing published history, and record the verified submodule pointer here.

After a protocol bump:

1. Inspect the protocol manifests and run `cargo metadata --format-version 1`
   / `cargo tree -p pocket_codex_bridge` to verify the dependency restriction
   above on all targets/features.
2. Mirror any still-required upstream WebSocket fork patches exactly and
   regenerate `Cargo.lock` without unrelated dependency upgrades.
3. Keep `sqlx` compatible with the protocol graph's transitive
   `libsqlite3-sys` version: Cargo permits only one native sqlite3 linker.
4. Keep the root Rust toolchain at or above the protocol crates' compiler
   floor and keep CI/release toolchains in sync.
5. Run the full first-party verification in §7 and build the desktop UI.
   Never run formatting or lint rewrites inside `deps/codex` for a routine
   application change.

## 9. Roadmap (rough)

The order below is our current best guess; it is not a contract.

1. **CLI bootstrap (done).** `pocket-codex version`, configuration
   loading, basic logging, command-line schema + dispatcher.
2. **Codex process manager (done).** `pocket-codex codex
   start|stop|status` spawning the user's local `codex app-server`,
   persisting PID / listen URL metadata to `state.toml`, surfacing
   logs.
3. **pb-mapper register / subscribe (done).** `pocket-codex pb
   register` and `pocket-codex pb subscribe` re-using the upstream
   `local::server::run_server_side_cli` /
   `local::client::run_client_side_cli` helpers.
4. **Combined `serve` / `connect` flow (done).** `pocket-codex serve`
   starts or reuses the local app-server, registers it with a relay and
   tracks the daemonised pb-mapper worker in `state.toml`;
   `pocket-codex connect` subscribes on the client side and prints the
   matching `codex --remote ...` command.
5. **Multi-device service selection + direct API proxy (done).**
   Pocket-Codex service keys use `pcx:<device>:<service>:<name>`;
   clients can discover services, set a local default target and choose
   app-server or direct Responses API proxy flows independently.
6. **Hosted account mode (done).** Optional `pocket-codex-backend`:
   GitHub login (device flow + web auth-code), SQLite-backed sessions,
   and `/v1/relay`, which vends each account a short-lived pb-mapper
   credential scoped to its own `pcxu:<user>:…` namespace. Clients then
   register/connect against the relay **directly** — the backend holds the
   administrator key and stays off the data path. It previously brokered
   every byte on its own port; direct connect removed that hop, so backend
   availability is no longer a prerequisite for two of a user's own devices
   to talk. Self-host stays the escape hatch behind `--relay`.
   Deployment unit lives in [`deploy/`](deploy/README.md).
7. **External Codex only (2026-09-13).** All desktop hosts use an installed
   external `codex`. The former embedded runtime is removed from builds and
   packaging; **Built-in engine** is a disabled, unimplemented placeholder.
   Only upstream protocol crates may be direct Codex dependencies (see §8.1).
8. **App-server protocol sync (2026-09-07).** Codex fork merged upstream main
   `db0568dbb`; CLI and UI share the acknowledged initialization handshake.
   Flutter reads v2 accounts and current thread model/effort, and answers
   asynchronous questions with `turn/steer` while running or `turn/start` when idle.
   **Strongly-typed JSON-RPC client (next).** Replace the
   `serde_json::Value` surface in `pocket-codex-codex::protocol` with
   the upstream `codex-app-server-protocol` types so the Flutter UI
   gets compile-time-checked methods.
9. **Flutter UI evolution.** `apps/flutter` consumes the bridge via
   `flutter_rust_bridge`. P1 shipped: onboarding (relay+key, `pcx1:`
   import/export, persisted to `config.toml` 0600), service discovery,
   API-service subscribe (local OpenAI-compatible endpoint), settings,
   responsive Material 3 (light/dark). P2 shipped: app-server remote
   control (threads, live event stream, approvals, attachments,
   per-thread config). P3 shipped: chat-first home — `/` resolves a
   host (last used → locally hosted → first reachable), auto-connects,
   and opens the latest session with all sessions in the sidebar; the
   services hub lives on at `/manage`; desktop auto-restores hosting on
   boot (`ui_state.json`).

When you ship a milestone, update `README.md` (Status table) **and**
this file's roadmap so the source of truth stays in sync.

## 10. Communication conventions

- Reply in whatever language the person you are talking to is using.
  This file does not mandate one.
- Lead with findings before summaries.
- Cite files as `path:line`.
- State assumptions explicitly. If an assumption could change the
  design or risk breakage/data loss, **stop and ask**.
