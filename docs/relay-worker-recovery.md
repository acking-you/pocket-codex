# Relay worker recovery and maintenance

Detached CLI network workers retain their SDK handles while the relay or DNS is
unavailable, including when startup takes longer than the interactive 30-second
readiness deadline. Their local service process is independent. A permanent relay
rejection still fails the worker. Account credential bootstrap retries transient
backend failures; the worker owns and cancels its refresh tasks on exit.

The pinned pb-mapper SDK refreshes relay DNS through the OS resolver, shares bounded
setup capacity across mappings in one process, uses network-change hints and
protocol deadlines, and preserves healthy data connections. It cannot bridge an
unavailable physical link or resume a data socket that has already reset.

Inspect one recorded worker:

```sh
pocket-codex pb diagnostics --role register --key pcx:device:api:default
```

Output includes PID liveness and, for a recent matching worker, its actual SDK
version, recovery state, failure/timing information and setup counts. Snapshot
files contain no credentials or application payload. PID/start-time validation and
a 30-second freshness bound prevent a stale snapshot from impersonating a running
worker. `runtime: null` / `unknown` means this worker cannot report fresh diagnostics;
the installed binary's version is never substituted for the running worker's SDK.
`pocket-codex status` also displays the running SDK version.

Startup reuse, status, diagnostics and stopping all verify the standalone worker's
executable, role, key, local endpoint and relay. A PID reused by another process,
a Linux thread ID or an exited worker is stale. Starting a mapping replaces its
stale record; stopping it removes only the record without signalling that PID.
A matching worker remains reusable while offline or without fresh diagnostics.

After installing an updated CLI, replace exactly one network worker:

```sh
pocket-codex pb restart --role register --key pcx:device:api:default
```

Both selectors are required. The command checks executable, worker subcommand,
key, endpoint and relay before sending a signal; it refuses a host supervisor or
another process at a reused PID. It resolves credentials before stopping the
worker and refuses a relay/account change. A restart replaces this mapping's
connections, while its local Codex/API process and other worker records remain
intact. There is no implicit all-workers operation. A worker embedded inside the
host/UI process must be upgraded with that owner; this command cannot restart it.

Protocol/source verification does not upgrade an already-running process. In
particular, a deployment that must preserve a Codex app-server mapping must leave
its worker and host untouched, report their old/unknown runtime SDK honestly,
and upgrade them only during a separately authorized maintenance window.

## Host and controller recovery

The account-mode CLI host retains its meta registration even when the initial
30-second readiness wait expires. The SDK continues retrying transient outages;
permanent relay rejection remains a failure. The Codex watchdog stops only the
process matching its own listener, even when another host last updated the shared
runtime record.

The App owns its credential renewal task by support directory, account and backend.
A changed issued expiry replaces the schedule; logout or self-host transport stops
it. Unchanged expiries outside the renewal margin are normal. Failed renewals retry
after one minute, and shortened deadlines are honored.

Controller reconnect checks cancellation after network awaits and on navigation or
backgrounding. Foreground return checks the connection immediately. History and
transport can be ready while optional metadata is still loading; sending waits for
settings restoration. Optional config and model waits each have a ten-second bound.
The Logs page includes `controller.recovery` stages, attempts, elapsed monotonic
milliseconds and failure details, and its existing Copy action exports them.

CLI diagnostics also include `events`: the last 32 state transitions across worker
restarts, with process identity, UTC epoch milliseconds and elapsed milliseconds
within that process. They are stored beside the worker log as `*.events.json` with
private permissions and retained after worker exit. Repeated heartbeat snapshots
do not add duplicate transitions. Event history does not assert current liveness;
only the fresh, identity-checked `runtime` snapshot does.
