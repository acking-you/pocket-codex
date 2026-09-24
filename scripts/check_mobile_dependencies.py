#!/usr/bin/env python3
"""Keep server/runtime dependencies out of the Android bridge artifact."""
import subprocess

# Walk only normal dependencies: protocol conformance fixtures may be tested
# separately, but their runtime support must never enter the shipped cdylib.
tree = subprocess.check_output(
    ["cargo", "tree", "--locked", "-p", "pocket_codex_bridge", "--target",
     "aarch64-linux-android", "--edges", "normal", "--prefix", "none"],
    text=True,
)
names = {line.split()[0] for line in tree.splitlines() if line.strip()}
forbidden = {name for name in names if name.startswith("codex-") or name in {
    "starlark", "pagable", "openssl", "openssl-sys", "native-tls",
    "rusqlite", "libsqlite3-sys", "sqlx",
}}
if forbidden:
    raise SystemExit("Mobile bridge imports runtime dependencies: " + ", ".join(sorted(forbidden)))
print(f"Android dependency boundary passed ({len(names)} normal dependencies).")
