# Pocket-Codex Mobile UI — P1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A responsive, Material 3 Flutter app over the existing Rust engine: onboarding (relay+key, `pcx1:` base64 import/export, persisted), relay service discovery, API-service subscribe (local OpenAI-compatible endpoint), and settings — with the project logo wired as launcher icon / splash / onboarding hero.

**Architecture:** The Rust bridge crate (`pocket_codex_bridge`) owns a global tokio runtime, config persistence (core `Config` serde struct → `<support_dir>/config.toml`), relay discovery (`pb::keys`), and an in-process subscription registry (spawned tokio tasks + abort handles — no `state.toml`, no child processes). Pure logic lives in non-bridged `engine::*` modules (unit-tested without FRB); thin FRB wrappers + DTOs live in `crate::api`. Dart is UI only: it talks to Rust through a `BridgeApi` Dart interface (real impl wraps the FRB-generated bindings; a fake backs widget tests), with Riverpod for state and go_router for navigation.

**Tech Stack:** Rust (tokio, base64, serde/toml, flutter_rust_bridge 2.12.0) · Flutter 3.44 / Dart 3.12, Material 3, flutter_riverpod, go_router, path_provider, flutter_launcher_icons, flutter_native_splash.

**Spec:** `docs/superpowers/specs/2026-06-01-flutter-ui-design.md` (this plan = P1 only; P2 sessions get a separate plan).

---

## File Structure

**Rust bridge (`crates/pocket-codex-bridge/`):**
- `Cargo.toml` — add deps: `pocket-codex-core`, `pocket-codex-pb`, `tokio`, `once_cell`, `anyhow`, `base64`, `serde_json`, `toml`.
- `src/lib.rs` — add `mod engine;` (keeps `pub mod api; mod frb_generated;`).
- `src/engine/mod.rs` — `pub mod config; pub mod runtime; pub mod discovery;`.
- `src/engine/config.rs` — load/save core `Config` at `<support_dir>/config.toml` (0600 on unix); `pcx1:` import/export. **Pure, unit-tested.**
- `src/engine/runtime.rs` — global `Runtime` + `support_dir` + subscription registry (`HashMap<String, SubEntry>`); `api_subscribe`/`api_unsubscribe`/`subscriptions` logic.
- `src/engine/discovery.rs` — resolve relay host:port → `SocketAddr`, call `pb::keys`, parse via `ServiceId::parse_key`.
- `src/api/bridge.rs` — FRB-exposed functions + DTOs (`ServiceIdDto`, `SubStatusDto`, `ConfigView`). `import_config` returns the parsed relay as a `String`.
- `src/api/mod.rs` — add `pub mod bridge;`.

**Flutter (`apps/flutter/`):**
- `pubspec.yaml` — deps + `assets/logo/`.
- `assets/logo/{logo.png,poster.png}` — copied from repo-root `assets/logo/`.
- `lib/src/bridge_api.dart` — `BridgeApi` abstract interface.
- `lib/src/bridge_api_rust.dart` — real impl wrapping FRB bindings.
- `lib/src/providers.dart` — Riverpod providers (`bridgeApiProvider`, config/discovery/subscriptions).
- `lib/src/theme.dart` — light/dark `ColorScheme`s + `ThemeMode.system`.
- `lib/src/router.dart` — go_router routes.
- `lib/src/screens/{onboarding,services,api_service,settings}_screen.dart`.
- `lib/main.dart` — boot FRB, `initBridge(supportDir)`, `ProviderScope` + `MaterialApp.router`.
- `test/*` — widget/provider tests with a `FakeBridgeApi`.

---
## Task 1: Bridge crate dependencies

**Files:** Modify `crates/pocket-codex-bridge/Cargo.toml`

- [ ] **Step 1: Add the engine deps** under `[dependencies]` (after the `flutter_rust_bridge` line):

```toml
pocket-codex-core = { workspace = true }
pocket-codex-pb = { workspace = true }
tokio = { workspace = true }
anyhow = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
toml = { workspace = true }
base64 = { workspace = true }
once_cell = { workspace = true }
```

- [ ] **Step 2: Add `pocket-codex-pb` to the workspace dep table** if missing. Check `Cargo.toml` root `[workspace.dependencies]` — it already lists `pocket-codex-core`, `pocket-codex-codex`, `pocket-codex-pb`. Confirm with: `rg -n "pocket-codex-pb" Cargo.toml`. Expected: a `pocket-codex-pb = { path = "crates/pocket-codex-pb" }` line. If absent, add it.

- [ ] **Step 3: Build to verify deps resolve**

