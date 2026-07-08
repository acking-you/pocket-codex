AGENTS.md

# TEMPORARY — Local-only development mode (GitHub account suspended)

**Active since 2026-07-07; delete this whole section once GitHub access is
restored** (account `acking-you` is suspended by mistake; appeal in progress,
expected ~2–3 weeks). While active, this section OVERRIDES every
GitHub-dependent part of AGENTS.md (PRs, Actions CI, releases, `gh`).

`git push origin` and every `gh` command fail hard (`Your account is
suspended` / `Bad credentials`). Do not retry them or debug "connectivity" —
it is an account-level block, not a network problem. The same applies to the
`deps/*` submodule remotes (they point at the same account), so no submodule
fetches either; work from the local checkouts.

## The local workflow

1. **Branch per change, exactly as before.** Work on `feat/*` / `fix/*` /
   `chore/*` branches cut from local `main`. Never develop directly on
   `main`; `main` must always be green.

2. **Local CI replaces GitHub CI.** Before ANY merge to `main`, run

   ```
   pwsh scripts/local-ci.ps1        # -SkipRust / -SkipFlutter to scope
   ```

   which mirrors `.github/workflows/ci.yml` (rust fmt/clippy/test +
   dart format/analyze/test) and must print `LOCAL CI: GREEN`. Notes:
   `cargo fmt` runs in write mode (Windows `--check` false-positives on
   CRLF — commit whatever it reformats); Rust gates are workspace-wide
   because the affected-surfaces script needs python3 (absent here).

3. **Merging replaces the PR.** A single-commit branch fast-forwards
   (`git merge --ff-only <branch>`); a multi-commit branch squash-merges
   (`git merge --squash <branch>`). Either way the commit that lands on
   `main` must carry what the PR description would have: summary + why,
   wire/contract impact, and test evidence — the PR-template sections, in
   the commit body. No `(#NN)` suffix during the outage; numbering resumes
   with real PRs after restoration.

4. **Review replaces PR review.** For substantive changes, run a code-review
   pass (e.g. `/code-review`) before merging and fix or explicitly waive the
   findings; note this in the commit body ("review: N findings, M fixed").
   Trivial/mechanical changes may skip this, same as trivial PRs did.

5. **Backup replaces GitHub-as-offsite (critical).** This machine is now the
   only copy of the source AND of the `deps/*` fork patches. Bare mirrors
   live on the owner's VPS (`ubuntu@lb7666.top:~/repos/*.git` — superproject
   `pocket-codex.git` plus `codex.git`, `pb-mapper.git`, `kanal.git`,
   `uni-stream.git`), reachable through the `vps` remote configured in the
   superproject and in each submodule. After every merge to `main` — and at
   least daily while a long-running branch is open — push:

   ```
   git push vps --all && git push vps --tags
   ```

   (and the same from any `deps/<name>` whose fork branch changed). SSH to
   the VPS is password-authenticated; the owner supplies the password per
   session — never store it in the repo or in scripts.

6. **Releases pause.** `release.yml` cannot run, so no `v*` releases during
   the outage. Version bumps and local tags are fine (they push to `vps`);
   platform binaries can be built locally on demand (`fvm flutter build
   windows`, `cargo build --release`, `fvm flutter build apk`).

## Restoration checklist (run when the account is back)

1. `git push origin main --tags`, plus any still-open branches; from each
   changed `deps/<name>`: `git push origin <fork-branch>`.
2. Re-auth `gh` (`gh auth login`) — the old token was invalidated.
3. Watch CI on `main`; fix anything local CI missed (Linux-only issues, e.g.
   path handling — see the project-folders lesson in the memory notes).
4. Deploy the backend to lb7666.top — the duplicate-register first-wins fix
   only takes effect server-side after a redeploy (`deploy/`).
5. Cut the next release via the normal draft-validate → tag flow.
6. Delete this section from CLAUDE.md and keep only the `AGENTS.md` pointer.
