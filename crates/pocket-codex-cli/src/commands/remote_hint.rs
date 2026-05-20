//! `pocket-codex remote-hint`: print the user-facing instructions a
//! remote operator should run on a client device to attach to the
//! relay-exposed `codex app-server`.

use anyhow::Result;

use crate::cli::RemoteHintArgs;

/// Print a copy-pasteable hint.
pub fn run(args: RemoteHintArgs) -> Result<()> {
    println!("# On the client device, run one of the following:");
    println!();
    println!("# 1. Using pocket-codex (recommended):");
    println!(
        "pocket-codex pb subscribe --key {key} --local-addr {local} --relay {relay}",
        key = args.key,
        local = args.local_addr,
        relay = args.relay.relay,
    );
    println!();
    println!("# 2. Using pb-mapper-client-cli directly:");
    println!(
        "pb-mapper-client-cli tcp-server --key {key} --addr {local} --pb-mapper-server {relay}",
        key = args.key,
        local = args.local_addr,
        relay = args.relay.relay,
    );
    println!();
    println!("# Then point your codex client (IDE plugin, custom UI, etc.) at:");
    println!("ws://{}", args.local_addr);
    Ok(())
}
