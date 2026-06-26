//! Pocket-Codex hosted backend entrypoint.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let cfg = pocket_codex_backend::ServerConfig::load()?;
    pocket_codex_backend::run(cfg).await
}
