#!/usr/bin/env python3
"""Scope CI to the surfaces a change actually affects.

Pocket-Codex has two independent CI surfaces:
  * the Cargo workspace (crates/pocket-codex-*), gated by fmt/clippy/test
  * the Flutter app (apps/flutter), gated by the flutter job

`detect` decides, from the pushed/PR diff alone (stdlib tomllib + git, no
cargo and no submodules, so the gate never compiles), which surfaces to run
and writes them to $GITHUB_OUTPUT:
  rust_mode      none | all | subset
  test_crates    space-separated: changed crates + their workspace dependents
  clippy_crates  space-separated: the directly-changed crates
  flutter        true | false

`run-clippy` / `run-test` consume MODE + CRATES and invoke cargo for the
affected crates (or the whole workspace for `all`); `none` skips cargo.

Crate-set rationale (mirrors the reference static_flow project):
  * test uses the dependents closure -- a library change can break a
    dependent's behaviour, so the dependents must be tested.
  * clippy uses only the directly-changed crates -- clippy lints a crate's
    own code; the (already clippy-clean) dependents need not be re-linted.

Env:
  BASE_SHA, HEAD_SHA   PR base/head (or push before/after); used by detect.
  MODE, CRATES         consumed by run-clippy / run-test (from gate outputs).
  CI_AFFECTED_DRY_RUN  print decisions/commands without invoking cargo.
"""
import os
import subprocess
import sys
import tomllib
from collections import defaultdict
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Root files / trees whose change can affect every crate's build or lint, so
# they force a full Rust run rather than a per-crate subset: the root manifest
# and lockfile, rustfmt config, the cargo config, the toolchain pin, and the
# vendored submodule trees first-party crates compile through path deps /
# [patch] (deps/{codex,pb-mapper,kanal,uni-stream}).
RUST_CROSS_EXACT = {"Cargo.toml", "Cargo.lock", "rustfmt.toml"}
RUST_CROSS_PREFIX = (".cargo/", "deps/")

# CI infrastructure: a change here is validated end to end (full Rust + the
# Flutter job) so a broken pipeline definition can't slip through on a narrow
# subset. This selector also makes the very commit that introduces it run the
# whole suite.
CI_INFRA_PREFIX = (".github/", "scripts/")

# The Flutter app lives outside the cargo workspace.
FLUTTER_PREFIX = ("apps/flutter/",)


def run(cmd, **kw):
    return subprocess.run(cmd, cwd=REPO, text=True, capture_output=True, **kw)


def workspace_graph():
    """Return (dir2name, deps) parsed from the workspace manifests.

    deps[crate] = set of intra-workspace crates it depends on (any dep kind).
    Pocket-Codex crates reference each other as `dep = { workspace = true }`
    (the path lives in the root `[workspace.dependencies]`), not as a local
    `{ path = .. }`, so an edge is recognised whenever a dependency *key*
    matches another member's package name. No cargo invocation, so the gate
    needs no submodules.
    """
    root = tomllib.load(open(REPO / "Cargo.toml", "rb"))
    members = root["workspace"]["members"]
    dir2name = {}
    for m in members:
        ct = tomllib.load(open(REPO / m / "Cargo.toml", "rb"))
        dir2name[m] = ct["package"]["name"]
    names = set(dir2name.values())
    deps = defaultdict(set)
    for m in members:
        ct = tomllib.load(open(REPO / m / "Cargo.toml", "rb"))
        me = dir2name[m]
        for sect in ("dependencies", "dev-dependencies", "build-dependencies"):
            for key in ct.get(sect, {}):
                if key in names and key != me:
                    deps[me].add(key)
    return dir2name, deps


def changed_files(base, head):
    """Files changed between base and head, or None if undeterminable."""
    if not base or not head or set(base) <= {"0"}:
        return None
    res = run(["git", "diff", "--name-only", f"{base}...{head}"])
    if res.returncode != 0:
        res = run(["git", "diff", "--name-only", base, head])
    if res.returncode != 0:
        return None
    return [f for f in res.stdout.splitlines() if f.strip()]


def is_rust_cross_cutting(f):
    return (
        f in RUST_CROSS_EXACT
        or any(f.startswith(p) for p in RUST_CROSS_PREFIX)
        or Path(f).name.startswith("rust-toolchain")
    )


