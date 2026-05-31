# `pocket-codex init` + relay config + connect timeout — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add an interactive `pocket-codex init` that persists the relay URL + `MSG_HEADER_KEY` to `config.toml` (0600), make those the default for all commands (precedence `flag > config > env`), and bound the relay connect so a dead relay fails in ~5s instead of hanging ~123s.

**Architecture:** Three crates change. `pocket-codex-core` gains a `key` field on `PbMapperConfig`, 0600 save, and relay/key accessors. `pocket-codex-pb` wraps the upstream `set_process_msg_header_key` and adds a connect timeout to `query_status`. `pocket-codex-cli` adds the `init` command, a `relay` resolution module, a `dispatch` key-application hook, and switches every relay call site from a required arg to `resolve_relay(flag, &config)`.

**Tech Stack:** Rust 2021, clap, tokio (`features=["full"]`), toml, anyhow, the vendored pb-mapper crate.

**Spec:** `docs/superpowers/specs/2026-05-31-relay-init-design.md`

---

## File Structure

- `crates/pocket-codex-core/src/config.rs` — add `PbMapperConfig.key`, accessors, 0600 save.
- `crates/pocket-codex-pb/src/session.rs` — connect timeout in `query_status`; wrap `set_process_msg_header_key`.
- `crates/pocket-codex-pb/src/lib.rs` — re-export the wrapper.
- `crates/pocket-codex-cli/src/cli.rs` — `Init` command + `InitArgs`; `PbRelayArgs.relay` → `Option<String>`.
- `crates/pocket-codex-cli/src/commands/relay.rs` — NEW: `resolve_relay`, `apply_configured_key`, `normalize_relay`, `validate_key`.
- `crates/pocket-codex-cli/src/commands/init.rs` — NEW: interactive `init` flow.
- `crates/pocket-codex-cli/src/commands/mod.rs` — register modules, dispatch hook + `Init` arm.
- `crates/pocket-codex-cli/src/commands/{services,connect,pb,serve,api,worker,remote_hint}.rs` — call `resolve_relay` (all 7 relay call sites).
- `README.md`, `docs/cli-verification.md` — document `init`, precedence, timeout.

---

### Task 1: `PbMapperConfig.key` + relay/key accessors + 0600 save

