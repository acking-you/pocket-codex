//! `pocket-codex serve` high-level host-side orchestration.
//!
//! ```text
//!                       pocket-codex serve …
//!                                │
//!                                ▼
//!                ServiceId::new(device, App, name).key()
//!                          (or args.key)
//!                                │
//!                                ▼
//!              pocket_codex_codex::spawn(SpawnOptions)
//!                                │
//!                                ▼
//!              websocket_listen_addr("ws://host:port")
//!                                │
//!                                ▼
//!              managed_pb::ensure(PbWorkerSpec {
//!                role:  PbRole::Register,
//!                key,
//!                local_addr,
//!                relay_addr,
//!                codec,
//!              })
//!                                │
//!                                ▼
//!              print_serve_summary + "client setup: pocket-codex
//!              connect --key <key> --relay <relay>"
//! ```
//!
//! `serve` is the host side of an app-server pairing: it owns the
//! `codex app-server` child and the pb-mapper register worker that
//! exposes its WebSocket. Non-WebSocket listen URLs (for example unix
//! sockets) are rejected because pb-mapper needs a relayable TCP
//! endpoint.

use anyhow::{Context, Result};
use pocket_codex_codex::{spawn, ListenSpec, SpawnOptions};
use pocket_codex_core::{
    service::{default_device_id, ServiceId, ServiceKind},
    state::PbRole,
};

use crate::{
    cli::ServeArgs,
    commands::{
        api_proxy,
        managed_pb::{self, EnsureOutcome, PbWorkerSpec},
        ui,
    },
};

/// Run the host-side one-shot setup flow.
pub async fn run(args: ServeArgs) -> Result<()> {
    let key = args.key.clone().unwrap_or_else(|| {
        ServiceId::new(
            args.device.clone().unwrap_or_else(default_device_id),
            ServiceKind::App,
            &args.name,
        )
        .key()
    });

    // Resolve the effective upstream proxy once (explicit flag or env) so we
    // fail fast on a bad scheme and can surface it. The spawned app-server
    // reads proxy settings only from its environment, never from codex's
    // config.toml, so we inject it there via SpawnOptions.
    let proxy_requested = args.proxy.is_some();
    let effective_proxy = api_proxy::resolve_proxy(args.proxy.as_deref());
    if let Some(raw) = effective_proxy.as_deref() {
        api_proxy::validate_proxy(raw)?;
    }

    let requested_listen = ListenSpec::WebSocket {
        host: args.host,
        port: args.port,
    };
    let report = spawn(SpawnOptions {
        binary: args.codex_binary,
        listen: requested_listen,
        extra_args: args.extra,
        log_file: None,
        proxy: effective_proxy.clone(),
    })?;
    let local_addr = websocket_listen_addr(&report.info.listen).with_context(|| {
        format!("codex listen URL `{}` is not relayable TCP", report.info.listen)
    })?;

    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Register,
        key: key.clone(),
        local_addr,
        relay_addr: args.relay.relay.clone(),
        codec: args.codec,
    })?;
    print_serve_summary(
        &report.info,
        &outcome,
        &key,
        &args.relay.relay,
        effective_proxy.as_deref(),
        proxy_requested,
        report.reused,
    );
    Ok(())
}

fn print_serve_summary(
    codex: &pocket_codex_core::state::CodexProcessInfo,
    pb: &EnsureOutcome,
    key: &str,
    relay: &str,
    effective_proxy: Option<&str>,
    proxy_requested: bool,
    reused: bool,
) {
    ui::headline(ui::Tone::Ok, "codex app-server");
    ui::field("pid", &codex.pid.to_string());
    ui::field("listen", &codex.listen);
    ui::field("log", &codex.log_file.display().to_string());
    print_proxy_status(effective_proxy, proxy_requested, reused);
    pb.render("pb register");
    ui::headline(ui::Tone::Action, "client setup");
    ui::code(&format!("pocket-codex connect --key {key} --relay {relay}"));
}

/// Surface the spawned app-server's proxy posture: confirm an injected
/// proxy, warn when none is set, flag SOCKS' HTTP blind spot, and note
/// when a `--proxy` could not take effect because the process was reused.
fn print_proxy_status(effective: Option<&str>, proxy_requested: bool, reused: bool) {
    match effective {
        Some(raw) => {
            ui::field("proxy", &api_proxy::redact_proxy(raw));
            if api_proxy::proxy_is_socks(raw) {
                ui::warn(
                    "socks5 proxy carries only the model WebSocket. codex's reqwest client has \
                     no SOCKS support, so codex_apps and plugin sync stay direct and will time \
                     out on a blocked network. Use an `http://` proxy to fix codex_apps.",
                );
            }
        },
        None => ui::warn(
            "no upstream proxy configured. The codex app-server reaches chatgpt.com directly and \
             will fail on networks that block it (codex_apps bootstrap times out, model calls \
             stall). Pass `--proxy http://host:port`, or export HTTPS_PROXY / ALL_PROXY / \
             HTTP_PROXY before running `pocket-codex serve`.",
        ),
    }
    if reused && proxy_requested {
        ui::warn(
            "the codex app-server was already running, so this `--proxy` did not take effect. To \
             apply a new proxy, run `pocket-codex stop` (or `pocket-codex codex stop`) first, \
             then `pocket-codex serve --proxy …`.",
        );
    }
}

fn websocket_listen_addr(listen: &str) -> Option<String> {
    listen
        .strip_prefix("ws://")
        .filter(|addr| !addr.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn websocket_listen_addr_extracts_tcp_addr() {
        assert_eq!(
            websocket_listen_addr("ws://127.0.0.1:18080").as_deref(),
            Some("127.0.0.1:18080")
        );
        assert_eq!(websocket_listen_addr("unix:///tmp/codex.sock"), None);
    }
}
