# Pocket-Codex Agent Guide

> This document is the contract between human contributors, AI coding
> agents (Claude Code, Codex CLI, etc.) and the project itself. Read it
> before touching code.

## 1. Project intent

Pocket-Codex turns the upstream
[`codex app-server`](https://github.com/openai/codex) protocol into a
portable, multi-device experience:

- A **pure-Rust CLI** (`pocket-codex`) supervises a local
  `codex app-server` process on the machine that already has Codex
  installed.
- The same CLI uses [`pb-mapper`](https://github.com/acking-you/pb-mapper)
  to either **register** local app-server / direct Responses API services
  with a relay or **subscribe** to remote ones, materialising them as
  local TCP endpoints.
- A **Flutter front-end** (under `apps/flutter`, driven through
  `flutter_rust_bridge`) consumes the app-server JSON-RPC protocol
  directly to give every platform a native UI without re-implementing
  the model runtime.

The repository deliberately does **not** vendor a model runtime; the
user-supplied `codex` binary is the source of truth.

## 2. Repository layout

```
apps/flutter/              # Flutter UI (FRB-driven, FVM-locked at 3.44.0)
assets/logo/               # Project artwork (poster.png, logo.png)
crates/
  pocket-codex-core/       # shared types, config schema, error helpers,
                           # paths/state.toml on-disk format
  pocket-codex-codex/      # codex app-server process manager,
                           # JSON-RPC envelope types
  pocket-codex-pb/         # pb-mapper register/subscribe glue
  pocket-codex-cli/        # `pocket-codex` binary entrypoint
  pocket-codex-bridge/     # cdylib consumed by flutter_rust_bridge
deps/
  codex/                   # upstream openai/codex (git submodule)
  pb-mapper/               # upstream pb-mapper (git submodule)
  kanal/                   # fork pinned to a known-good commit; transitively
                           # required by pb-mapper, redirected via [patch]
  uni-stream/              # ditto; transitively required by pb-mapper
docs/                      # design notes, protocol references
skills/                    # contributor / agent skill packs
```

`Cargo.toml` is a workspace root. The four `pocket-codex-*` crates and
`pocket_codex_bridge` are workspace members. Submodules under `deps/`
are kept **out** of the workspace via the `exclude` list — the pinned
upstream crates use their own lints/profiles and we depend on them
through explicit path or git deps where needed. The root manifest's
`[patch]` table redirects `acking-you/kanal` and `acking-you/uni-stream`
to the local submodules so the build stays reproducible across
contributor checkouts and CI even after the upstream forks evolve.

## 3. Crate responsibilities

| Crate                  | Owns                                                                                           |
| ---------------------- | ---------------------------------------------------------------------------------------------- |
| `pocket-codex-core`    | configuration schema, on-disk `state.toml`, well-known paths, error types — small, dependency-light |
| `pocket-codex-codex`   | spawning / supervising / inspecting the `codex app-server` child process, JSON-RPC envelope types  |
| `pocket-codex-pb`      | thin async wrappers around `pb_mapper::local::{server,client}` for register / subscribe / status   |
| `pocket-codex-cli`     | user-facing `pocket-codex` binary; high-level `serve` / `connect` / `api {serve,connect}` / `services {list,default set}` / `status` / `stop`, low-level `codex {start,stop,status}`, `pb {register,subscribe,status}`, `remote-hint`, `version` |
| `pocket_codex_bridge`  | `cdylib + staticlib` consumed by Flutter via `flutter_rust_bridge`; auto-generated bindings live in `lib/src/rust` of the Flutter app |

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
- `unsafe` is forbidden by default (`#![forbid(unsafe_code)]` at every
  crate root). If you really need it, justify it in the PR and gate it
  behind a Cargo feature.
- Keep functions short and modules shallow. Refactor when nesting
  goes past three levels.
- Prefer `tracing` over `println!`/`eprintln!` for anything that is not
  CLI output the user explicitly asked for.
- File paths in handoff messages and PR descriptions follow `path:line`
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
7. **Hand off.** Summarise in Chinese (per repo convention), cite
   `path:line`, list assumptions, state risks and next steps.

## 7. Verification commands

Run these before claiming a task is done. CI runs the same set.
The upstream/submodule code under `deps/` is deliberately outside this
workspace's formatting and linting contract: do not run rustfmt,
clippy or other rewrite/lint commands against `deps/` unless the task is
an intentional submodule bump or upstream contribution. In particular,
do **not** run `cargo fmt --all`; use the explicit first-party package
list below so path/patch dependencies under `deps/` are never rewritten.

```bash
# Rust workspace
cargo fmt -p pocket-codex-core -p pocket-codex-codex -p pocket-codex-pb -p pocket-codex-cli -p pocket_codex_bridge -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked

# Flutter front-end (requires `fvm install 3.44.0 --setup` once)
cd apps/flutter
fvm flutter pub get
dart format --output=none --set-exit-if-changed lib/main.dart test integration_test
fvm flutter analyze
fvm flutter test
```

Optional but encouraged for crates touching FFI or the protocol layer:

```bash
cargo doc --workspace --no-deps
flutter_rust_bridge_codegen generate    # after editing crates/pocket-codex-bridge/src/api/
```

## 8. Working with submodules

`deps/codex` and `deps/pb-mapper` are git submodules. They pin specific
upstream commits; do **not** edit them in place from this repo.

```bash
# After pulling new commits in this repo:
git submodule update --init --recursive

# To bump a submodule (only when intentional):
git -C deps/codex fetch
git -C deps/codex checkout <sha-or-tag>
git add deps/codex
git commit -m "deps(codex): bump to <sha-or-tag>"
```

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
6. **Strongly-typed JSON-RPC client (next).** Replace the
   `serde_json::Value` surface in `pocket-codex-codex::protocol` with
   the upstream `codex-app-server-protocol` types so the Flutter UI
   gets compile-time-checked methods.
7. **Flutter UI evolution (in progress).** `apps/flutter` consuming
   the protocol via `flutter_rust_bridge`; today it only ships a
   sample bridge round-trip.

When you ship a milestone, update `README.md` (Status table) **and**
this file's roadmap so the source of truth stays in sync.

## 10. Communication conventions

- Final responses to the user are in **Chinese** (per repo norm).
- Lead with findings before summaries.
- Cite files as `path:line`.
- State assumptions explicitly. If an assumption could change the
  design or risk breakage/data loss, **stop and ask**.
