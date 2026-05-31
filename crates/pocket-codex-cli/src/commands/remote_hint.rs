//! `pocket-codex remote-hint`: print the user-facing instructions a
//! remote operator should run on a client device to attach to the
//! relay-exposed `codex app-server`.

use anyhow::Result;

use crate::{
    cli::RemoteHintArgs,
    commands::{connect::codex_remote_command, ui},
};

/// Print a copy-pasteable hint.
pub fn run(args: RemoteHintArgs) -> Result<()> {
    let config = pocket_codex_core::config::Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
    for line in remote_hint_lines(&args, &relay) {
        if let Some(comment) = line.strip_prefix('#') {
            ui::muted(&format!("#{comment}"));
        } else if line.is_empty() {
            println!();
        } else {
            ui::code(&line);
        }
    }
    Ok(())
}

fn remote_hint_lines(args: &RemoteHintArgs, relay: &str) -> Vec<String> {
    vec![
        "# On the client device, run:".into(),
        format!(
            "pocket-codex connect --key {key} --local-addr {local} --relay {relay}",
            key = args.key,
            local = args.local_addr,
        ),
        String::new(),
        "# Then start Codex against the local subscriber listener:".into(),
        codex_remote_command(&args.local_addr),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::PbRelayArgs;

    #[test]
    fn remote_hint_lines_prefer_connect_and_codex_remote() {
        let lines = remote_hint_lines(
            &RemoteHintArgs {
                key: "codex".into(),
                local_addr: "127.0.0.1:28080".into(),
                relay: PbRelayArgs {
                    relay: Some("relay.example:7666".into()),
                },
            },
            "relay.example:7666",
        );

        assert!(lines.iter().any(|line| line
            == "pocket-codex connect --key codex --local-addr 127.0.0.1:28080 --relay \
                relay.example:7666"));
        assert!(lines
            .iter()
            .any(|line| line == "codex --remote ws://127.0.0.1:28080"));
    }
}