Run: `cargo build -p pocket_codex_bridge --locked`
Expected: compiles (bridge still only has `simple.rs`; new deps unused yet → may warn, that's fine for this task).

- [ ] **Step 4: Commit**

```bash
git add crates/pocket-codex-bridge/Cargo.toml Cargo.toml Cargo.lock
git commit -m "build(bridge): add core/pb/tokio/base64 deps for the mobile engine"
```

---

## Task 2: `engine::config` — pcx1 import/export + load/save (pure)

**Files:** Create `crates/pocket-codex-bridge/src/engine/mod.rs`, `crates/pocket-codex-bridge/src/engine/config.rs`; modify `crates/pocket-codex-bridge/src/lib.rs`.

- [ ] **Step 1: Register the module.** In `src/lib.rs`, add `mod engine;` so it reads:

```rust
pub mod api;
mod engine;
mod frb_generated;
```

Create `src/engine/mod.rs`:

```rust
//! Non-bridged engine: pure logic + the tokio runtime/registry. Kept
//! separate from `api/` so it is unit-testable without flutter_rust_bridge.
pub mod config;
pub mod discovery;
pub mod runtime;
```

- [ ] **Step 2: Write `src/engine/config.rs` with failing tests.** Create the file with the impl AND the test module below (write impl first so it compiles; TDD here = tests pin the behaviour):

```rust
//! Config persistence + `pcx1:` share-string codec. Pure (no FRB, no
//! runtime), so it unit-tests standalone.
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use pocket_codex_core::config::Config;
use serde::{Deserialize, Serialize};

const PCX1_PREFIX: &str = "pcx1:";

/// Relay address + shared key carried by a `pcx1:` share string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharePayload {
    /// Relay `host:port`.
    pub relay: String,
    /// 32-byte `MSG_HEADER_KEY`.
    pub key: String,
}

/// Encode relay+key into a `pcx1:` + base64url(JSON) share string.
pub fn encode_pcx1(relay: &str, key: &str) -> Result<String> {
    let json = serde_json::to_vec(&SharePayload {
        relay: relay.to_string(),
        key: key.to_string(),
    })?;
    Ok(format!("{PCX1_PREFIX}{}", URL_SAFE_NO_PAD.encode(json)))
}

/// Decode a `pcx1:` share string; validates the key is exactly 32 bytes.
pub fn decode_pcx1(text: &str) -> Result<SharePayload> {
    let body = text
        .trim()
        .strip_prefix(PCX1_PREFIX)
        .ok_or_else(|| anyhow!("not a pcx1 share string (missing `pcx1:` prefix)"))?;
    let bytes = URL_SAFE_NO_PAD
        .decode(body.trim())
        .context("invalid base64 in pcx1 string")?;
    let payload: SharePayload =
        serde_json::from_slice(&bytes).context("invalid JSON in pcx1 string")?;
    if payload.key.len() != 32 {
        bail!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", payload.key.len());
    }
    Ok(payload)
}
```

Append the load/save functions to the same file (below `decode_pcx1`):

```rust
/// Path to the persisted config TOML under the app-support dir.
pub fn config_path(support_dir: &Path) -> PathBuf {
    support_dir.join("config.toml")
}

/// Load the config from `<support_dir>/config.toml`. Missing file → default.
pub fn load_config(support_dir: &Path) -> Result<Config> {
    let path = config_path(support_dir);
    match std::fs::read_to_string(&path) {
        Ok(raw) => toml::from_str(&raw).context("parsing config.toml"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(e).context("reading config.toml"),
    }
}

/// Persist the config. On unix the file is created 0o600 (it holds the
/// relay MSG_HEADER_KEY); permissions are set before the bytes are written.
pub fn save_config(support_dir: &Path, config: &Config) -> Result<()> {
    std::fs::create_dir_all(support_dir).context("creating support dir")?;
    let path = config_path(support_dir);
    let raw = toml::to_string_pretty(config)?;
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        let mut f = std::fs::OpenOptions::new()
            .write(true).create(true).truncate(true).mode(0o600).open(&path)?;
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        f.write_all(raw.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, raw)?;
    Ok(())
}
```

- [ ] **Step 3: Append the test module** to `src/engine/config.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcx1_round_trips() {
        let key = "0123456789abcdef0123456789abcdef";
        let enc = encode_pcx1("lb7666.top:7666", key).expect("encode");
        assert!(enc.starts_with("pcx1:"));
        let got = decode_pcx1(&enc).expect("decode");
        assert_eq!(got.relay, "lb7666.top:7666");
        assert_eq!(got.key, key);
    }

    #[test]
    fn decode_rejects_bad_input() {
        assert!(decode_pcx1("lb7666.top:7666").is_err()); // no prefix
        assert!(decode_pcx1("pcx1:!!!notbase64").is_err());
        let short = encode_pcx1("r:1", "short").expect("encode");
        assert!(decode_pcx1(&short).is_err()); // key != 32 bytes
    }

    #[test]
    fn config_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("pcx-cfg-{}", std::process::id()));
        let mut cfg = Config::default();
        cfg.set_relay("lb7666.top:7666");
        cfg.set_relay_key("0123456789abcdef0123456789abcdef");
        save_config(&dir, &cfg).expect("save");
        let back = load_config(&dir).expect("load");
        assert_eq!(back.relay(), Some("lb7666.top:7666"));
        assert_eq!(back.relay_key(), Some("0123456789abcdef0123456789abcdef"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 4: Run tests** — `cargo test -p pocket_codex_bridge engine::config --locked` → 3 pass.

- [ ] **Step 5: Commit**

```bash
git add crates/pocket-codex-bridge/src/lib.rs crates/pocket-codex-bridge/src/engine/
git commit -m "feat(bridge): engine::config — pcx1 codec + 0600 config load/save"
```

## Task 3: `engine::discovery` — resolve relay + list services

**Files:** Create `crates/pocket-codex-bridge/src/engine/discovery.rs`.

- [ ] **Step 1: Write the module:**

```rust
//! Relay service discovery: resolve `host:port` and list `pcx:*` keys.
use std::net::SocketAddr;

use anyhow::{anyhow, Context, Result};
use pocket_codex_core::service::ServiceId;
use tokio::net::lookup_host;

/// One discovered Pocket-Codex service on the relay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredService {
    /// Device id segment.
    pub device: String,
    /// `app` or `api`.
    pub kind: String,
    /// Instance name segment.
    pub name: String,
    /// Full `pcx:<device>:<kind>:<name>` key.
    pub key: String,
}

/// Resolve a `host:port` relay string to one `SocketAddr`.
pub async fn resolve_relay(relay: &str) -> Result<SocketAddr> {
    lookup_host(relay)
        .await
        .with_context(|| format!("resolving relay `{relay}`"))?
        .next()
        .ok_or_else(|| anyhow!("relay `{relay}` resolved to no addresses"))
}

