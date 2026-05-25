# Contributing to Pocket-Codex

Thanks for taking the time to help shape Pocket-Codex. This guide
captures the practical bits — how to set up the toolchain, run the same
checks CI runs, and land a clean change. The deeper *why* behind the
project (intent, crate boundaries, engineering principles) lives in
[`AGENTS.md`](AGENTS.md); please read it before any non-trivial change.

> [!WARNING]
> **Pre-1.0.** CLI flags, on-disk layout, the protocol mapping and
> crate boundaries are all expected to change. Treat this guide as the
> contract for *how* to contribute, not as a stability promise about
> what already exists.

## 1. Before you start

- **Issues first for non-trivial work.** Open a feature request or bug
  report (see [`.github/ISSUE_TEMPLATE`](.github/ISSUE_TEMPLATE)) before
  spending serious time on a patch. For tiny fixes (typos, obvious
  bugs), a PR is fine.
- **Read [`AGENTS.md`](AGENTS.md).** Especially §4 (engineering
  principles), §5 (code-editing rules) and §7 (verification commands).
- **Don't break userspace.** Once a CLI flag, on-disk layout or
  wire-protocol field is documented it is part of the contract — add,
  don't mutate. Breaking changes need an explicit migration note in the
  PR.

## 2. Prerequisites

| Tool                     | Version / source                                         |
| ------------------------ | -------------------------------------------------------- |
| Rust toolchain           | Pinned by [`rust-toolchain.toml`](rust-toolchain.toml) (nightly `2026-04-01`, with `rustfmt`, `clippy`, `rust-src`, target `wasm32-unknown-unknown`). `rustup` will pick this up automatically. |
| Flutter                  | **3.44.0**, locked in lockstep across [`apps/flutter/.fvmrc`](apps/flutter/.fvmrc), [`apps/flutter/pubspec.yaml`](apps/flutter/pubspec.yaml) (`environment.flutter`) and CI (`subosito/flutter-action@v2`). Bump all three together. |
| FVM                      | Recommended Flutter version manager. `brew tap leoafarias/fvm && brew install fvm` then `fvm install 3.44.0 --setup`. |
| `codex`                  | A working `codex` binary on `$PATH` for any flow that exercises `pocket-codex serve` / `connect` / `api …` against a real app-server. Pocket-Codex deliberately does **not** vendor a model runtime. |
| `flutter_rust_bridge_codegen` | Only needed when editing `crates/pocket-codex-bridge/src/api/`. |

## 3. Repository setup

```bash
# Clone with submodules (deps/codex, deps/pb-mapper, deps/kanal, deps/uni-stream).
git clone --recurse-submodules git@github.com:acking-you/pocket-codex.git
cd pocket-codex

# If you cloned without --recurse-submodules:
git submodule update --init --recursive

# Rust workspace.
cargo build --workspace

# Flutter front-end (one-time).
cd apps/flutter
fvm flutter pub get
```

