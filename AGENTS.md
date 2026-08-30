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
  pb-mapper/               # upstream pb-mapper (git submodule)
  kanal/                   # fork pinned to a known-good commit; transitively
                           # required by pb-mapper, redirected via [patch]
  uni-stream/              # ditto; transitively required by pb-mapper
docs/                      # design notes, protocol references, CLI verification
scripts/                   # install scripts, local CI, CI affected-surface gate
```

`Cargo.toml` is a workspace root; every crate under `crates/` is a
workspace member (see the `members` list for the canonical set).
Submodules under `deps/` are kept **out** of the workspace via the
`exclude` list — the pinned
upstream crates use their own lints/profiles and we depend on them
through explicit path or git deps where needed. The root manifest's
`[patch]` table redirects `acking-you/kanal` and `acking-you/uni-stream`
to the local submodules so the build stays reproducible across
contributor checkouts and CI even after the upstream forks evolve.

## 3. Crate responsibilities

Shared / host side:

| Crate                       | Owns                                                                                           |
| --------------------------- | ---------------------------------------------------------------------------------------------- |
| `pocket-codex-core`         | configuration schema, on-disk `state.toml`, well-known paths, error types, `service::{ServiceId, ServiceKind, sanitize_component, default_device_id}` for `pcx:<device>:<kind>:<name>` relay keys — small, dependency-light |
| `pocket-codex-codex`        | spawning / supervising / inspecting the `codex app-server` child process (out-of-process *and* the in-process `embedded-codex` path), JSON-RPC envelope types |
| `pocket-codex-pb`           | async wrappers around the published `pb-mapper` client SDK: `RelaySession` (address + credential), register / subscribe / status, `publish` (and the one name-conflict failure a caller must not retry), admin credential issuance, and credential keep-alive |
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
pulled in transitively) is gone: pb-mapper is a registry dependency now, so its
own transitive pins come from the lockfile rather than a mirrored `[patch]`
table. After pulling this repo, materialise the submodule with:

```bash
git submodule update --init --recursive
```

### 8.1 `deps/codex` is a fork branch, not vanilla upstream

`deps/codex` tracks the **`pocket-codex` branch** of our fork
`git@github.com:acking-you/codex.git` (pinned via `branch = pocket-codex`
in `.gitmodules`). That branch is **upstream `openai/codex` main plus a
small, self-contained set of pocket-codex adaptations** — it is not a
vanilla upstream checkout. The adaptations are the *only* first-party
changes allowed to live inside `deps/codex`, and they exist because the
desktop app compiles codex **in-process** (the `embedded-codex` feature,
Windows/macOS), so codex's own child processes run under our GUI host:

- **Windows `CREATE_NO_WINDOW` console suppression.** Every `git` / shell /
  hook / plugin / PTY child codex spawns gets `creation_flags(0x08000000)`.
  Without it a console-subsystem child flashes a black terminal window
  because the host process has no console. Applied at each spawn site and
  the central git-command helpers.
- **Embedded PTY `HPCON` → std `RawHandle` cast** so the ConPTY code builds
  as an in-workspace path dependency.

**Rule: never carry pocket-codex logic in `deps/codex` beyond these
host-integration shims.** Anything else belongs in the `crates/` above codex.

### 8.2 Periodically merging upstream

The `pocket-codex` branch is long-lived; we bring in upstream by **merging
`origin/main` into it** (never rebasing — it is shared and pinned). Roughly
every so often:

```bash
cd deps/codex
git fetch origin                       # fork's main mirrors openai/codex main
git checkout pocket-codex
git merge origin/main                  # re-apply our shims onto upstream refactors
# resolve conflicts by TAKING UPSTREAM's new structure, then re-inject the
# CREATE_NO_WINDOW suppression into the moved/renamed git-command builders.
git push origin pocket-codex
```

Then, back in **this** repo, record the new codex + keep the build green:

```bash
git add deps/codex .gitmodules         # bump the submodule pointer
```

- **Mirror codex's `[patch]` table.** codex pins forked
  `tokio-tungstenite` / `tungstenite` (and may add others). Copy the exact
  `rev`s from `deps/codex/codex-rs/Cargo.toml`'s `[patch]` into the root
  `Cargo.toml` `[patch]` (both the `crates-io` and the `ssh://…tungstenite`
  blocks) — our WS client shares those forks.
- **Regenerate `Cargo.lock`** (`cargo update -w`); the CLI release builds
  `--locked`.
- **Compile the embedded path**: on Windows/macOS `cargo check -p
  pocket_codex_bridge` pulls codex in-process, so it is the real test that
  the merge + our shims + the patch revs still build. The `embedded-codex`
  dependency is target-gated to Windows/macOS, so **Linux CI does not compile
  codex at all** and will not catch codex-integration breakage — check it
  locally on a desktop OS before opening the PR.
- **Keep `sqlx` in lock-step with codex's `libsqlite3-sys`.** codex's
  `codex-state` links the native `sqlite3` at a specific `libsqlite3-sys`
  version; so does the backend's `sqlx`. Cargo forbids two `sqlite3`-linking
  crates at *different* versions in one workspace graph, so a codex bump that
  moves `libsqlite3-sys` breaks the whole workspace resolve. When bumping
  codex, read `deps/codex/codex-rs/Cargo.toml`'s `libsqlite3-sys` pin and set
  the root `sqlx` to a version whose bundled `libsqlite3-sys` matches it
  (codex `0.37` ↔ sqlx `0.9`; codex `0.30`/`0.28` ↔ sqlx `0.8`).
- **Keep the root `rust-toolchain.toml` at/above codex's rustc floor.** codex
  pins its own toolchain in `deps/codex/codex-rs/rust-toolchain.toml` (e.g.
  `1.95.0`). The desktop app compiles codex through cargokit, which we patched
  (`apps/flutter/rust_builder/cargokit/build_tool/lib/src/builder.dart`) to read
  the root `rust-toolchain.toml` `channel` — so `flutter build` and `cargo`
  share ONE pinned toolchain that `rustup` auto-installs (a one-click build),
  instead of cargokit's default stale `stable`. When codex raises its floor,
  bump the root `rust-toolchain.toml` (and the matching `RUST_TOOLCHAIN` in
  `ci.yml` / `release.yml`) to a nightly at/above it, and re-run
  `cargo fmt --check` on the new nightly (rustfmt output can shift between
  nightlies and force a reformat).

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
7. **Embedded codex (done).** Desktop builds compile codex in-process
   behind the `embedded-codex` feature, so a machine can host without a
   separate `codex` install; see §8.1 for the shims this requires.
8. **Strongly-typed JSON-RPC client (next).** Replace the
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
