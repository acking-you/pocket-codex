# pocket-codex-host-svc

A **host-side meta service**: a small axum HTTP server, run on the machine that
hosts a `codex` app-server (via the app's in-app hosting or the CLI
`pocket-codex serve`). It is published through the account broker as a **third
tunnel** — `pcx:<device>:meta:<name>` — alongside the existing
`app:<name>` (codex remote control) and `api:<name>` (Responses API proxy)
tunnels that one host already exposes.

It exists so a **remote** client (the phone app driving a desktop host) can do
what previously only worked when the Flutter app ran *on* the host:

* list the host's local `CODEX_HOME` sessions (including ones owned by another
  codex client, e.g. the desktop app) — requirement **#5**;
* read a session transcript read-only;
* **force-resume** a session: evict the live holder processes on the host, then
  `thread/resume` it into the colocated app-server;
* persist **per-thread config** (model / reasoning effort / permission mode /
  plan mode) in a host-side store so it survives restarts and is shared across
  devices — requirement **#2**.

## Store choice: atomic-write JSON, not sqlite

The store is a single JSON file on the host, guarded by an async mutex and
written atomically (temp + rename). This was chosen over an embedded SQL engine
deliberately: this crate is linked **in-process by the bridge**, which compiles
for **mobile** too, and a bundled C-sqlite would add a native dependency (and
build/size risk) to the mobile artifact for a feature that only ever runs on a
desktop host. The data is a small, low-write per-thread config map for which a
mutex-serialized map is the right tool — it still survives restarts and is the
single source of truth shared across every device that reaches the host. (If a
richer store is ever needed, swapping the `store` module is isolated.)

## Why a separate service (not the app tunnel)

Listing rollouts and evicting holder processes are **host filesystem/process**
operations that the codex app-server protocol does not expose. The meta service
runs on the host with that access; the app tunnel only speaks the app-server
JSON-RPC. The force-resume endpoint is self-contained (evict **and** resume over
loopback) so there is no evict→resume race split across the relay.

## Endpoints

| Method + path                     | Purpose                                            |
| --------------------------------- | -------------------------------------------------- |
| `GET  /healthz`                   | liveness probe (mirrors codex `/readyz`)           |
| `GET  /sessions`                  | list local sessions, newest first                  |
| `GET  /sessions/{id}/liveness`    | one session's liveness + would-be takeover targets |
| `GET  /sessions/{id}/transcript`  | read-only transcript parsed from the rollout       |
| `POST /sessions/{id}/resume`      | evict holders + `thread/resume` into local codex   |
| `GET  /threads/{id}/config`       | persisted per-thread config (empty if unset)       |
| `PUT  /threads/{id}/config`       | upsert per-thread config                           |

## Auth / trust model

Mirrors `pocket-codex-api-proxy`: the **broker** authenticates the subscriber by
account JWT before bridging the tunnel, and the backend scopes every key to the
JWT's user namespace (`pcxu:<user>:…`), so only the account owner can reach their
own meta tunnel. The loopback listener is host-trusted (same posture as the
api-proxy, which forwards the host's ChatGPT token on the same basis). A
per-host bearer secret is a possible future hardening for the local-loopback leg.

## Reuse (no bridge dependency)

* sessions / liveness / transcript → `pocket_codex_codex::{rollout, takeover,
  liveness}` directly (the bridge's `engine/sessions.rs` is the same thin wrapper
  and now delegates here).
* resume → `pocket_codex_codex::client::AppClient::connect("ws://<app_local>")`,
  `initialize` (`capabilities.experimentalApi = true`), then
  `thread/resume {threadId}` — exactly the handshake the bridge uses.
* protected pids → this process + whatever serves the colocated app-server's
  listen port (`pocket_codex_core::process::find_codex_app_server`), so a
  takeover can never kill the server it resumes into.

## Cloud dependency

`account-proto::BrokerHello.kind` is a serde-encoded `ServiceKind`, so the
**deployed backend must be rebuilt with the `Meta` variant** before a remote
`meta` tunnel is accepted (`app`/`api` are unaffected; the local-loopback path
works regardless). That redeploy is part of the Phase 2 rollout.

Forward/backward compatibility: `ServiceKind` has a `#[serde(other)] Unknown`
fallback, so a client never fails to parse a wire payload that carries a kind a
newer peer introduced. The backend additionally omits `meta` (and any unknown
kind) from `GET /v1/services`, so an **already-shipped** pre-Phase-2 app — which
can't deserialize `meta` — is not broken by the rollout; the meta key is derived
client-side from the app key and never needs to appear in that listing.

## Known limitations

* **Windows force-resume blast radius.** A force takeover on Windows can't name
  the exact rollout holder, so it evicts from the whole `codex app-server`
  process pool (minus the protected pids). A host running several unrelated codex
  app-servers may therefore have one evicted by a remote force-resume. This is
  inherited from the local takeover (`pocket_codex_codex::takeover`); the
  killed/survived processes are reported back in the resume outcome.
* **Cross-process config store.** When two host processes share one `CODEX_HOME`
  (e.g. an app host and a CLI host at once), `put` re-reads + merges the on-disk
  map under its lock so writes aren't clobbered, but a true sub-write-window race
  is not fully prevented without an OS file lock — an accepted edge for this
  convenience state. No `fsync`, so an unclean shutdown may lose the last write
  (the atomic rename still guarantees the file is never partial).
* **Idle meta subscription.** A remote viewer keeps its `meta:` broker
  subscription registered (reused across re-opens, not a growing leak) until the
  app exits; it self-heals if torn down. A `meta` disconnect-on-teardown is a
  possible future tidy-up.
* **Supervisor liveness.** The in-process api-proxy / meta supervisors restart
  their inner server forever, so the only way a supervisor *task* ends is a
  panic; the host-reuse path re-registers tunnels but does not re-spawn a
  panicked supervisor (treated as a should-not-happen).