/// List Pocket-Codex services registered on the relay (bare keys filtered).
pub async fn discover(relay: &str) -> Result<Vec<DiscoveredService>> {
    let addr = resolve_relay(relay).await?;
    let keys = pocket_codex_pb::keys(addr).await.context("querying relay keys")?;
    Ok(keys
        .into_iter()
        .filter_map(|k| {
            ServiceId::parse_key(&k).map(|id| DiscoveredService {
                device: id.device,
                kind: id.kind.as_key_segment().to_string(),
                name: id.name,
                key: k,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn resolve_relay_rejects_garbage() {
        // No port → resolution fails fast, not a hang.
        assert!(resolve_relay("not-a-host-without-port").await.is_err());
    }
}
```

- [ ] **Step 2: Build + test**

Run: `cargo test -p pocket_codex_bridge engine::discovery --locked`
Expected: `resolve_relay_rejects_garbage` passes.

- [ ] **Step 3: Commit**

```bash
git add crates/pocket-codex-bridge/src/engine/discovery.rs
git commit -m "feat(bridge): engine::discovery — resolve relay + list pcx services"
```

---

## Task 4: `engine::runtime` — global runtime + subscription registry

**Files:** Create `crates/pocket-codex-bridge/src/engine/runtime.rs`.

- [ ] **Step 1: Write the module.** Holds a global tokio `Runtime`, the support dir, and a registry of spawned subscription tasks (key → abort handle + local addr). `pb::subscribe` runs forever, so we spawn it and keep its `JoinHandle` to abort on unsubscribe.

```rust
//! Process-global tokio runtime, support-dir, and the in-process
//! subscription registry. Mobile has no child processes / state.toml, so
//! subscriptions are spawned tasks tracked here and aborted on unsubscribe.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use once_cell::sync::OnceCell;
use pocket_codex_pb::{subscribe, SubscribeOptions};
use tokio::runtime::Runtime;
use tokio::task::JoinHandle;

static RUNTIME: OnceCell<Runtime> = OnceCell::new();
static SUPPORT_DIR: OnceCell<PathBuf> = OnceCell::new();
static REGISTRY: OnceCell<Mutex<HashMap<String, SubEntry>>> = OnceCell::new();

struct SubEntry {
    local_addr: String,
    handle: JoinHandle<()>,
}

/// Initialise the runtime + support dir. Idempotent; safe to call once at boot.
pub fn init(support_dir: PathBuf) -> Result<()> {
    RUNTIME
        .set(Runtime::new().map_err(|e| anyhow!("building tokio runtime: {e}"))?)
        .ok();
    SUPPORT_DIR.set(support_dir).ok();
    REGISTRY.set(Mutex::new(HashMap::new())).ok();
    Ok(())
}

/// The global runtime; panics if [`init`] was not called (a boot-order bug).
pub fn runtime() -> &'static Runtime {
    RUNTIME.get().expect("engine::runtime::init must run before runtime()")
}

/// The configured app-support directory.
pub fn support_dir() -> Result<PathBuf> {
    SUPPORT_DIR.get().cloned().ok_or_else(|| anyhow!("bridge not initialised"))
}

fn registry() -> &'static Mutex<HashMap<String, SubEntry>> {
    REGISTRY.get().expect("engine::runtime::init must run first")
}
```
Append the registry operations to the same file (below `registry()`):

```rust
/// Status of one active subscription, surfaced to the UI.
#[derive(Debug, Clone)]
pub struct SubStatus {
    /// Service key being subscribed to.
    pub key: String,
    /// Local `host:port` the subscriber listener is bound on.
    pub local_addr: String,
    /// Whether the spawned task is still running.
    pub alive: bool,
}

/// Start (or no-op if already live) an in-process subscription exposing
/// `key` on `127.0.0.1:<local_port>`. `pb::subscribe` runs forever, so we
/// spawn it and keep the handle for [`unsubscribe_service`] to abort.
pub fn subscribe_service(key: String, local_port: u16, relay: String) -> Result<SubStatus> {
    let local_addr = format!("127.0.0.1:{local_port}");
    let mut reg = registry().lock().expect("registry poisoned");
    if let Some(e) = reg.get(&key) {
        if !e.handle.is_finished() {
            return Ok(SubStatus { key, local_addr: e.local_addr.clone(), alive: true });
        }
    }
    let opts = SubscribeOptions { key: key.clone(), local_addr: local_addr.clone(), relay_addr: relay };
    let handle = runtime().spawn(async move { subscribe(opts).await });
    reg.insert(key.clone(), SubEntry { local_addr: local_addr.clone(), handle });
    Ok(SubStatus { key, local_addr, alive: true })
}

/// Abort and forget the subscription for `key`. No-op if absent.
pub fn unsubscribe_service(key: &str) {
    if let Some(e) = registry().lock().expect("registry poisoned").remove(key) {
        e.handle.abort();
    }
}

/// Snapshot of all tracked subscriptions.
pub fn list_subscriptions() -> Vec<SubStatus> {
    registry().lock().expect("registry poisoned").iter()
        .map(|(key, e)| SubStatus { key: key.clone(), local_addr: e.local_addr.clone(), alive: !e.handle.is_finished() })
        .collect()
}
```

- [ ] **Step 2: Build** — `cargo build -p pocket_codex_bridge --locked` (no live-relay unit test; covered by api layer + manual).

- [ ] **Step 3: Commit**

```bash
git add crates/pocket-codex-bridge/src/engine/runtime.rs
git commit -m "feat(bridge): engine::runtime — global runtime + subscription registry"
```

## Task 5: `api::bridge` — FRB functions + DTOs, regenerate Dart bindings

**Files:** Modify `crates/pocket-codex-bridge/src/api/mod.rs`; create `crates/pocket-codex-bridge/src/api/bridge.rs`.

FRB note: these are **non-`sync`** `pub fn`s. FRB runs them on a worker thread and hands Dart a `Future`; inside we call `runtime().block_on(..)` for async engine calls. Do NOT mark them `async` — FRB's async executor has no tokio reactor; our own runtime provides it.

- [ ] **Step 1: Register the module.** In `src/api/mod.rs`, add below `pub mod simple;`:

```rust
/// Real bridge surface: config, discovery, API-service subscribe.
pub mod bridge;
```

- [ ] **Step 2: Write `src/api/bridge.rs` (DTOs + functions):**

```rust
//! FRB-exposed bridge surface: config, discovery, API-service subscribe.
//! Thin glue over `crate::engine`; DTOs are plain (FRB-friendly) structs.
use std::path::PathBuf;

use anyhow::{anyhow, Result};

use crate::engine::{config, discovery, runtime};

/// View of persisted config for the UI; never exposes the raw key.
pub struct ConfigView {
    /// Configured relay `host:port`, if any.
    pub relay: Option<String>,
    /// Whether a 32-byte key is stored (value withheld).
    pub has_key: bool,
}

/// A discovered service, mirrored for Dart.
pub struct ServiceIdDto {
    /// Device id segment.
    pub device: String,
    /// `app` or `api`.
    pub kind: String,
    /// Instance name segment.
    pub name: String,
    /// Full relay key.
    pub key: String,
}

/// Status of one active subscription, mirrored for Dart.
pub struct SubStatusDto {
    /// Service key.
    pub key: String,
    /// Local `host:port`.
    pub local_addr: String,
    /// Task still running.
    pub alive: bool,
}

/// Initialise the engine with the platform app-support dir (from Dart's
/// path_provider). Must be called once after `RustLib.init()`.
pub fn init_bridge(support_dir: String) -> Result<()> {
    runtime::init(PathBuf::from(support_dir))
}

fn current_relay() -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    cfg.relay().map(str::to_string).ok_or_else(|| anyhow!("no relay configured"))
}

/// Apply the stored MSG_HEADER_KEY to this process (relay validates it).
fn apply_key() -> Result<()> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    if let Some(k) = cfg.relay_key() {
        pocket_codex_pb::set_msg_header_key(Some(k)).map_err(|e| anyhow!("{e}"))?;
    }
    Ok(())
}
```
Append the remaining FRB functions to `src/api/bridge.rs`:

```rust
/// Current config view (relay + whether a key is set).
pub fn get_config() -> Result<ConfigView> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    Ok(ConfigView { relay: cfg.relay().map(str::to_string), has_key: cfg.relay_key().is_some() })
}

/// Set the relay `host:port` and persist.
pub fn set_relay(relay: String) -> Result<()> {
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay(&relay);
    config::save_config(&dir, &cfg)
}

/// Set the 32-byte MSG_HEADER_KEY and persist (validates length).
pub fn set_key(key: String) -> Result<()> {
    if key.len() != 32 {
        return Err(anyhow!("MSG_HEADER_KEY must be exactly 32 bytes (got {})", key.len()));
    }
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay_key(&key);
    config::save_config(&dir, &cfg)
}

/// Import a `pcx1:` share string: decode, persist relay + key, return relay.
pub fn import_config(text: String) -> Result<String> {
    let payload = config::decode_pcx1(&text)?;
    let dir = runtime::support_dir()?;
    let mut cfg = config::load_config(&dir)?;
    cfg.set_relay(&payload.relay);
    cfg.set_relay_key(&payload.key);
    config::save_config(&dir, &cfg)?;
    Ok(payload.relay)
}

/// Export the current relay+key as a `pcx1:` share string.
pub fn export_config() -> Result<String> {
    let cfg = config::load_config(&runtime::support_dir()?)?;
    let relay = cfg.relay().ok_or_else(|| anyhow!("no relay configured"))?;
    let key = cfg.relay_key().ok_or_else(|| anyhow!("no key configured"))?;
    config::encode_pcx1(relay, key)
}

