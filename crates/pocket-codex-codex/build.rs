//! Bake the `deps/codex` submodule commit into the crate so the app can report
//! which codex the embedded (自带) app-server was built from. codex's own crate
//! version is a `0.0.0` placeholder (the real number is stamped at codex's
//! release time), so the short commit is the meaningful "version" for our fork.

use std::process::Command;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let codex_dir = format!("{manifest}/../../deps/codex");
    let commit = Command::new("git")
        .args(["-C", &codex_dir, "rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=EMBEDDED_CODEX_COMMIT={commit}");
    // Re-run when the submodule pointer moves (the superproject records it here).
    println!("cargo:rerun-if-changed={manifest}/../../.git/modules/deps/codex/HEAD");
}
