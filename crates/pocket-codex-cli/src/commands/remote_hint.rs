//! `pocket-codex remote-hint`: print the user-facing instructions a
//! remote operator should run on a client device to attach to the
//! relay-exposed `codex app-server`.

use anyhow::Result;

use crate::{cli::RemoteHintArgs, commands::connect::codex_remote_command};

/// Print a copy-pasteable hint.
pub fn run(args: RemoteHintArgs) -> Result<()> {
    for line in remote_hint_lines(&args) {
        println!("{line}");
    }
    Ok(())
}

fn remote_hint_lines(args: &RemoteHintArgs) -> Vec<String> {
    vec![
        "# On the client device, run:".into(),
        format!(
            "pocket-codex connect --key {key} --local-addr {local} --relay {relay}",
            key = args.key,
            local = args.local_addr,
            relay = args.relay.relay,
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
        let lines = remote_hint_lines(&RemoteHintArgs {
            key: "codex".into(),
            local_addr: "127.0.0.1:28080".into(),
            relay: PbRelayArgs {
                relay: "relay.example:7666".into(),
            },
        });

        assert!(lines.iter().any(|line| line
            == "pocket-codex connect --key codex --local-addr 127.0.0.1:28080 --relay \
                relay.example:7666"));
        assert!(lines
            .iter()
            .any(|line| line == "codex --remote ws://127.0.0.1:28080"));
    }
}
