# App-server protocol sync: 2026-10-04

The reference is upstream `openai/codex` main, pulled on 2026-10-04 with
`git -C ~/rust_pro/codex pull --ff-only origin main`:
`afb436df8b70bb5bc57b86d9a3e829968988cd21`. Local `HEAD` and `origin/main`
matched after the pull. The fork merges that exact commit at
`beaefbdf0ed3788762f7a30f3a83ad982d717fd4`; its `app-server-protocol` and
`protocol` directories have no differences from the pulled upstream revision.

This corrects 0.2.6, which used the stale local revision `17a9df60e` despite
the request to pull latest. The new reference includes 225 further commits.
It does not assert that an external Codex executable installed by a user was
built from this commit.

Pocket-Codex still launches the installed external executable. The submodule is
the wire-schema reference, not a bundled runtime or a linked protocol library.

```mermaid
flowchart LR
    U["upstream main: afb436df8b"] -->|"git pull --ff-only"| L["~/rust_pro/codex"]
    L -->|"merge: beaefbdf0"| S
    S["deps/codex: reference schemas"] -. "audit wire compatibility" .-> B["Pocket bridge: local JSON envelopes"]
    F[Flutter] <--> B
    B <-->|"initialize, initialized, requests and events"| C["Installed external codex app-server"]
    C -->|"turn/completed: failed or interrupted with error"| E["Visible error reason"]
```

## Consumed surface

| Upstream change since `c08819510` | Pocket-Codex behavior |
| --- | --- |
| `Turn.error` can accompany an interrupted turn; `tooManyDenials` and `flexUnavailable` error information added | Show explicit terminal errors on failed or interrupted live turns. Ordinary interruption without an error stays quiet. Preserve the raw error JSON. |
| `thread/items/list.cursor` additionally accepts an item anchor object | Existing opaque string cursors remain supported and are passed back unchanged. No extra reads or translation are needed; Pocket does not expose item-anchor navigation. |
| MCP discovery adds optional `serverName` and `threadId` | Existing full-inventory calls remain valid. Unknown response fields are retained in wire values. |
| Enterprise MCP OAuth adds `loginId` and completion metadata | No enterprise login flow is added in this release; Pocket's existing account login behavior is unchanged. |
| Plugin summary drops obsolete extension metadata; account plan/error variants expand | Pocket does not deserialize these into closed upstream enums. No runtime dependency or compatibility shim is added. |
| Tool-call execution metadata changes within upstream model messages | The app-server item/event surface remains the integration boundary. Pocket does not interpret model-provider message internals. |
| Command rollouts now persist only `aggregated_output`; app-server still sends `aggregatedOutput` | Both existing readers already accept these fields. Regression coverage checks current rollouts without legacy stdout/stderr/formatted output, plus live and restored app-server items. Older rollout fallback fields remain readable. |
| Persisted paginated command output is capped at 64 KiB with a middle truncation marker | Keep the server's text and marker visible. Pocket does not claim the stored output is complete or reread files to reconstruct omitted bytes. |
| Unknown `CodexErrorInfo` string/object variants are accepted upstream | Local JSON envelopes retain them; the UI shows the supplied error message without requiring a known error enum. Tests cover both future wire shapes. |
| Resume reuses unchanged stored snapshots after validating writer ownership | This is internal to the external server. Existing `excludeTurns: true` resume and bounded history requests remain valid; no extra controller request or snapshot copy is added. |
| Rollouts can contain `additional_tools` model metadata | Read-only history ignores it as metadata while retaining command output; it is not a user or assistant transcript row. |
| Optional goal mutation `origin`, attachment owner lookup, prediction and Bedrock advisory methods are added; `namespaceTools` capability is removed | Pocket does not call these new methods, mutate goals, or depend on the removed capability. Existing consumed requests require no new fields. |

Initialization still waits for the initialize response before sending
`initialized`. Bounded history windows, model/service-tier discovery, async
questions, approvals and image references retain their existing contracts.
The upstream WebSocket fork revisions and Rust 1.95 compiler floor are unchanged.

## Local host identity

```text
systemd pocket-codex-host
  +-- Pocket host supervisor
  +-- npm / Node launcher
  |     +-- native codex app-server PID  <-- record this process
  |           +-- runtime worker TIDs   <-- never record these as the host PID
  +-- pb register worker

worker identity on Linux = boot ID + /proc/PID/stat start ticks
wall-clock synchronization changes neither part of that identity
```

An executable replaced by npm can remain alive as `codex (deleted)`. Status
recognizes it until a deliberate restart loads the replacement. Listener
matching uses an exact `--listen` argument, not a URL substring.

## Verification

Run the full first-party gates in `AGENTS.md`, including Flutter lifecycle
regressions for interrupted errors and ordinary interruptions, current
aggregated-output rollout/live/history shapes and future error variants.
Audit the resolved bridge dependency graph on all targets/features: it must contain no
Codex runtime/protocol crate. The Responses proxy's HTTP and WebSocket transport
tests exercise host authentication, streaming and error forwarding. Build the
desktop application and require the native macOS host CI result.

After local installation, verify the loaded native executable and SDK version,
app-server initialization, model/history requests and a relay round trip. A
submodule update alone does not upgrade an already-running external process.
