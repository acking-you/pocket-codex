//! `pocket-codex version`.

use anyhow::Result;

/// Print the build version and a short banner.
pub fn run() -> Result<()> {
    println!("pocket-codex {} (work-in-progress bootstrap)", env!("CARGO_PKG_VERSION"));
    Ok(())
}