/// Discover services on the configured relay (applies the stored key first).
pub fn discover_services() -> Result<Vec<ServiceIdDto>> {
    apply_key()?;
    let relay = current_relay()?;
    let found = runtime::runtime().block_on(discovery::discover(&relay))?;
    Ok(found.into_iter().map(|s| ServiceIdDto {
        device: s.device, kind: s.kind, name: s.name, key: s.key,
    }).collect())
}

/// Subscribe to an API service, exposing it on `127.0.0.1:<local_port>`.
pub fn api_subscribe(service_key: String, local_port: u16) -> Result<SubStatusDto> {
    apply_key()?;
    let relay = current_relay()?;
    let s = runtime::subscribe_service(service_key, local_port, relay)?;
    Ok(SubStatusDto { key: s.key, local_addr: s.local_addr, alive: s.alive })
}

/// Stop an API-service subscription.
pub fn api_unsubscribe(service_key: String) {
    runtime::unsubscribe_service(&service_key);
}

/// List all active subscriptions.
pub fn subscriptions() -> Vec<SubStatusDto> {
    runtime::list_subscriptions().into_iter()
        .map(|s| SubStatusDto { key: s.key, local_addr: s.local_addr, alive: s.alive })
        .collect()
}
```

- [ ] **Step 3: Build the bridge** — `cargo build -p pocket_codex_bridge --locked`. Expected: compiles.

- [ ] **Step 4: Regenerate Dart bindings.** From `apps/flutter/`:

```bash
flutter_rust_bridge_codegen generate
```
Expected: regenerates `lib/src/rust/api/bridge.dart` + updates `frb_generated.*`. If the tool isn't installed: `cargo install flutter_rust_bridge_codegen@2.12.0` (must match the crate's `=2.12.0`).

- [ ] **Step 5: Commit**

```bash
git add crates/pocket-codex-bridge/src/api/ apps/flutter/lib/src/rust/
git commit -m "feat(bridge): FRB api — config/import/export/discover/api-subscribe"
```

---
## Task 6: Flutter deps, logo assets, launcher icon + splash

**Files:** Modify `apps/flutter/pubspec.yaml`; create `apps/flutter/assets/logo/{logo.png,poster.png}`.

- [ ] **Step 1: Copy the logo assets into the package** (Flutter can't bundle assets outside the package dir):

```bash
mkdir -p apps/flutter/assets/logo
cp assets/logo/logo.png assets/logo/poster.png apps/flutter/assets/logo/
```

- [ ] **Step 2: Add runtime deps + dev tools to `apps/flutter/pubspec.yaml`.** Under `dependencies:` (after the existing `flutter_rust_bridge: 2.12.0` / `pocket_codex_bridge` lines) add:

```yaml
  flutter_riverpod: ^2.6.1
  go_router: ^14.6.2
  path_provider: ^2.1.5
```

Under `dev_dependencies:` add:

```yaml
  flutter_launcher_icons: ^0.14.2
  flutter_native_splash: ^2.4.3
```

- [ ] **Step 3: Declare assets + icon/splash config.** Replace the `flutter:` block at the end with:

```yaml
flutter:
  uses-material-design: true
  assets:
    - assets/logo/

flutter_launcher_icons:
  image_path: "assets/logo/logo.png"
  android: true
  ios: true
  remove_alpha_ios: true

flutter_native_splash:
  image: assets/logo/logo.png
  color: "#FFFFFF"
  color_dark: "#101316"
  android_12:
    image: assets/logo/logo.png
    color: "#FFFFFF"
    color_dark: "#101316"
```

- [ ] **Step 4: Resolve deps + generate icon/splash**

```bash
cd apps/flutter
flutter pub get
dart run flutter_launcher_icons
dart run flutter_native_splash:create
```
Expected: `pub get` succeeds; icon/splash generators write platform assets with no error.

- [ ] **Step 5: Commit**

```bash
git add apps/flutter/pubspec.yaml apps/flutter/pubspec.lock apps/flutter/assets/ \
        apps/flutter/android apps/flutter/ios
git commit -m "feat(flutter): add deps, logo assets, launcher icon + splash"
```

---
## Task 7: Dart `BridgeApi` interface + real impl + fake

**Files:** Create `apps/flutter/lib/src/bridge_api.dart`, `apps/flutter/lib/src/bridge_api_rust.dart`, `apps/flutter/test/fake_bridge_api.dart`.

The UI depends only on `BridgeApi` (never on FRB directly), so widget tests run without the native library.

- [ ] **Step 1: Write the interface** `apps/flutter/lib/src/bridge_api.dart`:

```dart
/// Plain Dart mirrors of the bridge DTOs (decoupled from FRB types so the
/// UI and tests do not import generated bindings).
class ServiceEntry {
  const ServiceEntry({required this.device, required this.kind, required this.name, required this.key});
  final String device;
  final String kind; // 'app' | 'api'
  final String name;
  final String key;
}

class SubInfo {
  const SubInfo({required this.key, required this.localAddr, required this.alive});
  final String key;
  final String localAddr;
  final bool alive;
}

class ConfigInfo {
  const ConfigInfo({required this.relay, required this.hasKey});
  final String? relay;
  final bool hasKey;
}

/// The whole engine surface the UI is allowed to touch. One real impl wraps
/// FRB; a fake backs widget tests.
abstract interface class BridgeApi {
  Future<ConfigInfo> getConfig();
  Future<void> setRelay(String relay);
  Future<void> setKey(String key);
  Future<String> importConfig(String text); // returns relay; throws on bad input
  Future<String> exportConfig();
  Future<List<ServiceEntry>> discoverServices();
  Future<SubInfo> apiSubscribe(String serviceKey, int localPort);
  Future<void> apiUnsubscribe(String serviceKey);
  Future<List<SubInfo>> subscriptions();
}
```

- [ ] **Step 2: Write the real impl** `apps/flutter/lib/src/bridge_api_rust.dart` (maps FRB types ↔ the plain mirrors; import path matches `dart_output: lib/src/rust`):

```dart
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;

/// Real [BridgeApi] backed by the flutter_rust_bridge bindings.
class RustBridgeApi implements BridgeApi {
  const RustBridgeApi();

  @override
  Future<ConfigInfo> getConfig() async {
    final c = await frb.getConfig();
    return ConfigInfo(relay: c.relay, hasKey: c.hasKey);
  }

  @override
  Future<void> setRelay(String relay) => frb.setRelay(relay: relay);

  @override
  Future<void> setKey(String key) => frb.setKey(key: key);

  @override
  Future<String> importConfig(String text) => frb.importConfig(text: text);

  @override
  Future<String> exportConfig() => frb.exportConfig();

  @override
  Future<List<ServiceEntry>> discoverServices() async {
    final list = await frb.discoverServices();
    return list
        .map((s) => ServiceEntry(device: s.device, kind: s.kind, name: s.name, key: s.key))
        .toList();
  }

  @override
  Future<SubInfo> apiSubscribe(String serviceKey, int localPort) async {
    final s = await frb.apiSubscribe(serviceKey: serviceKey, localPort: localPort);
    return SubInfo(key: s.key, localAddr: s.localAddr, alive: s.alive);
  }