def owning_crate(f, dir2name):
    """Longest workspace-member dir that contains `f`, as its package name."""
    best = None
    for d in dir2name:
        if f == d or f.startswith(d + "/"):
            if best is None or len(d) > len(best):
                best = d
    return dir2name[best] if best else None


def dependents_closure(seeds, deps):
    """seeds plus every crate that (transitively) depends on a seed."""
    rev = defaultdict(set)
    for crate, ds in deps.items():
        for d in ds:
            rev[d].add(crate)
    affected, stack = set(seeds), list(seeds)
    while stack:
        for dep in rev.get(stack.pop(), ()):
            if dep not in affected:
                affected.add(dep)
                stack.append(dep)
    return affected


def emit(rust_mode, test_crates, clippy_crates, flutter):
    test_s = " ".join(sorted(test_crates))
    clippy_s = " ".join(sorted(clippy_crates))
    flutter_s = "true" if flutter else "false"
    print(f"[detect] rust_mode={rust_mode}")
    print(f"[detect] test_crates=[{test_s}]")
    print(f"[detect] clippy_crates=[{clippy_s}]")
    print(f"[detect] flutter={flutter_s}")
    gh_out = os.environ.get("GITHUB_OUTPUT")
    if gh_out:
        with open(gh_out, "a") as fh:
            fh.write(
                f"rust_mode={rust_mode}\n"
                f"test_crates={test_s}\n"
                f"clippy_crates={clippy_s}\n"
                f"flutter={flutter_s}\n"
            )


def detect():
    base = os.environ.get("BASE_SHA", "").strip()
    head = os.environ.get("HEAD_SHA", "").strip()
    files = changed_files(base, head)
    if files is None:
        print("[detect] base/head unresolved; running everything.")
        return emit("all", [], [], True)
    if not files:
        return emit("none", [], [], False)

    flutter = any(f.startswith(p) for f in files for p in FLUTTER_PREFIX)
    ci_infra = [f for f in files if any(f.startswith(p) for p in CI_INFRA_PREFIX)]
    if ci_infra:
        print(f"[detect] CI infra change(s): {sorted(ci_infra)[:5]} -> run everything.")
        return emit("all", [], [], True)

    rust_cross = [f for f in files if is_rust_cross_cutting(f)]
    if rust_cross:
        print(f"[detect] rust cross-cutting: {sorted(rust_cross)[:5]} -> full workspace.")
        return emit("all", [], [], flutter)

    dir2name, deps = workspace_graph()
    seeds = {c for c in (owning_crate(f, dir2name) for f in files) if c}
    if not seeds:
        rust_mode, test_crates, clippy_crates = "none", [], []
        print("[detect] no first-party crate affected (docs / flutter / vendored).")
    else:
        rust_mode = "subset"
        test_crates = dependents_closure(seeds, deps)
        clippy_crates = seeds
    return emit(rust_mode, test_crates, clippy_crates, flutter)


def sh(cmd):
    print("+", " ".join(cmd))
    if os.environ.get("CI_AFFECTED_DRY_RUN", "").lower() in ("1", "true"):
        return 0
    return subprocess.run(cmd, cwd=REPO).returncode


def run_clippy():
    """clippy the changed crates (all -> the whole workspace)."""
    mode = os.environ.get("MODE", "")
    crates = os.environ.get("CRATES", "").split()
    if mode == "none" or (mode == "subset" and not crates):
        print("No changed crates; skipping clippy.")
        return 0
    if mode == "all":
        return sh(["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"])
    cmd = ["cargo", "clippy"]
    for c in crates:
        cmd += ["-p", c]
    cmd += ["--all-targets", "--", "-D", "warnings"]
    return sh(cmd)


def run_test():
    """test the changed crates + dependents (all -> the whole workspace)."""
    mode = os.environ.get("MODE", "")
    crates = os.environ.get("CRATES", "").split()
    if mode == "none" or (mode == "subset" and not crates):
        print("No affected crates; skipping tests.")
        return 0
    if mode == "all":
        return sh(["cargo", "test", "--workspace", "--locked"])
    cmd = ["cargo", "test", "--locked"]
    for c in crates:
        cmd += ["-p", c]
    return sh(cmd)


def main():
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "detect":
        detect()
    elif cmd == "run-clippy":
        sys.exit(run_clippy())
    elif cmd == "run-test":
        sys.exit(run_test())
    else:
        sys.exit(f"usage: {sys.argv[0]} {{detect|run-clippy|run-test}}")


if __name__ == "__main__":
    main()


