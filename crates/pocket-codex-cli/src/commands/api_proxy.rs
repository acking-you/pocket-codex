//! CLI-side helpers for the local Responses API proxy.
//!
//! The proxy server itself — the axum router that forwards `/v1/responses`
//! (HTTP + WebSocket) to ChatGPT's Codex backend — lives in the shared
//! [`pocket_codex_api_proxy`] crate so both the `api serve` worker subprocess
//! and the in-app host can run it. This module re-exports that surface and
//! keeps the CLI-only presentation (proxy-status warnings) used by `serve` and
//! `codex start`.

pub(crate) use pocket_codex_api_proxy::{redact_proxy, resolve_proxy, run, validate_proxy};
use url::Url;

use super::ui;

/// Report whether `raw` is a SOCKS proxy (`socks5://` / `socks5h://`).
/// codex's reqwest client has no SOCKS support, so a SOCKS proxy only
/// carries the model WebSocket — HTTP traffic (codex_apps, plugin sync)
/// still goes direct. Callers use this to warn the user. Returns `false`
/// for unparseable input; `validate_proxy` already rejects those.
fn proxy_is_socks(raw: &str) -> bool {
    Url::parse(raw).is_ok_and(|url| matches!(url.scheme(), "socks5" | "socks5h"))
}

/// Which spawn command surfaced an app-server proxy, used to tailor the
/// no-proxy / reuse warnings with the right command names.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SpawnCommand {
    /// `pocket-codex serve`.
    Serve,
    /// `pocket-codex codex start`.
    CodexStart,
}

impl SpawnCommand {
    /// Invocation a user would rerun with `--proxy`.
    fn invocation(self) -> &'static str {
        match self {
            Self::Serve => "pocket-codex serve",
            Self::CodexStart => "pocket-codex codex start",
        }
    }

    /// Stop command(s) that clear the supervised app-server before a
    /// proxy change can take effect on the next spawn.
    fn stop_hint(self) -> &'static str {
        match self {
            Self::Serve => "`pocket-codex stop` (or `pocket-codex codex stop`)",
            Self::CodexStart => "`pocket-codex codex stop`",
        }
    }
}

/// Resolve the effective upstream proxy for a spawned `codex app-server`
/// (explicit `--proxy` wins, then the standard proxy env vars) and
/// validate **only** an explicit `--proxy`.
///
/// Env-derived proxies are intentionally not validated here: when an
/// app-server is already alive, [`pocket_codex_codex::spawn`] reuses it
/// and never consumes the proxy, so failing fast on an inherited (and
/// possibly unsupported, e.g. `https://`) env value would needlessly
/// break the reuse path. An explicit `--proxy` is the user asking us to
/// use it now, so a bad scheme there is still a hard error.
pub(crate) fn resolve_app_server_proxy(explicit: Option<&str>) -> anyhow::Result<Option<String>> {
    let effective = resolve_proxy(explicit);
    let explicit_nonempty = explicit.map(str::trim).is_some_and(|v| !v.is_empty());
    if explicit_nonempty {
        if let Some(raw) = effective.as_deref() {
            validate_proxy(raw)?;
        }
    }
    Ok(effective)
}

/// Surface a spawned app-server's proxy posture: confirm an injected
/// proxy, warn when none is set, flag SOCKS' HTTP blind spot, and note
/// when a `--proxy` could not take effect because the process was reused.
pub(crate) fn print_proxy_status(
    effective: Option<&str>,
    proxy_requested: bool,
    reused: bool,
    command: SpawnCommand,
) {
    match effective {
        Some(raw) => {
            ui::field("proxy", &redact_proxy(raw));
            if proxy_is_socks(raw) {
                ui::warn(
                    "socks5 proxy carries only the model WebSocket. codex's reqwest client has \
                     no SOCKS support, so codex_apps and plugin sync stay direct and will time \
                     out on a blocked network. Use an `http://` proxy to fix codex_apps.",
                );
            }
        },
        None => ui::warn(&format!(
            "no upstream proxy configured. The codex app-server reaches chatgpt.com directly and \
             will fail on networks that block it (codex_apps bootstrap times out, model calls \
             stall). Pass `--proxy http://host:port`, or export HTTPS_PROXY / ALL_PROXY / \
             HTTP_PROXY before running `{}`.",
            command.invocation()
        )),
    }
    if reused && proxy_requested {
        ui::warn(&format!(
            "the codex app-server was already running, so this `--proxy` did not take effect. To \
             apply a new proxy, run {} first, then `{} --proxy …`.",
            command.stop_hint(),
            command.invocation()
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_app_server_proxy_validates_explicit_bad_scheme() {
        // An explicit `--proxy` with an unsupported scheme is a hard error:
        // the user asked us to use it now.
        assert!(resolve_app_server_proxy(Some("https://proxy.example:8443")).is_err());
        assert!(resolve_app_server_proxy(Some("ftp://nope")).is_err());
    }

    #[test]
    fn resolve_app_server_proxy_accepts_explicit_supported_schemes() {
        assert_eq!(
            resolve_app_server_proxy(Some("http://127.0.0.1:11111")).expect("http ok"),
            Some("http://127.0.0.1:11111".to_string())
        );
        assert_eq!(
            resolve_app_server_proxy(Some("socks5://127.0.0.1:1080")).expect("socks ok"),
            Some("socks5://127.0.0.1:1080".to_string())
        );
    }

    #[test]
    fn proxy_is_socks_detects_socks_schemes() {
        assert!(proxy_is_socks("socks5://127.0.0.1:1080"));
        assert!(proxy_is_socks("socks5h://127.0.0.1:1080"));
        assert!(!proxy_is_socks("http://127.0.0.1:11111"));
        assert!(!proxy_is_socks("not a url"));
    }
}