Submodules are pinned commits — see [`AGENTS.md` §8](AGENTS.md#8-working-with-submodules)
for the bump procedure. Don't edit code inside `deps/` from this repo.

## 4. Branching and commits

- **Branch naming.** `<type>/<short-slug>`, e.g. `feat/api-proxy-tls`,
  `fix/state-toml-permissions`, `docs/contributing-guide`. Push to a
  fork or topic branch; never push directly to `main`.
- **Commits follow [Conventional Commits](https://www.conventionalcommits.org/).**
  Recent history is the source of truth for accepted prefixes; common
  ones are:
  - `feat(<scope>): …` — new user-visible behaviour
  - `fix(<scope>): …` — bug fixes
  - `docs(<scope>): …` — docs / comments only
  - `refactor(<scope>): …` — no behaviour change
  - `deps(<submodule>): bump to <sha-or-tag>` — submodule bumps
  - `ci: …` — workflow changes
  Keep the subject under ~70 characters; put the *why* in the body.
- **Comments and code in English** ([`AGENTS.md` §5](AGENTS.md#5-code-editing-rules)).
  Final review responses to maintainers are in **Chinese** per repo
  norm — but PR descriptions, commits, code and comments are English.

## 5. Code-editing rules (TL;DR — full list in `AGENTS.md` §5)

- Public items get doc comments (`missing_docs = "deny"` is on at the
  workspace level).
- No `unwrap()` / `expect()` in non-test code without a `// reason: …`
  follow-up. Clippy's `unwrap_used` is `warn` and treated as `deny` in
  review.
- `unsafe` is forbidden by default (`#![forbid(unsafe_code)]` at every
  crate root). If you really need it, justify it in the PR and gate it
  behind a Cargo feature.
- Prefer `tracing` over `println!` / `eprintln!` for anything that is
  not CLI output the user explicitly asked for.
- Cite files as `path:line` in PR descriptions and review threads.

## 6. Verification (must pass before requesting review)

These commands are exactly what [`.github/workflows/ci.yml`](.github/workflows/ci.yml)
runs. Run them locally before pushing.

```bash
# Rust workspace.
cargo fmt --check \
  -p pocket-codex-core \
  -p pocket-codex-codex \
  -p pocket-codex-pb \
  -p pocket-codex-cli \
  -p pocket_codex_bridge
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked

# Flutter front-end.
cd apps/flutter
fvm flutter pub get
dart format --output=none --set-exit-if-changed lib/main.dart test integration_test
fvm flutter analyze
fvm flutter test
```

> [!IMPORTANT]
> **Don't run `cargo fmt --all` or clippy / rustfmt against `deps/`.**
> The submodule tree is intentionally outside our formatting and lint
> contract; the explicit `-p …` package list above keeps `deps/kanal`,
> `deps/uni-stream`, `deps/codex` and `deps/pb-mapper` untouched.
> Likewise, `dart format` is restricted to `lib/main.dart`, `test/` and
> `integration_test/` — the FRB-generated tree under `lib/src/rust/`
> follows its own formatting and is regenerated, not hand-edited.

Optional but encouraged when touching FFI or the protocol layer:

```bash
cargo doc --workspace --no-deps
flutter_rust_bridge_codegen generate    # after editing crates/pocket-codex-bridge/src/api/
```

## 7. Submodule bumps

Only intentional bumps; never an accidental side effect.

```bash
git -C deps/<sub> fetch
git -C deps/<sub> checkout <sha-or-tag>
git add deps/<sub>
git commit -m "deps(<sub>): bump to <sha-or-tag>"
```

If a bump changes the upstream API, update the wrapper crate
(`pocket-codex-pb` or `pocket-codex-codex`) in the same PR and call out
the upstream commit range in the description.

## 8. Pull requests

- Open the PR against `main` from a topic branch.
- Fill in the [PR template](.github/PULL_REQUEST_TEMPLATE.md) — pay
  attention to the **Userspace impact** section.
- Keep PRs focused; split unrelated changes into separate PRs.
- When you ship a roadmap milestone (see [`AGENTS.md` §9](AGENTS.md#9-roadmap-rough)),
  update both `README.md`'s Status table **and** `AGENTS.md`'s roadmap
  in the same PR so the source of truth stays in sync.

## 9. Reporting bugs and proposing features

Use the issue forms under [`.github/ISSUE_TEMPLATE`](.github/ISSUE_TEMPLATE):

- **Bug report** — include `pocket-codex version`, `codex --version`
  (when relevant), OS, and any `tracing` output. Pre-1.0 means we'd
  rather hear about something twice than miss it.
- **Feature request** — lead with the problem you're trying to solve;
  proposed solutions are welcome but optional.

## 10. License and sign-off

Pocket-Codex is licensed under the [Apache License 2.0](LICENSE). By
contributing you agree your contributions are licensed under the same
terms. Submodules under `deps/` keep their own licenses; consult each
submodule for details.
