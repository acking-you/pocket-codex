<!--
Thanks for the patch! Fill in the sections below — short bullets are fine.
PR description, commits and code stay in English; final review replies in
the repo follow the Chinese-summary convention from `AGENTS.md` §10.
-->

## Summary

<!--
What changes, and why. Cite files as `path:line` (e.g.
`crates/pocket-codex-cli/src/main.rs:42`). Keep the *why* here; commit
messages can stay short.
-->

## Type of change

<!-- Tick all that apply. -->

- [ ] `feat` — new user-visible behaviour
- [ ] `fix` — bug fix
- [ ] `docs` — documentation / comments only
- [ ] `refactor` — no behaviour change
- [ ] `deps` — submodule or dependency bump
- [ ] `ci` — workflow / tooling
- [ ] other (please describe):

## Userspace impact

<!--
`AGENTS.md` §4.1: once a CLI flag, on-disk layout or wire-protocol field
is documented it is part of the contract. If you're changing one of
these, call it out explicitly and write a migration note.
-->

- [ ] Changes a `pocket-codex` CLI flag, subcommand or output format
- [ ] Changes the on-disk layout (`state.toml`, paths under
      `pocket-codex-core::paths`)
- [ ] Changes a wire-protocol field (codex app-server JSON-RPC,
      Responses API proxy, pb-mapper key shape)
- [ ] Changes the FRB API surface in
      `crates/pocket-codex-bridge/src/api/`
- [ ] None of the above

If any of the above are ticked, describe the migration / compatibility
plan here:

## Tested

<!--
The commands below mirror `.github/workflows/ci.yml`. Tick the ones you
ran locally; if anything was skipped, say why.
-->

- [ ] `cargo fmt --check -p pocket-codex-core -p pocket-codex-codex -p pocket-codex-pb -p pocket-codex-cli -p pocket_codex_bridge`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] `dart format --output=none --set-exit-if-changed lib/main.dart test integration_test` (run from `apps/flutter`)
- [ ] `fvm flutter analyze` (run from `apps/flutter`)
- [ ] `fvm flutter test` (run from `apps/flutter`)
- [ ] `flutter_rust_bridge_codegen generate` (only if `crates/pocket-codex-bridge/src/api/` changed)
- [ ] Manual flow exercised (describe):

## Linked issues

<!--
Use `Closes #123` / `Refs #123`. If this is a roadmap milestone (see
`AGENTS.md` §9), call it out here.
-->

## Sync checklist

- [ ] If this ships a roadmap milestone, `README.md`'s Status table is
      updated.
- [ ] If this ships a roadmap milestone, `AGENTS.md`'s §9 roadmap is
      updated.
- [ ] Public items added in this PR have doc comments
      (`missing_docs = "deny"`).
- [ ] No new `unwrap()` / `expect()` in non-test code without a
      `// reason: …` follow-up.
- [ ] No formatting / lint / rewrite commands were run against `deps/`.