  @override
  Future<void> apiUnsubscribe(String serviceKey) => frb.apiUnsubscribe(serviceKey: serviceKey);

  @override
  Future<List<SubInfo>> subscriptions() async {
    final list = await frb.subscriptions();
    return list.map((s) => SubInfo(key: s.key, localAddr: s.localAddr, alive: s.alive)).toList();
  }
}
```
- [ ] **Step 3: Write the fake** `apps/flutter/test/fake_bridge_api.dart` (in-memory, no native lib):

```dart
import 'package:pocket_codex/src/bridge_api.dart';

/// In-memory [BridgeApi] for widget/provider tests. Seed [services] and
/// [config] per test; records subscribe/unsubscribe calls.
class FakeBridgeApi implements BridgeApi {
  FakeBridgeApi({ConfigInfo? config, List<ServiceEntry>? services})
      : _config = config ?? const ConfigInfo(relay: null, hasKey: false),
        _services = services ?? const [];

  ConfigInfo _config;
  List<ServiceEntry> _services;
  final Map<String, SubInfo> _subs = {};

  /// Make [discoverServices] throw, to exercise error states.
  Object? discoverError;

  @override
  Future<ConfigInfo> getConfig() async => _config;

  @override
  Future<void> setRelay(String relay) async =>
      _config = ConfigInfo(relay: relay, hasKey: _config.hasKey);

  @override
  Future<void> setKey(String key) async {
    if (key.length != 32) throw ArgumentError('key must be 32 bytes');
    _config = ConfigInfo(relay: _config.relay, hasKey: true);
  }

  @override
  Future<String> importConfig(String text) async {
    if (!text.startsWith('pcx1:')) throw const FormatException('not a pcx1 string');
    _config = const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true);
    return 'lb7666.top:7666';
  }

  @override
  Future<String> exportConfig() async => 'pcx1:ZmFrZQ';

  @override
  Future<List<ServiceEntry>> discoverServices() async {
    if (discoverError != null) throw discoverError!;
    return _services;
  }

  @override
  Future<SubInfo> apiSubscribe(String serviceKey, int localPort) async {
    final s = SubInfo(key: serviceKey, localAddr: '127.0.0.1:$localPort', alive: true);
    _subs[serviceKey] = s;
    return s;
  }

  @override
  Future<void> apiUnsubscribe(String serviceKey) async => _subs.remove(serviceKey);

  @override
  Future<List<SubInfo>> subscriptions() async => _subs.values.toList();
}
```

- [ ] **Step 4: Analyze** — `cd apps/flutter && flutter analyze lib/src/bridge_api.dart lib/src/bridge_api_rust.dart`. Expected: no issues (bridge.dart bindings exist from Task 5).

- [ ] **Step 5: Commit**

```bash
git add apps/flutter/lib/src/bridge_api.dart apps/flutter/lib/src/bridge_api_rust.dart \
        apps/flutter/test/fake_bridge_api.dart
git commit -m "feat(flutter): BridgeApi interface + Rust impl + in-memory fake"
```

---
## Task 8: Theme, providers, router, and `main` boot

**Files:** Create `apps/flutter/lib/src/theme.dart`, `apps/flutter/lib/src/providers.dart`, `apps/flutter/lib/src/router.dart`; rewrite `apps/flutter/lib/main.dart`.

- [ ] **Step 1: Theme** `apps/flutter/lib/src/theme.dart` (light + dark from one seed, M3):

```dart
import 'package:flutter/material.dart';

/// Brand seed colour for both schemes.
const _seed = Color(0xFF4C8DF6);

/// Light Material 3 theme.
ThemeData lightTheme() => ThemeData(
      colorScheme: ColorScheme.fromSeed(seedColor: _seed, brightness: Brightness.light),
      useMaterial3: true,
    );

/// Dark Material 3 theme.
ThemeData darkTheme() => ThemeData(
      colorScheme: ColorScheme.fromSeed(seedColor: _seed, brightness: Brightness.dark),
      useMaterial3: true,
    );
```

- [ ] **Step 2: Providers** `apps/flutter/lib/src/providers.dart`:

```dart
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/bridge_api_rust.dart';

/// The engine API. Overridden with a FakeBridgeApi in tests.
final bridgeApiProvider = Provider<BridgeApi>((ref) => const RustBridgeApi());

/// Current persisted config (relay + whether a key is set).
final configProvider = FutureProvider<ConfigInfo>((ref) async {
  return ref.watch(bridgeApiProvider).getConfig();
});

/// Discovered services on the configured relay. Re-run by invalidating.
final servicesProvider = FutureProvider<List<ServiceEntry>>((ref) async {
  return ref.watch(bridgeApiProvider).discoverServices();
});

/// Active local subscriptions.
final subscriptionsProvider = FutureProvider<List<SubInfo>>((ref) async {
  return ref.watch(bridgeApiProvider).subscriptions();
});
```

- [ ] **Step 3: Router** `apps/flutter/lib/src/router.dart` (push-stack; onboarding-gated):

```dart
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/screens/onboarding_screen.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';

/// Build the app router. [initialLocation] is `/onboarding` on first run
/// (no relay configured) and `/` otherwise.
GoRouter buildRouter({required String initialLocation}) => GoRouter(
      initialLocation: initialLocation,
      routes: [
        GoRoute(path: '/onboarding', builder: (c, s) => const OnboardingScreen()),
        GoRoute(path: '/', builder: (c, s) => const ServicesScreen()),
        GoRoute(path: '/settings', builder: (c, s) => const SettingsScreen()),
        GoRoute(
          path: '/api/:key',
          builder: (c, s) => ApiServiceScreen(serviceKey: s.pathParameters['key']!),
        ),
      ],
    );
```

- [ ] **Step 4: `main.dart`** — boot FRB, init bridge with the support dir, gate onboarding:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:path_provider/path_provider.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/router.dart';
import 'package:pocket_codex/src/theme.dart';
import 'package:pocket_codex/src/rust/api/bridge.dart' as frb;
import 'package:pocket_codex/src/rust/frb_generated.dart';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await RustLib.init();
  final dir = await getApplicationSupportDirectory();
  await frb.initBridge(supportDir: dir.path);
  final cfg = await frb.getConfig();
  final start = cfg.relay == null ? '/onboarding' : '/';
  runApp(ProviderScope(child: PocketCodexApp(initialLocation: start)));
}

/// Root app: Material 3 light/dark following the system, go_router nav.
class PocketCodexApp extends StatelessWidget {
  /// [initialLocation] decides onboarding vs services on cold start.
  const PocketCodexApp({super.key, required this.initialLocation});

  /// Route to open on launch.
  final String initialLocation;

  @override
  Widget build(BuildContext context) {
    return MaterialApp.router(
      title: 'Pocket-Codex',
      theme: lightTheme(),
      darkTheme: darkTheme(),
      themeMode: ThemeMode.system,
      routerConfig: buildRouter(initialLocation: initialLocation),
    );
  }
}
```

