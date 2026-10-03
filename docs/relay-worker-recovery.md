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