**Files:**
- Modify: `crates/pocket-codex-core/src/config.rs`
- Test: `crates/pocket-codex-core/src/config.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing tests**

Add to the existing `mod tests` block in `config.rs`:

```rust
    #[test]
    fn relay_and_key_roundtrip_in_config() {
        let mut config = Config::default();
        assert_eq!(config.relay(), None);
        assert_eq!(config.relay_key(), None);

        config.set_relay("lb7666.top:7666");
        config.set_relay_key("0123456789abcdef0123456789abcdef");

        assert_eq!(config.relay(), Some("lb7666.top:7666"));
        assert_eq!(config.relay_key(), Some("0123456789abcdef0123456789abcdef"));

        let raw = toml::to_string_pretty(&config).expect("serialize");
        let reloaded: Config = toml::from_str(&raw).expect("deserialize");
        assert_eq!(reloaded.relay(), Some("lb7666.top:7666"));
        assert_eq!(reloaded.relay_key(), Some("0123456789abcdef0123456789abcdef"));
    }

    #[test]
    fn empty_relay_or_key_is_treated_as_unset() {
        let mut config = Config::default();
        config.set_relay("   ");
        config.set_relay_key("");
        assert_eq!(config.relay(), None);
        assert_eq!(config.relay_key(), None);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p pocket-codex-core relay_and_key --locked`
Expected: FAIL — `no method named relay`/`set_relay` on `Config`.

- [ ] **Step 3: Add the `key` field and accessors**

In `config.rs`, extend `PbMapperConfig`:

```rust
/// Configuration for the `pb-mapper` integration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PbMapperConfig {
    /// Bare `host:port` of the upstream `pb-mapper` relay
    /// (e.g. `relay.example.com:7666`).
    pub relay: Option<String>,

    /// Shared 32-byte `MSG_HEADER_KEY` the relay validates every control
    /// message against. Stored here so commands default to it without an
    /// exported environment variable.
    pub key: Option<String>,
}
```

Add these methods inside `impl Config` (after `set_default_service`):

```rust
    /// Configured relay `host:port`, or `None` when unset/blank.
    pub fn relay(&self) -> Option<&str> {
        self.pb_mapper.relay.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    /// Configured `MSG_HEADER_KEY`, or `None` when unset/blank.
    pub fn relay_key(&self) -> Option<&str> {
        self.pb_mapper.key.as_deref().map(str::trim).filter(|s| !s.is_empty())
    }

    /// Set the relay `host:port`. A blank value clears it.
    pub fn set_relay(&mut self, relay: impl AsRef<str>) {
        let trimmed = relay.as_ref().trim();
        self.pb_mapper.relay = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }

    /// Set the shared `MSG_HEADER_KEY`. A blank value clears it.
    pub fn set_relay_key(&mut self, key: impl AsRef<str>) {
        let trimmed = key.as_ref().trim();
        self.pb_mapper.key = (!trimmed.is_empty()).then(|| trimmed.to_string());
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p pocket-codex-core relay_and_key empty_relay_or_key --locked`
Expected: PASS (both tests).

- [ ] **Step 5: Tighten `save()` to 0600 on unix**

Replace the body of `Config::save` in `config.rs` with:

```rust
    /// Persist configuration to the default location. On unix the file is
    /// written with `0o600` because it may hold the relay `MSG_HEADER_KEY`.
    pub fn save(&self) -> Result<()> {
        let path = paths::config_file()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = toml::to_string_pretty(self)?;
        std::fs::write(&path, raw)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }
```

- [ ] **Step 6: Verify the whole core crate still builds + tests**

Run: `cargo test -p pocket-codex-core --locked`
Expected: PASS (all tests).

- [ ] **Step 7: Commit**

```bash
git add crates/pocket-codex-core/src/config.rs
git commit -m "feat(core): add relay key to config, accessors, 0600 save"
```

---

### Task 2: pb crate — connect timeout in `query_status`

**Files:**
- Modify: `crates/pocket-codex-pb/src/session.rs`
- Test: `crates/pocket-codex-pb/src/session.rs` (inline `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Add a test module at the end of `session.rs`:

```rust
#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    #[tokio::test]
    async fn connect_relay_errors_fast_on_unreachable_addr() {
        // 192.0.2.1 is RFC 5737 TEST-NET-1: routable-looking but black-holed,
        // so connect() neither succeeds nor RSTs quickly. Bound it ourselves.
        let addr: SocketAddr = "192.0.2.1:7666".parse().expect("addr");
        let result =
            tokio::time::timeout(Duration::from_secs(3), connect_relay(addr, Duration::from_millis(200)))
                .await;
        // Outer timeout must NOT fire: connect_relay's own bound returns first.
        let inner = result.expect("connect_relay hung past its own timeout");
        assert!(inner.is_err(), "expected a connect error/timeout, got Ok");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pocket-codex-pb connect_relay_errors_fast --locked`
Expected: FAIL — `cannot find function connect_relay`.

- [ ] **Step 3: Add `connect_relay` and route `query_status` through it**

In `session.rs`, add imports near the top (merge into existing `use` groups):

```rust
use std::time::Duration;

use tokio::time::timeout;
```

Add the default constant just above `query_status`:

```rust
/// Default bound for establishing the relay control connection. Without
/// it, a dead/black-holed relay makes `TcpStream::connect` hang on the
/// kernel SYN retry budget (~123s) and the CLI looks frozen.
const RELAY_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Connect to the relay, bounding the attempt by `dur`. Split out so the
/// timeout behaviour is unit-testable without a real dead relay.
async fn connect_relay(relay_addr: SocketAddr, dur: Duration) -> Result<TcpStream> {
    match timeout(dur, TcpStream::connect(relay_addr)).await {
        Ok(result) => result.with_context(|| format!("connecting to pb-mapper relay {relay_addr}")),
        Err(_) => Err(anyhow!(
            "connecting to pb-mapper relay {relay_addr} timed out after {dur:?}"
        )),
    }
}
```

Replace the `TcpStream::connect(...)` call inside `query_status` so it reads:

```rust
async fn query_status(relay_addr: SocketAddr, req: PbConnStatusReq) -> Result<PbConnStatusResp> {
    let mut stream = connect_relay(relay_addr, RELAY_CONNECT_TIMEOUT).await?;
    get_status(&mut stream, req)
        .await
        .map_err(|err| anyhow!("querying pb-mapper relay status: {err}"))
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p pocket-codex-pb connect_relay_errors_fast --locked`
Expected: PASS (returns within 3s with an error).

- [ ] **Step 5: Commit**

```bash
git add crates/pocket-codex-pb/src/session.rs
git commit -m "fix(pb): bound relay connect so a dead relay fails fast"
```

---

### Task 3: pb crate — wrap and re-export `set_process_msg_header_key`

**Files:**
- Modify: `crates/pocket-codex-pb/src/session.rs`
- Modify: `crates/pocket-codex-pb/src/lib.rs`
- Test: `crates/pocket-codex-pb/src/session.rs` (extend the `mod tests` from Task 2)

- [ ] **Step 1: Write the failing test**

Add to the `mod tests` block in `session.rs`:

```rust
    #[test]
    fn set_msg_header_key_rejects_wrong_length() {
        // Validation happens before any global mutation, so this is safe to
        // run in parallel: a 5-byte key can never be accepted.
        assert!(set_msg_header_key(Some("short")).is_err());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p pocket-codex-pb set_msg_header_key_rejects --locked`
Expected: FAIL — `cannot find function set_msg_header_key`.

- [ ] **Step 3: Add the wrapper**

In `session.rs`, add this function (place it after `subscribe`, before the `StatusKind` enum):

```rust
/// Apply the shared `MSG_HEADER_KEY` to this process and the environment
/// child workers inherit.
///
/// Thin wrapper over the upstream
/// [`pb_mapper::common::checksum::set_process_msg_header_key`]: it both
/// sets the `MSG_HEADER_KEY` env var (so spawned `__worker` children pick
/// it up) and updates pb-mapper's in-process key (so calls made from this
/// process validate too). `Some(non-empty)` must be exactly 32 bytes;
/// `None`/empty resets to the upstream default. Errors are surfaced as
/// `anyhow` so callers stay decoupled from pb-mapper's error type.
pub fn set_msg_header_key(key: Option<&str>) -> Result<()> {
    pb_mapper::common::checksum::set_process_msg_header_key(key)
        .map_err(|err| anyhow!("applying MSG_HEADER_KEY: {err}"))
}
```

- [ ] **Step 4: Re-export from the crate root**

In `lib.rs`, extend the `pub use session::{...}` list to include `set_msg_header_key`:

```rust
pub use session::{
    keys, register, service_connections, set_msg_header_key, status, subscribe, RegisterOptions,
    StatusKind, SubscribeOptions,
};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test -p pocket-codex-pb set_msg_header_key_rejects --locked`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/pocket-codex-pb/src/session.rs crates/pocket-codex-pb/src/lib.rs
git commit -m "feat(pb): expose set_msg_header_key wrapper"
```

---

### Task 4: relay resolution — `relay.rs`, `PbRelayArgs` → optional, dispatch hook, all call sites

This is one cohesive commit: making `PbRelayArgs.relay` optional breaks all 7
call sites at once, so they migrate together. The crate only compiles again
after every site is updated — the green checkpoint is Step 11.

**Files:**
- Create: `crates/pocket-codex-cli/src/commands/relay.rs`
- Modify: `crates/pocket-codex-cli/src/cli.rs` (PbRelayArgs + 4 test asserts)
- Modify: `crates/pocket-codex-cli/src/commands/mod.rs` (register module + hook)
- Modify: `crates/pocket-codex-cli/src/commands/{services,connect,pb,serve,api,worker,remote_hint}.rs`

- [ ] **Step 1: Write the pure-resolution unit test**

Put this in the new file `crates/pocket-codex-cli/src/commands/relay.rs` as its
`#[cfg(test)] mod tests` (implementation lands in Step 2):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_wins_over_config_and_env() {
        let r = resolve_relay_from(Some("flag:1"), Some("cfg:2"), Some("env:3")).unwrap();
        assert_eq!(r, "flag:1");
    }

    #[test]
    fn config_wins_over_env_when_no_flag() {
        let r = resolve_relay_from(None, Some("cfg:2"), Some("env:3")).unwrap();
        assert_eq!(r, "cfg:2");
    }

    #[test]
    fn env_used_when_no_flag_or_config() {
        let r = resolve_relay_from(None, None, Some("env:3")).unwrap();
        assert_eq!(r, "env:3");
    }

    #[test]
    fn blank_candidates_are_skipped_then_error() {
        assert_eq!(resolve_relay_from(Some("  "), None, Some("env:3")).unwrap(), "env:3");
        assert!(resolve_relay_from(None, None, None).is_err());
        assert!(resolve_relay_from(Some(""), Some("  "), Some("")).is_err());
    }
}
```
<!--NEXT-->

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p pocket-codex-cli flag_wins_over_config --locked`
Expected: FAIL — `relay` module / `resolve_relay_from` not found (won't compile yet).

- [ ] **Step 3: Implement `relay.rs` (resolution + key hook)**

Prepend this above the test module written in Step 1:

```rust
//! Relay address + shared-key resolution for `pocket-codex` commands.
//!
//! Precedence is `flag > config > $PB_MAPPER_SERVER`. The shared
//! `MSG_HEADER_KEY` is applied once per process in [`apply_configured_key`]
//! so both in-process relay queries and spawned `__worker` children agree
//! on it.

use anyhow::{anyhow, Result};
use pocket_codex_core::config::Config;

/// Environment variable pb-mapper and the CLI both read for the relay.
const RELAY_ENV: &str = "PB_MAPPER_SERVER";

/// Pure precedence resolver, factored out for testing.
fn resolve_relay_from(
    flag: Option<&str>,
    config: Option<&str>,
    env: Option<&str>,
) -> Result<String> {
    [flag, config, env]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            anyhow!("no relay configured; run `pocket-codex init` or pass --relay <host:port>")
        })
}

/// Resolve the effective relay from an explicit flag and loaded config,
/// falling back to `$PB_MAPPER_SERVER`.
pub(crate) fn resolve_relay(flag: Option<&str>, config: &Config) -> Result<String> {
    let env = std::env::var(RELAY_ENV).ok();
    resolve_relay_from(flag, config.relay(), env.as_deref())
}
```

- [ ] **Step 4: Add the key-application hook to `relay.rs`**

Append below `resolve_relay` (still above the tests):

```rust
/// Apply the configured `MSG_HEADER_KEY` to this process once, before any
/// relay traffic or worker spawn. No-op when config has no key (the
/// existing `$MSG_HEADER_KEY` env, if any, then stands). Best-effort: a
/// broken config must not stop offline commands like `version`.
pub(crate) fn apply_configured_key() {
    let Ok(config) = Config::load() else {
        return;
    };
    if let Some(key) = config.relay_key() {
        if let Err(err) = pocket_codex_pb::set_msg_header_key(Some(key)) {
            tracing::warn!("ignoring configured relay key: {err}");
        }
    }
}
```
<!--NEXT-->

- [ ] **Step 5: Register the module + dispatch hook in `mod.rs`**

In `crates/pocket-codex-cli/src/commands/mod.rs`, add `mod relay;` to the module
list (alphabetical, after `mod pb;`). Then change `dispatch` so the key hook runs
before any command:

```rust
/// Dispatch a parsed [`Cli`] invocation to the matching subcommand.
pub async fn dispatch(cli: Cli) -> Result<()> {
    // Apply the configured MSG_HEADER_KEY once, before any relay traffic or
    // worker spawn, so this process and its children agree on it.
    relay::apply_configured_key();

    match cli.command {
        Command::Serve(args) => serve::run(args).await,
        Command::Connect(args) => connect::run(args).await,
        Command::Api(cmd) => api::run(cmd).await,
        Command::Services(cmd) => services::run(cmd).await,
        Command::Status => status::run(),
        Command::Stop(args) => stop::run(args),
        Command::Version => version::run(),
        Command::Codex(cmd) => codex::run(cmd).await,
        Command::Pb(cmd) => pb::run(cmd).await,
        Command::RemoteHint(args) => remote_hint::run(args),
        Command::Worker(cmd) => worker::run(cmd).await,
    }
}
```

(The `Init` arm is added in Task 5; leave the match as above for now.)

- [ ] **Step 6: Make `PbRelayArgs.relay` optional in `cli.rs`**

Replace the `PbRelayArgs` struct in `cli.rs`:

```rust
/// Common pb-mapper relay locator.
#[derive(Debug, Args, Clone)]
pub struct PbRelayArgs {
    /// `host:port` of the upstream pb-mapper relay. When omitted, falls
    /// back to the configured relay (`pocket-codex init`) and then
    /// `$PB_MAPPER_SERVER`.
    #[arg(long)]
    pub relay: Option<String>,
}
```

Update the four `cli.rs` test assertions that read `args.relay.relay`:

- `serve_parses_high_level_host_flow_defaults`:
  `assert_eq!(args.relay.relay.as_deref(), Some("relay.example:7666"));`
- `connect_parses_high_level_client_flow_defaults`:
  `assert_eq!(args.relay.relay.as_deref(), Some("relay.example:7666"));`
- `api_serve_parses_device_service_defaults`:
  `assert_eq!(args.relay.relay.as_deref(), Some("relay.example:7666"));`
- `hidden_worker_parses_pb_register_args`:
  `assert_eq!(args.relay.relay.as_deref(), Some("relay.example:7666"));`

- [ ] **Step 7: Update `services.rs` call site**

In `services::list`, resolve before discovery:

```rust
async fn list(args: ServicesListArgs) -> Result<()> {
    let kind = args.kind.map(ServiceKind::from);
    let config = pocket_codex_core::config::Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
    let mut services = discover_services(&relay).await?;
    services.retain(|id| kind.is_none_or(|kind| id.kind == kind));
    services.sort_by_key(|id| id.key());
    // ... unchanged below ...
```

(Leave the rest of `list` as-is.)
<!--NEXT-->

- [ ] **Step 8: Update `connect.rs` call site**

In `connect::run`, resolve once and reuse for discovery + the worker spec.
Replace the body from the `let config = Config::load()?;` line through the
`managed_pb::ensure(...)` call so it reads:

```rust
    let needs_discovery = request.key.is_none() && request.device.is_none();
    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
    let state = RuntimeState::load()?;
    let has_local_default = config.default_service(ServiceKind::App).is_some()
        || state.selected_service(ServiceKind::App).is_some();
    let discovered = if needs_discovery && !has_local_default {
        discover_services(&relay).await?
    } else {
        Vec::new()
    };
    let target = choose_target(ServiceKind::App, request, &config, &state, &discovered)?;
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Subscribe,
        key: target.key,
        local_addr: args.local_addr,
        relay_addr: relay,
        codec: false,
    })?;
```

- [ ] **Step 9: Update `pb.rs` call sites (register, subscribe, status)**

In `pb.rs`, add `use pocket_codex_core::config::Config;` to the imports. Then
resolve the relay at the top of each handler.

`register`:
```rust
async fn register(args: PbRegisterArgs) -> Result<()> {
    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
    let opts = RegisterOptions {
        key: args.key.clone(),
        local_addr: args.local_addr.clone(),
        relay_addr: relay,
        codec: args.codec,
    };
    // ... ui::* and pb_register(opts).await unchanged ...
```

`subscribe`:
```rust
async fn subscribe(args: PbSubscribeArgs) -> Result<()> {
    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
    let opts = SubscribeOptions {
        key: args.key.clone(),
        local_addr: args.local_addr.clone(),
        relay_addr: relay,
    };
    // ... ui::* and pb_subscribe(opts).await unchanged ...
```

`status`:
```rust
async fn status(args: PbStatusArgs) -> Result<()> {
    let kind = match args.kind {
        PbStatusKind::Keys => StatusKind::Keys,
        PbStatusKind::RemoteId => StatusKind::RemoteId,
    };
    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
    let addr = resolve_one(&relay).await?;
    pb_status(addr, kind).await;
    Ok(())
}
```
<!--NEXT-->

- [ ] **Step 10: Update `serve.rs` call site**

In `serve.rs`, add `config::Config` to the existing `pocket_codex_core::{...}`
import group (so it reads `use pocket_codex_core::{config::Config,
service::{...}, state::PbRole};`). In `run`, resolve the relay right after
computing `key` (before `spawn`):

```rust
    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
```

Change the `managed_pb::ensure` `relay_addr` field and the
`print_serve_summary` relay argument to use `relay`:

```rust
    let outcome = managed_pb::ensure(PbWorkerSpec {
        role: PbRole::Register,
        key: key.clone(),
        local_addr,
        relay_addr: relay.clone(),
        codec: args.codec,
    })?;
    print_serve_summary(
        &report.info,
        &outcome,
        &key,
        &relay,
        effective_proxy.as_deref(),
        proxy_requested,
        report.reused,
    );
```

- [ ] **Step 11: Update `api.rs` call sites (serve + connect)**

`api::serve` is sync; resolve near the top after computing `local_addr`:

```rust
    let config = Config::load()?;
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
```

Then in its `managed_pb::ensure` use `relay_addr: relay.clone()`, and in
`print_serve_summary` pass `&relay` instead of `&args.relay.relay`.

`api::connect` already loads `config`; add right after it:

```rust
    let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
```

Then change `discover_services(&args.relay.relay)` → `discover_services(&relay)`
and the `managed_pb::ensure` `relay_addr: args.relay.relay` → `relay_addr: relay`.

(`Config` is already imported in `api.rs`.)

- [ ] **Step 12: Update `worker.rs` call sites**

The parent always passes `--relay` to workers, but route through `resolve_relay`
for one consistent path. In `worker.rs`, add
`use pocket_codex_core::config::Config;` and resolve per arm:

```rust
        WorkerCmd::PbRegister(args) => {
            let config = Config::load()?;
            let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
            pb_register(RegisterOptions {
                key: args.key,
                local_addr: args.local_addr,
                relay_addr: relay,
                codec: args.codec,
            })
            .await;
        },
        WorkerCmd::PbSubscribe(args) => {
            let config = Config::load()?;
            let relay = crate::commands::relay::resolve_relay(args.relay.relay.as_deref(), &config)?;
            pb_subscribe(SubscribeOptions {
                key: args.key,
                local_addr: args.local_addr,
                relay_addr: relay,
            })
            .await;
        },
```

(`WorkerCmd::ApiProxy` arm unchanged.)
<!--NEXT-->

- [ ] **Step 13: Update `remote_hint.rs` (sync command + its test)**

`remote_hint::run` is sync; load config and resolve, then thread the resolved
relay into `remote_hint_lines` (which changes signature to take `relay: &str`).

```rust
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
```

Update its test to pass `Some(..)` for the optional relay and the new
`relay` arg:

```rust
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
```

- [ ] **Step 14: Build the whole workspace (first green checkpoint)**

Run: `cargo build --workspace --locked`
Expected: compiles clean (no `args.relay.relay` type errors remaining).

- [ ] **Step 15: Run CLI + workspace tests**

Run: `cargo test -p pocket-codex-cli --locked`
Expected: PASS, including the four new `relay::tests` and the updated asserts.

- [ ] **Step 16: Commit**

```bash
git add crates/pocket-codex-cli/src/cli.rs \
        crates/pocket-codex-cli/src/commands/relay.rs \
        crates/pocket-codex-cli/src/commands/mod.rs \
        crates/pocket-codex-cli/src/commands/services.rs \
        crates/pocket-codex-cli/src/commands/connect.rs \
        crates/pocket-codex-cli/src/commands/pb.rs \
        crates/pocket-codex-cli/src/commands/serve.rs \
        crates/pocket-codex-cli/src/commands/api.rs \
        crates/pocket-codex-cli/src/commands/worker.rs \
        crates/pocket-codex-cli/src/commands/remote_hint.rs
git commit -m "feat(cli): resolve relay via flag>config>env, apply configured key"
```

---

### Task 5: `pocket-codex init` command

**Files:**
- Create: `crates/pocket-codex-cli/src/commands/init.rs`
- Modify: `crates/pocket-codex-cli/src/cli.rs` (Init command + InitArgs)
- Modify: `crates/pocket-codex-cli/src/commands/mod.rs` (mod + dispatch arm)
- Test: `crates/pocket-codex-cli/src/commands/init.rs` (inline tests)

- [ ] **Step 1: Write failing tests for the pure helpers**

Create `crates/pocket-codex-cli/src/commands/init.rs` containing only its test
module for now (implementation lands in Step 3):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_relay_strips_scheme_and_keeps_host_port() {
        assert_eq!(normalize_relay("tcp://lb7666.top:7666").unwrap(), "lb7666.top:7666");
        assert_eq!(normalize_relay("  lb7666.top:7666  ").unwrap(), "lb7666.top:7666");
        assert_eq!(normalize_relay("tcp://1.2.3.4:7666/").unwrap(), "1.2.3.4:7666");
    }

    #[test]
    fn normalize_relay_rejects_missing_port_or_host() {
        assert!(normalize_relay("lb7666.top").is_err());
        assert!(normalize_relay(":7666").is_err());
        assert!(normalize_relay("lb7666.top:notaport").is_err());
    }

    #[test]
    fn validate_key_requires_32_bytes() {
        assert!(validate_key("short").is_err());
        let ok = "0123456789abcdef0123456789abcdef";
        assert_eq!(validate_key(ok).unwrap(), ok);
        assert_eq!(ok.len(), 32);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p pocket-codex-cli normalize_relay --locked`
Expected: FAIL — `init` module / `normalize_relay` not found (won't compile).

- [ ] **Step 3: Implement `init.rs` (header + pure helpers)**

Prepend above the test module:

```rust
//! `pocket-codex init`: interactively persist the default relay + key.
//!
//! Stores `host:port` and the shared `MSG_HEADER_KEY` in `config.toml`
//! (0600) so later commands need neither `--relay` nor an exported
//! `$MSG_HEADER_KEY`. Verifies reachability before saving unless
//! `--no-verify` is passed.

use anyhow::{anyhow, bail, Result};
use pocket_codex_core::config::Config;

use crate::{
    cli::InitArgs,
    commands::{service_target::discover_services, ui},
};

/// Strip an optional `tcp://` scheme and validate `host:port`.
pub(crate) fn normalize_relay(input: &str) -> Result<String> {
    let trimmed = input.trim();
    let bare = trimmed.strip_prefix("tcp://").unwrap_or(trimmed).trim_end_matches('/');
    let (host, port) =
        bare.rsplit_once(':').ok_or_else(|| anyhow!("relay `{input}` must be host:port"))?;
    if host.is_empty() {
        bail!("relay `{input}` is missing a host");
    }
    port.parse::<u16>().map_err(|_| anyhow!("relay `{input}` has an invalid port"))?;
    Ok(bare.to_string())
}

/// Validate the shared key is exactly 32 bytes (pb-mapper's requirement).
pub(crate) fn validate_key(input: &str) -> Result<String> {
    let trimmed = input.trim();
    if trimmed.len() != 32 {
        bail!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", trimmed.len());
    }
    Ok(trimmed.to_string())
}
```
<!--NEXT-->

- [ ] **Step 4: Implement the interactive prompts + `run`**

Append below the pure helpers (still above the tests):

```rust
/// Read one line for `label`, showing `default_hint` in brackets. Returns
/// the trimmed input, or `None` when the user accepts the default (blank).
/// Errors if stdin is not a TTY (so non-interactive callers fail clearly).
fn prompt(label: &str, default_hint: Option<&str>) -> Result<Option<String>> {
    use std::io::{stdin, stdout, IsTerminal, Write};
    if !stdin().is_terminal() {
        bail!("non-interactive environment: pass --relay and --key");
    }
    match default_hint {
        Some(hint) => print!("{label} [{hint}]: "),
        None => print!("{label}: "),
    }
    stdout().flush()?;
    let mut line = String::new();
    stdin().read_line(&mut line)?;
    let trimmed = line.trim();
    Ok((!trimmed.is_empty()).then(|| trimmed.to_string()))
}

/// Run `pocket-codex init`.
pub async fn run(args: InitArgs) -> Result<()> {
    let mut config = Config::load()?;

    // Relay: flag, else prompt (default = existing config value).
    let relay_input = match args.relay {
        Some(r) => r,
        None => prompt("relay (host:port)", config.relay())?
            .or_else(|| config.relay().map(str::to_string))
            .ok_or_else(|| anyhow!("a relay is required"))?,
    };
    let relay = normalize_relay(&relay_input)?;

    // Key: flag, else prompt. Existing key shown as "keep current".
    let key_hint = config.relay_key().map(|_| "keep current");
    let key_input = match args.key {
        Some(k) => k,
        None => prompt("MSG_HEADER_KEY (32 bytes)", key_hint)?
            .or_else(|| config.relay_key().map(str::to_string))
            .ok_or_else(|| anyhow!("a 32-byte MSG_HEADER_KEY is required"))?,
    };
    let key = validate_key(&key_input)?;

    if !args.no_verify {
        // Apply the new key to this process so discovery validates with it,
        // then probe the relay (bounded by the connect timeout from Task 2).
        pocket_codex_pb::set_msg_header_key(Some(&key))?;
        match discover_services(&relay).await {
            Ok(found) => ui::field("verified", &format!("reached relay, {} service(s)", found.len())),
            Err(err) => bail!(
                "could not reach relay `{relay}`: {err}\n\
                 fix the relay/key, or re-run with --no-verify to save anyway"
            ),
        }
    }

    config.set_relay(&relay);
    config.set_relay_key(&key);
    config.save()?;

    ui::headline(ui::Tone::Ok, "relay configured");
    ui::field("relay", &relay);
    ui::field("key", &format!("len={}", key.len()));
    ui::field("config", &pocket_codex_core::paths::config_file()?.display().to_string());
    Ok(())
}
```
<!--NEXT-->

- [ ] **Step 5: Add the `Init` command + `InitArgs` to `cli.rs`**

Add a variant to the `Command` enum (place it first, before `Serve`, since it's
the bootstrap step):

```rust
    /// Interactively configure the default relay URL and shared key.
    Init(InitArgs),
```

Add the args struct (place it just above `ServeArgs`):

```rust
/// Args for `pocket-codex init`.
#[derive(Debug, Args)]
pub struct InitArgs {
    /// Relay `host:port` (or `tcp://host:port`). Prompted when omitted
    /// on a TTY.
    #[arg(long)]
    pub relay: Option<String>,

    /// Shared 32-byte `MSG_HEADER_KEY`. Prompted when omitted on a TTY.
    #[arg(long)]
    pub key: Option<String>,

    /// Skip the post-save reachability check against the relay.
    #[arg(long)]
    pub no_verify: bool,
}
```

- [ ] **Step 6: Add a `cli.rs` parse test**

Add to `cli.rs`'s `mod tests`:

```rust
    #[test]
    fn init_parses_relay_key_and_no_verify() {
        let cli = Cli::parse_from([
            "pocket-codex", "init", "--relay", "lb7666.top:7666", "--key",
            "0123456789abcdef0123456789abcdef", "--no-verify",
        ]);
        let Command::Init(args) = cli.command else {
            panic!("expected init command");
        };
        assert_eq!(args.relay.as_deref(), Some("lb7666.top:7666"));
        assert_eq!(args.key.as_deref(), Some("0123456789abcdef0123456789abcdef"));
        assert!(args.no_verify);
    }
```

- [ ] **Step 7: Register module + dispatch arm in `mod.rs`**

Add `mod init;` (after `mod connect;`). Add the dispatch arm inside the `match`
(first arm, mirroring the enum order):

```rust
        Command::Init(args) => init::run(args).await,
```

- [ ] **Step 8: Build + run the new tests**

Run: `cargo test -p pocket-codex-cli init_parses normalize_relay validate_key --locked`
Expected: PASS (all three).

- [ ] **Step 9: Commit**

```bash
git add crates/pocket-codex-cli/src/cli.rs \
        crates/pocket-codex-cli/src/commands/init.rs \
        crates/pocket-codex-cli/src/commands/mod.rs
git commit -m "feat(cli): add interactive pocket-codex init command"
```

---

### Task 6: Documentation — README status + cli-verification

**Files:**
- Modify: `README.md`
- Modify: `docs/cli-verification.md`

- [ ] **Step 1: Add `init` to the README command surface**

Find the command list / status table in `README.md` and add a row documenting
`pocket-codex init` (interactive relay+key setup; precedence flag>config>env).
Match the table's existing column shape. Example row:

```markdown
| `pocket-codex init` | Persist the default relay `host:port` + `MSG_HEADER_KEY` to `config.toml` (0600). Later commands default to it; `--relay` still overrides. |
```

- [ ] **Step 2: Add an init section to `docs/cli-verification.md`**

Add a new subsection near the top of the relay workflow (before §4 app-server
flow) documenting:

```markdown
## 0. 一次性初始化 relay（推荐先做）

```bash
$PCX init
# 交互填入：relay host:port、32 字节 MSG_HEADER_KEY
# 或非交互：
$PCX init --relay lb7666.top:7666 --key <32B> [--no-verify]
```

写入 `~/.config/pocket-codex/config.toml`（unix 下 0600）。之后所有命令在不带
`--relay` 时默认走这份配置，且会自动应用其中的 key——无需再 `export
MSG_HEADER_KEY`。

**解析优先级**：`--relay` flag > config > `$PB_MAPPER_SERVER` env。key 同理
`config > $MSG_HEADER_KEY`。所以 init 后即便 shell 里残留旧的
`PB_MAPPER_SERVER`，也以 config 为准。

**连接超时**：发现/状态查询现在对 relay 连接有 5s 上限——指向不可达 relay 时
~5s 内报错，不再卡满内核 TCP 超时（~123s）。

`init` 默认存盘前会连一次 relay 校验（✓ 列出服务数 / ✗ 报错不存盘）；relay
临时不可达时可加 `--no-verify` 跳过。
```
```

- [ ] **Step 3: Commit**

```bash
git add README.md docs/cli-verification.md
git commit -m "docs: document pocket-codex init, relay precedence, connect timeout"
```

---

## Final Verification (run before declaring done)

- [ ] `cargo fmt -p pocket-codex-core -p pocket-codex-codex -p pocket-codex-pb -p pocket-codex-cli -p pocket_codex_bridge -- --check`
- [ ] `cargo +stable clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace --locked`
- [ ] Manual smoke: `pocket-codex init --relay lb7666.top:7666 --key <32B>` then
      `pocket-codex services list` (no `--relay`) returns instantly; with a dead
      relay configured, `services list` errors in ~5s, not ~123s.