- [ ] **Step 5: Analyze** — `cd apps/flutter && flutter analyze lib/src/theme.dart lib/src/providers.dart lib/src/router.dart`. Screens don't exist yet → `main.dart`/`router.dart` will error on missing imports; that's expected until Task 9. Analyze only the three listed files here.

- [ ] **Step 6: Commit**

```bash
git add apps/flutter/lib/src/theme.dart apps/flutter/lib/src/providers.dart \
        apps/flutter/lib/src/router.dart apps/flutter/lib/main.dart
git commit -m "feat(flutter): theme (M3 light/dark), Riverpod providers, router, boot"
```

---
## Task 9: Screens — Onboarding, Services, ApiService, Settings

**Files:** Create `apps/flutter/lib/src/screens/{onboarding,services,api_service,settings}_screen.dart`.

All screens are `ConsumerWidget`/`ConsumerStatefulWidget` (Riverpod) using stock Material 3 widgets. Responsive rule lives in `ServicesScreen` (Task 9.2). Each screen ships with a widget test (Task 9.5) that mounts it via `ProviderScope(overrides: [bridgeApiProvider.overrideWithValue(FakeBridgeApi(...))])`.

- [ ] **Step 1: Onboarding** `lib/src/screens/onboarding_screen.dart` — logo hero + `pcx1:` import OR manual relay/key, then go to `/`:

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/providers.dart';

/// First-run setup: import a `pcx1:` string or type relay + key.
class OnboardingScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const OnboardingScreen({super.key});
  @override
  ConsumerState<OnboardingScreen> createState() => _OnboardingState();
}

class _OnboardingState extends ConsumerState<OnboardingScreen> {
  final _import = TextEditingController();
  final _relay = TextEditingController();
  final _key = TextEditingController();
  String? _error;
  bool _busy = false;

