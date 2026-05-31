//! `pocket-codex version`.

use anyhow::Result;

use crate::commands::ui;

/// Print the build version and a short banner.
pub fn run() -> Result<()> {
    ui::banner("pocket-codex", Some(env!("CARGO_PKG_VERSION")));
    ui::muted("portable, multi-device codex app-server + Responses API relay");
    Ok(())
}