  Future<void> _run(Future<void> Function() op) async {
    setState(() { _busy = true; _error = null; });
    try {
      await op();
      if (mounted) context.go('/');
    } catch (e) {
      setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final api = ref.read(bridgeApiProvider);
    return Scaffold(
      body: SafeArea(
        child: Center(
          child: ConstrainedBox(
            constraints: const BoxConstraints(maxWidth: 480),
            child: ListView(
              padding: const EdgeInsets.all(24),
              shrinkWrap: true,
              children: [
                Image.asset('assets/logo/poster.png', height: 120, key: const Key('onboarding-logo')),
                const SizedBox(height: 24),
                Text('连接到 pb-mapper relay', style: Theme.of(context).textTheme.titleLarge),
                const SizedBox(height: 16),
                TextField(
                  controller: _import,
                  decoration: const InputDecoration(
                    labelText: 'pcx1: 分享串(一键导入)', border: OutlineInputBorder()),
                ),
                const SizedBox(height: 8),
                FilledButton(
                  key: const Key('import-btn'),
                  onPressed: _busy ? null : () => _run(() => api.importConfig(_import.text)),
                  child: const Text('导入'),
                ),
                const Divider(height: 32),
                TextField(controller: _relay, decoration: const InputDecoration(
                  labelText: 'relay host:port', border: OutlineInputBorder())),
                const SizedBox(height: 8),
                TextField(controller: _key, decoration: const InputDecoration(
                  labelText: 'MSG_HEADER_KEY (32 字节)', border: OutlineInputBorder())),
                const SizedBox(height: 8),
                FilledButton.tonal(
                  key: const Key('manual-btn'),
                  onPressed: _busy ? null : () => _run(() async {
                    await api.setRelay(_relay.text);
                    await api.setKey(_key.text);
                  }),
                  child: const Text('保存'),
                ),
                if (_error != null) Padding(
                  padding: const EdgeInsets.only(top: 16),
                  child: Text(_error!, key: const Key('onboarding-error'),
                    style: TextStyle(color: Theme.of(context).colorScheme.error)),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
```
- [ ] **Step 2: Services** `lib/src/screens/services_screen.dart` — home: relay header + device/status, grouped API / App-server list, ⚙ to settings. Responsive: <600 single-column list (tap → push `/api/:key`); ≥600 list stays left, detail shows inline on the right.

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:go_router/go_router.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/api_service_screen.dart';

/// Home screen: lists discovered services on the configured relay.
class ServicesScreen extends ConsumerWidget {
  /// Default constructor.
  const ServicesScreen({super.key});

  @override
  Widget build(BuildContext context, WidgetRef ref) {
    final servicesAsync = ref.watch(servicesProvider);
    final config = ref.watch(configProvider).valueOrNull;
    return Scaffold(
      appBar: AppBar(
        title: const Text('Pocket-Codex'),
        actions: [
          IconButton(
            key: const Key('settings-btn'),
            icon: const Icon(Icons.settings),
            onPressed: () => context.push('/settings'),
          ),
        ],
      ),
      body: servicesAsync.when(
        loading: () => const Center(child: CircularProgressIndicator()),
        error: (e, _) => _ErrorState(message: '$e', onRetry: () => ref.invalidate(servicesProvider)),
        data: (services) => RefreshIndicator(
          onRefresh: () async => ref.invalidate(servicesProvider),
          child: LayoutBuilder(
            builder: (context, c) {
              final list = _ServiceList(
                relay: config?.relay,
                services: services,
                onTapApi: (key) => c.maxWidth >= 600 ? null : context.push('/api/$key'),
              );
              if (c.maxWidth < 600) return list;
              // Wide: list left, detail right.
              final firstApi = services.where((s) => s.kind == 'api').firstOrNull;
              return Row(children: [
                SizedBox(width: 360, child: list),
                const VerticalDivider(width: 1),
                Expanded(
                  child: firstApi == null
                      ? const Center(child: Text('选择一个 API 服务'))
                      : ApiServiceScreen(serviceKey: firstApi.key, embedded: true),
                ),
              ]);
            },
          ),
        ),
      ),
    );
  }
}

class _ServiceList extends StatelessWidget {
  const _ServiceList({required this.relay, required this.services, required this.onTapApi});
  final String? relay;
  final List<ServiceEntry> services;
  final void Function(String key)? Function(String key) onTapApi;

  @override
  Widget build(BuildContext context) {
    final api = services.where((s) => s.kind == 'api').toList();
    final app = services.where((s) => s.kind == 'app').toList();
    return ListView(
      children: [
        ListTile(
          leading: const Icon(Icons.dns, color: Colors.green),
          title: Text(relay ?? '(未配置 relay)'),
          subtitle: const Text('relay'),
        ),
        if (api.isNotEmpty) const _SectionHeader('API 服务'),
        ...api.map((s) => ListTile(
              key: Key('svc-${s.key}'),
              leading: const Icon(Icons.api),
              title: Text(s.name),
              subtitle: Text(s.device),
              onTap: () => onTapApi(s.key),
            )),
        if (app.isNotEmpty) const _SectionHeader('App-server 服务'),
        ...app.map((s) => ListTile(
              key: Key('svc-${s.key}'),
              leading: const Icon(Icons.computer),
              title: Text(s.name),
              subtitle: Text('${s.device} · 会话功能见 P2'),
              enabled: false,
            )),
        if (services.isEmpty)
          const Padding(padding: EdgeInsets.all(32), child: Center(child: Text('该 relay 上没有发现服务'))),
      ],
    );
  }
}

class _SectionHeader extends StatelessWidget {
  const _SectionHeader(this.text);
  final String text;
  @override
  Widget build(BuildContext context) => Padding(
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 4),
        child: Text(text.toUpperCase(),
            style: Theme.of(context).textTheme.labelSmall?.copyWith(letterSpacing: .5)),
      );
}

class _ErrorState extends StatelessWidget {
  const _ErrorState({required this.message, required this.onRetry});
  final String message;
  final VoidCallback onRetry;
  @override
  Widget build(BuildContext context) => Center(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          Text(message, key: const Key('services-error'), textAlign: TextAlign.center),
          const SizedBox(height: 12),
          FilledButton(onPressed: onRetry, child: const Text('重试')),
        ]),
      );
}
```
- [ ] **Step 3: ApiService** `lib/src/screens/api_service_screen.dart` — pick local port → subscribe → show `base_url` (copyable) + provider snippet + no-auth warning + stop. `embedded` drops the Scaffold/AppBar so it nests in the wide-screen right pane.

```dart
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';

/// API-service detail: subscribe and expose a local OpenAI-compatible port.
class ApiServiceScreen extends ConsumerStatefulWidget {
  /// [serviceKey] is the full `pcx:<device>:api:<name>` key. [embedded] omits
  /// the Scaffold so it can nest in the wide-layout right pane.
  const ApiServiceScreen({super.key, required this.serviceKey, this.embedded = false});
  final String serviceKey;
  final bool embedded;
  @override
  ConsumerState<ApiServiceScreen> createState() => _ApiServiceState();
}

class _ApiServiceState extends ConsumerState<ApiServiceScreen> {
  final _port = TextEditingController(text: '18180');
  SubInfo? _sub;
  String? _error;
  bool _busy = false;

  Future<void> _subscribe() async {
    setState(() { _busy = true; _error = null; });
    try {
      final port = int.tryParse(_port.text) ?? (throw const FormatException('端口必须是数字'));
      _sub = await ref.read(bridgeApiProvider).apiSubscribe(widget.serviceKey, port);
      ref.invalidate(subscriptionsProvider);
    } catch (e) {
      _error = '$e';
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _stop() async {
    await ref.read(bridgeApiProvider).apiUnsubscribe(widget.serviceKey);
    ref.invalidate(subscriptionsProvider);
    setState(() => _sub = null);
  }

  @override
  Widget build(BuildContext context) {
    final body = ListView(
      padding: const EdgeInsets.all(16),
      children: [
        Text(widget.serviceKey, style: Theme.of(context).textTheme.bodySmall),
        const SizedBox(height: 16),
        if (_sub == null) ...[
          TextField(
            controller: _port,
            keyboardType: TextInputType.number,
            decoration: const InputDecoration(labelText: '本地端口', border: OutlineInputBorder()),
          ),
          const SizedBox(height: 12),
          FilledButton(
            key: const Key('subscribe-btn'),
            onPressed: _busy ? null : _subscribe,
            child: const Text('启动订阅'),
          ),
        ] else ...[
          Card(child: ListTile(
            title: SelectableText('http://${_sub!.localAddr}/v1', key: const Key('base-url')),
            subtitle: const Text('base_url'),
            trailing: IconButton(
              icon: const Icon(Icons.copy),
              onPressed: () => Clipboard.setData(ClipboardData(text: 'http://${_sub!.localAddr}/v1')),
            ),
          )),
          const SizedBox(height: 8),
          _ProviderSnippet(localAddr: _sub!.localAddr),
          const SizedBox(height: 8),
          Card(
            color: Theme.of(context).colorScheme.errorContainer,
            child: const Padding(
              padding: EdgeInsets.all(12),
              child: Text('⚠ 本地端点无鉴权,仅监听 127.0.0.1。仅在 App 前台存活。'),
            ),
          ),
          const SizedBox(height: 12),
          OutlinedButton(key: const Key('stop-btn'), onPressed: _stop, child: const Text('停止')),
        ],
        if (_error != null) Padding(
          padding: const EdgeInsets.only(top: 16),
          child: Text(_error!, key: const Key('api-error'),
            style: TextStyle(color: Theme.of(context).colorScheme.error)),
        ),
      ],
    );
    if (widget.embedded) return body;
    return Scaffold(appBar: AppBar(title: const Text('API 服务')), body: body);
  }
}

class _ProviderSnippet extends StatelessWidget {
  const _ProviderSnippet({required this.localAddr});
  final String localAddr;
  @override
  Widget build(BuildContext context) {
    final snippet = 'model_provider = "pocket-codex-api"\n\n'
        '[model_providers.pocket-codex-api]\n'
        'name = "Pocket-Codex API"\n'
        'base_url = "http://$localAddr/v1"\n'
        'wire_api = "responses"\n'
        'requires_openai_auth = false\n'
        'supports_websockets = true';
    return Card(child: Padding(
      padding: const EdgeInsets.all(12),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Row(mainAxisAlignment: MainAxisAlignment.spaceBetween, children: [
          const Text('~/.codex/config.toml'),
          IconButton(icon: const Icon(Icons.copy),
            onPressed: () => Clipboard.setData(ClipboardData(text: snippet))),
        ]),
        SelectableText(snippet, style: const TextStyle(fontFamily: 'monospace', fontSize: 12)),
      ]),
    ));
  }
}
```
- [ ] **Step 4: Settings** `lib/src/screens/settings_screen.dart` — change relay, re-import key (masked), per-subscription status, export `pcx1:`, about (version).

```dart
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:pocket_codex/src/providers.dart';

/// Settings: relay/key, subscription status, export, about.
class SettingsScreen extends ConsumerStatefulWidget {
  /// Default constructor.
  const SettingsScreen({super.key});
  @override
  ConsumerState<SettingsScreen> createState() => _SettingsState();
}

class _SettingsState extends ConsumerState<SettingsScreen> {
  String? _msg;

  @override
  Widget build(BuildContext context) {
    final api = ref.read(bridgeApiProvider);
    final config = ref.watch(configProvider).valueOrNull;
    final subs = ref.watch(subscriptionsProvider).valueOrNull ?? const [];
    return Scaffold(
      appBar: AppBar(title: const Text('设置')),
      body: ListView(
        children: [
          ListTile(
            title: const Text('relay'),
            subtitle: Text(config?.relay ?? '(未配置)'),
            trailing: const Icon(Icons.edit),
            onTap: () => _editRelay(api),
          ),
          ListTile(
            title: const Text('MSG_HEADER_KEY'),
            subtitle: Text(config?.hasKey == true ? '•••••••• (已设置)' : '(未设置)'),
            trailing: const Icon(Icons.edit),
            onTap: () => _editKey(api),
          ),
          const Divider(),
          const Padding(padding: EdgeInsets.fromLTRB(16, 8, 16, 4), child: Text('活跃订阅')),
          if (subs.isEmpty) const ListTile(dense: true, title: Text('(无)'))
          else ...subs.map((s) => ListTile(
                dense: true,
                leading: Icon(Icons.circle, size: 12, color: s.alive ? Colors.green : Colors.red),
                title: Text(s.key),
                subtitle: Text(s.localAddr),
              )),
          const Divider(),
          ListTile(
            key: const Key('export-btn'),
            title: const Text('导出 pcx1: 分享串'),
            trailing: const Icon(Icons.copy),
            onTap: () async {
              final s = await api.exportConfig();
              await Clipboard.setData(ClipboardData(text: s));
              setState(() => _msg = '已复制 pcx1: 分享串');
            },
          ),
          if (_msg != null) Padding(
            padding: const EdgeInsets.all(16),
            child: Text(_msg!, key: const Key('settings-msg')),
          ),
        ],
      ),
    );
  }

  Future<void> _editRelay(api) async {
    final ctrl = TextEditingController(text: ref.read(configProvider).valueOrNull?.relay ?? '');
    final ok = await _prompt(context, 'relay host:port', ctrl);
    if (ok == true) {
      await api.setRelay(ctrl.text);
      ref.invalidate(configProvider);
      ref.invalidate(servicesProvider);
    }
  }

  Future<void> _editKey(api) async {
    final ctrl = TextEditingController();
    final ok = await _prompt(context, 'MSG_HEADER_KEY (32 字节)', ctrl, obscure: true);
    if (ok == true) {
      try {
        await api.setKey(ctrl.text);
        ref.invalidate(configProvider);
      } catch (e) {
        setState(() => _msg = '$e');
      }
    }
  }

  Future<bool?> _prompt(BuildContext context, String label, TextEditingController ctrl,
      {bool obscure = false}) {
    return showDialog<bool>(
      context: context,
      builder: (c) => AlertDialog(
        content: TextField(
          controller: ctrl,
          obscureText: obscure,
          decoration: InputDecoration(labelText: label),
        ),
        actions: [
          TextButton(onPressed: () => Navigator.pop(c, false), child: const Text('取消')),
          FilledButton(onPressed: () => Navigator.pop(c, true), child: const Text('保存')),
        ],
      ),
    );
  }
}
```
- [ ] **Step 5: Widget tests** `apps/flutter/test/screens_test.dart` (mount each screen with `FakeBridgeApi`; no native lib):

```dart
import 'package:flutter/material.dart';
import 'package:flutter_riverpod/flutter_riverpod.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pocket_codex/src/bridge_api.dart';
import 'package:pocket_codex/src/providers.dart';
import 'package:pocket_codex/src/screens/services_screen.dart';
import 'package:pocket_codex/src/screens/settings_screen.dart';
import 'fake_bridge_api.dart';

Widget _host(Widget child, BridgeApi api) => ProviderScope(
      overrides: [bridgeApiProvider.overrideWithValue(api)],
      child: MaterialApp(home: child),
    );

void main() {
  testWidgets('Services groups api + app and shows relay', (t) async {
    final api = FakeBridgeApi(
      config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true),
      services: const [
        ServiceEntry(device: 'lb7666', kind: 'api', name: 'default', key: 'pcx:lb7666:api:default'),
        ServiceEntry(device: 'lb7666', kind: 'app', name: 'default', key: 'pcx:lb7666:app:default'),
      ],
    );
    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    expect(find.text('API 服务'), findsOneWidget);
    expect(find.text('App-server 服务'), findsOneWidget);
    expect(find.byKey(const Key('svc-pcx:lb7666:api:default')), findsOneWidget);
    expect(find.text('lb7666.top:7666'), findsOneWidget);
  });

  testWidgets('Services shows error state with retry', (t) async {
    final api = FakeBridgeApi(config: const ConfigInfo(relay: 'r:1', hasKey: true))
      ..discoverError = Exception('relay down');
    await t.pumpWidget(_host(const ServicesScreen(), api));
    await t.pumpAndSettle();
    expect(find.byKey(const Key('services-error')), findsOneWidget);
    expect(find.text('重试'), findsOneWidget);
  });

  testWidgets('Settings shows masked key and relay', (t) async {
    final api = FakeBridgeApi(config: const ConfigInfo(relay: 'lb7666.top:7666', hasKey: true));
    await t.pumpWidget(_host(const SettingsScreen(), api));
    await t.pumpAndSettle();
    expect(find.text('lb7666.top:7666'), findsOneWidget);
    expect(find.text('•••••••• (已设置)'), findsOneWidget);
    expect(find.byKey(const Key('export-btn')), findsOneWidget);
  });
}
```

- [ ] **Step 6: Run analyze + tests**

```bash
cd apps/flutter
flutter analyze
flutter test
```
Expected: analyze clean; all widget tests pass.

- [ ] **Step 7: Commit**

```bash
git add apps/flutter/lib/src/screens/ apps/flutter/test/screens_test.dart
git commit -m "feat(flutter): onboarding, services, api-service, settings screens + tests"
```

---

## Task 10: Final verification + responsive check + README

**Files:** Modify `README.md` (Status table — note the Flutter app now ships P1 UI).

- [ ] **Step 1: Full Rust gate** (from repo root):

```bash
cargo fmt -p pocket-codex-core -p pocket-codex-codex -p pocket-codex-pb -p pocket-codex-cli -p pocket_codex_bridge -- --check
cargo +stable clippy -p pocket_codex_bridge --all-targets -- -D warnings
cargo test -p pocket_codex_bridge --locked
```
Expected: fmt clean, clippy clean, bridge tests pass (config + discovery).

- [ ] **Step 2: Full Flutter gate**

```bash
cd apps/flutter
dart format --output=none --set-exit-if-changed lib test
flutter analyze
flutter test
```
Expected: all clean / pass.

- [ ] **Step 3: Responsive smoke (manual or `flutter run`).** Verify: phone width (<600) → single-column list, tap API service pushes detail; tablet/desktop width (≥600) → list left + API detail right; light/dark follow the OS toggle; launcher icon + splash show the logo.

- [ ] **Step 4: README Status row.** In `README.md`, update the Flutter front-end row to note P1 shipped: onboarding (relay+key / `pcx1:` import), discovery, API-service subscribe, settings; responsive Material 3; P2 = app-server sessions.

- [ ] **Step 5: Commit**

```bash
git add README.md
git commit -m "docs: README — Flutter app P1 UI (onboarding, discovery, API subscribe, settings)"
```

---

## Self-review notes (for the implementer)

- Every Rust `pub fn`/`pub struct` in `api/` and `engine/` has a doc comment (workspace `missing_docs = deny`); test code may use `.expect()` but **not `.unwrap()`** (workspace lints `clippy::unwrap_used` on `--all-targets`).
- `init_bridge` must run after `RustLib.init()` and before any other bridge call (boot order in `main.dart`).
- FRB Dart method names are lowerCamelCase with named args (`frb.setRelay(relay: ...)`); regenerate (`flutter_rust_bridge_codegen generate`) whenever `api/bridge.rs` changes.
- P1 is read-only on the app-server side: App-server services render in the list but their tile is disabled (sessions = P2).

