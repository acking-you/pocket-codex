# ACP agents

Pocket-Codex can host any agent that speaks the
[Agent Client Protocol](https://agentclientprotocol.com) (ACP) v1 over stdio.
An agent is configured as a program plus an argument list; it is never run
through a shell. OpenCode (`opencode acp`) is the first preset. Codex
app-server stays the native, first-class engine, and the existing OpenCode
HTTP gateway (`opencode:<name>`) is unchanged.

Decision record: [ADR-0003](adr/0003-generic-acp-agents.md).

## Architecture

```
             host (desktop App)                                     controller (any device)
┌───────────────────────────────────────────────────────┐   ┌────────────────────────────────────┐
│ agent process (program + argv, own process group)     │   │ Flutter session UI (shared)        │
│        ▲ stdio NDJSON JSON-RPC 2.0                     │   │   │ BridgeApi app_* / meta_*       │
│ AgentHost (host-svc acp/): the one ACP client          │   │   ▼                                │
│   lifecycle owner, generations, event log, folding     │   │ bridge SessionEngine               │
│ /acp/v1 gateway (loopback HTTP + SSE)  ◄── pb ─────────┼───┼─►  ├─ CodexEngine    (app:)        │
│   key: …:acp:<name>                                    │   │    ├─ OpenCodeHttpEngine (opencode:)│
│ meta service (fs / uploads / file links)  ◄── pb ──────┼───┼─►  └─ AcpEngine      (acp:)        │
│   key: …:meta:<name>                                   │   │                                    │
└───────────────────────────────────────────────────────┘   └────────────────────────────────────┘
```

- **The host owns the ACP connection.** ACP is a single-client stdio
  protocol, so exactly one `AgentHost` speaks to the agent. Controllers never
  see ACP frames; they use the versioned `/acp/v1` HTTP + SSE gateway, which
  survives controller reconnects and lets several controllers watch one
  agent.
- **Protocol identity comes from the service key kind** (`app`, `opencode`,
  `acp`), never from the provider's display name. The bridge picks the engine
  with `engine(&key)` and every `app_*` entry point dispatches through the
  `SessionEngine` trait; unsupported operations return a stable
  "unsupported" error instead of being sent to the wrong protocol.
- **Capabilities are negotiated and conservative.** Until the agent has
  initialized, the UI receives `Capabilities::acp_unnegotiated()` (nothing
  optional enabled). Flutter gates every control on capabilities, including
  voice/dictation warm-up, shortcuts, steer, image attach, git diff, compact,
  rename and the model chip.

### Gateway vs ACP

| | OpenCode HTTP gateway (`opencode:`) | ACP host (`acp:`) |
| --- | --- | --- |
| Agent process | user's background service, attached, never owned | spawned and owned by the host |
| Wire to agent | OpenCode HTTP/SSE, allowlisted routes, injected Basic auth | ACP v1 stdio JSON-RPC |
| Translation | bridge `OpenCodeHttpEngine` | host folds ACP updates; bridge `AcpEngine` maps folded items |
| Stopping hosting | leaves OpenCode running | terminates the agent's process group |

## Host lifecycle

- `AgentHost` has a single lifecycle lock. `start`, `stop`, `restart` and
  crash recovery all go through it, and each launch starts a new
  **generation**. Events, snapshots, permission handles and turn outcomes are
  tagged with their generation; anything from an older generation is ignored.
- The agent runs in its own process group (`process_group(0)`). Shutdown
  closes stdin, waits briefly for output to drain, then sends `SIGTERM` to
  the group, waits 300 ms and sends `SIGKILL` *before* reaping the child, so
  a recycled PID is never signalled. Grandchildren are killed with the group.
- An exit is detected when the agent's stdout closes **or** when the leader
  process exits, whichever comes first. The leader is polled with
  `waitid(WEXITED | WNOHANG | WNOWAIT)`, which observes the exit without
  reaping it, so a descendant that inherited stdout cannot hide a dead agent
  and the group is still signalled before the PID is released.
- An unexpected exit is handled by one supervisor task per host. Each
  generation's watcher only reports "closed"; the supervisor takes the
  lifecycle lock, tears the generation down (unless a stop or restart already
  did) and relaunches after 2 s. Relaunches, including failed ones, are
  limited to 3 per 10 minutes; a stop or manual restart during the delay
  wins. Every turn running when the agent exits completes once with
  `agentExited`.
- Stopping a host is final and bounded: a launch still in its handshake is
  cut short (it does not wait out the 20 s handshake timeout), the process
  group is cleaned up as above, and pending session-record writes get 2 s.
- **Quitting the app.** One ownership table holds every owned agent until
  its cleanup has finished: an agent enters it before it is launched and
  leaves it only after it was stopped — also while an ordinary "stop
  hosting" is still retiring it. Quit closes the table's barrier (no start
  begins afterwards, a start in flight can no longer enter the host table)
  and stops every agent still in it — starting, hosted or retiring — all at
  once. Stopping an agent joins a stop already in progress, so quit (and a
  second, overlapping quit) returns only once every owned process was
  cleaned up. Agent processes are stopped first; relay keys of the hosts the
  quit drained are withdrawn afterwards within 2 s, so a slow relay never
  delays process cleanup. The desktop tray waits for this (capped at 20 s)
  before destroying the window; closing the window to the tray keeps hosting
  as before. This is exercised against the production table with agents
  that are created but never launched (a quit overlapping a paused
  retirement, two overlapping quits, a start racing the barrier) and by a
  mock-agent test of a stop during the handshake — not by quitting a real
  app.
- **Re-publishing** (`reregister`) waits for the relay without holding the
  host table. Its result is installed only into the same host incarnation,
  in the same transport context, and only if that service's registration was
  not changed meanwhile (a deregistration or another publication); a late
  result is withdrawn instead and never replaces a newer host's registration
  under the same name.
- stderr is drained and discarded (it may contain secrets).
- Tool lookup uses a child-only PATH (`pocket_codex_codex::external_tool_path()`,
  the macOS login-shell PATH); the controller's environment is not changed.
  An explicit program path is never replaced; a bare name is searched on
  PATH and then in the preset's fallback directories (OpenCode:
  `~/.opencode/bin`).
- **Windows is gated.** `acp_hosting_supported()` returns false and the
  dialog explains why; a correct job-object implementation is future work.

## Protocol surface

The host is an ACP **client**. It sends `initialize` with `protocolVersion: 1`
and advertises no file-system, terminal or other optional client
capabilities, so agent requests for `fs/*` or `terminal/*` are answered with
`-32601` (method not found). Any other protocol version is refused with a
clear "unsupported version" phase.

JSON-RPC envelopes are strict: request ids may be `null`, int64 or strings and
are echoed verbatim; outbound ids are numeric in their own namespace. Frames
are limited to 8 MiB; the writer queue is bounded (32 MiB / 512 frames) and at
most 64 outbound requests may be pending. A frame sent with a deadline counts
as sent only once the writer has written and flushed all of it; one deadline
covers waiting for queue room and that write. A frame whose deadline passes
before it was queued was never sent, and the connection stays open; one whose
deadline passes after it was queued may be half-written, so the connection is
closed at once (nothing more is written) and the generation ends. Budgets:

- a prompt has 30 s from its admission — for the dispatch lane, queue room
  and the complete write. Only a prompt the agent received in full then waits
  without limit for its answer. A prompt that was never queued fails with a
  message and touches nothing else.
- a cancel or a permission answer has 10 s, covering the dispatch lane (for a
  cancel) and every reply it writes, including the `cancelled` answers to the
  turn's withdrawn permission requests. Such an exchange has already consumed
  what it answers, so when it cannot complete the generation is closed rather
  than leaving the agent waiting on a live connection: the supervisor ends
  its turns (`agentExited`) and restarts within the restart budget. The first
  answer to a permission still wins; a retry then reports that the request or
  turn is gone instead of a silent success.

Closing the connection releases every waiter at once, and a closed
connection is not "ready" even before its generation was torn down.

Supported optional features, each used only when negotiated:

| Feature | Negotiation | Use |
| --- | --- | --- |
| `session/load` | `agentCapabilities.loadSession: true` | reopen with replayed history |
| `session/resume` | `sessionCapabilities.resume` is an object | reopen without replay (retained history only) |
| `session/list` | `sessionCapabilities.list` is an object | agent-owned session list, cursor-paged |
| `session/close` | `sessionCapabilities.close` is an object | release idle sessions evicted from the host |
| config options | `configOptions` in session responses | select-type options as chips (`session/set_config_option`) |
| modes | `modes` in session responses | mode chip (`session/set_mode`) |
| images | `promptCapabilities.image: true` | image attachments |

Without `session/list` the host lists sessions it created itself
(`sessionList: "host"`); without load or resume those sessions cannot be
reopened after an agent restart.

`auth_required` (`-32000`) is surfaced as an `authRequired` host state with
the agent's auth methods, whether it comes from `session/new`, a reopen or
`session/prompt`; a prompt rejected this way ends its turn once as *failed*
with a sign-in message. Pocket-Codex does not perform agent sign-in. Sign in
with the agent's own CLI on the host and restart hosting.

Queued frames retain their delivery deadline in a bounded table owned by the
peer, with one watchdog for the connection. Automatic permission replies have
the same control budget even when the writer is blocked by an earlier prompt.
Dropping a caller after consuming a cancel or permission closes that peer;
it cannot leave a consumed request unresolved on a live generation.

### Turns and permissions

- `session/prompt` is acknowledged immediately with a host-assigned turn id;
  the agent's final response is awaited in the background and ends the turn
  exactly once. `end_turn` maps to *completed*, `cancelled` to
  *interrupted*; refusals, token limits, unknown stop reasons and errors map
  to *failed* with a message.
- The bridge sends every mutation (`sessions/new`, `open`, `prompt`,
  `cancel`, `config`, `mode`) with the host incarnation (`hostId`) and
  generation its view shows. A replacement host behind the same relay key
  restarts its generations and may hand out the same session ids, so a
  request naming another incarnation or generation is refused with
  `409 stale_host` (a cancel answers `{"cancelled": false}`) before anything
  reaches the agent. The fields are optional on the wire and a request
  without them is not checked: this keeps a stale *bridge* view from
  mutating a newer host; it is not an authentication or authorization
  mechanism for arbitrary `/acp/v1` callers. The bridge applies an answer
  only to the connection that asked, and only while it still shows that host
  and generation.
- `session/cancel` names only a session, so every prompt dispatch and every
  cancel of a host is written under one dispatch lane, and each checks under
  it which turn is running. A cancel for turn A therefore never reaches a
  turn B admitted after A ended: either it is queued before B's prompt (the
  agent sees it while nothing runs) or it sees that A is gone and is not
  sent. Cancel names the turn it means; a stale cancel for an older turn
  returns `{"cancelled": false}` and touches nothing. A cancel that wins the
  race before the prompt is written ends the turn as *interrupted* without
  ever sending it. Only that turn's pending permission requests are answered
  with `cancelled`. Updates arriving after the cancel are still folded until
  the prompt response arrives.
- Permission requests get opaque host handles. The controller answers with
  the exact agent `optionId`; the first answer wins and later answers get
  `alreadyResolved`. Options are shown in the agent's order, and
  `allow_always` / `reject_always` ask for confirmation.

### History and limits

The host folds `session/update` notifications into a bounded transcript
(8 MiB, 4000 items, 500 turns). Every retained byte counts against the
budget: text, ids, titles, tool `rawInput`, locations and images, plus a
fixed per-item overhead. Per item, text is clipped at 512 KiB (the text then
ends with a visible marker and later text for that item is dropped),
`rawInput` above 64 KiB is replaced by a size note, and at most 8 images /
2 MiB are kept. Anything else dropped — images, diffs, tool input, locations,
content entries, plan steps — is named on the item, and the UI shows a
notice next to it ("Not kept because it was too large: images"). Dropping an
image never stops the text that follows it. Tool and plan updates replace
the collections they carry; the notice is state of its item, so each such
update re-states it, and when the replacement fits the notice is re-sent
empty and the session screen removes it — live, without a reconnect or a
history read. Notices and gaps have ids of their own (`#omitted:<item>`,
`#gap:<turn>`, …) that no agent-derived id can take, so a tool the agent
names `x:omitted` never collides with the notice of tool `x`.

Tool call and message ids are the agent's and are compared exactly. An id
longer than 256 bytes is kept as a 192-byte prefix plus its SHA-256, a form no
verbatim id can take, so distinct ids never merge and the chunks of one long
message id stay one message.

At most 32 sessions hold a transcript at a time. A running or reopening
session's transcript is never evicted for another; when all 32 belong to
running or reopening sessions, a new session, a reopen or a prompt of a
session without a transcript is refused (`429 capacity`, "too many sessions
are busy") instead of exceeding the pool. An idle transcript is evicted
least-recently-used, and one recreated later is marked incomplete.

The bounds hold inside a single turn: whole older turns are evicted first,
then older items of the oversized turn (never its prompt or newest item).
Item ids are per-turn ordinals that are never reused, so a later item can
never take an evicted item's id. Whatever was dropped is reported: history
reads start with a `historyGap` item when whole turns were dropped, and a
turn with evicted items carries an in-turn gap. The UI shows a notice instead
of pretending the history is complete. History is **retained only**: with
`session/resume`, or after the host evicts a transcript, earlier turns are not
recovered, and an evicted transcript that is recreated starts out marked
incomplete.

The event log keeps 4096 events / 16 MiB. A controller resumes the SSE stream
after its watermark; if that position was dropped, the generation changed or
a different host incarnation (`hostId`) answers, the gateway sends `reset`.
The bridge then reads the snapshot and recovers in a fixed order: turns that
ended meanwhile are completed first (with their final text and recorded
outcome), then turns it had not seen begin are announced, then each running
turn's folded history — which carries the log position it reflects — is
installed with the host's item ids, and only later events are applied. Every
item event names its turn, and the session screen ignores a terminal event
for a turn other than the one it shows running, so a late completion never
ends or regroups a newer turn.

A turn whose prompt carried images is begun in the controller's fold only
from the host's own fold: `turn_started` carries just the image count (image
data is never streamed in events), and a prompt folded without its images
would weigh less than the host's and evict — and number — later items
differently. The bridge reads that turn's fold right after announcing it.

Every history read — the newest window, an older page, one turn, a recovery
read — is accepted only from the host incarnation and generation the view
shows, and only while that connection is still the current one; otherwise it
is refused ("try again") without showing items or moving the paging cursor,
also when the answer is empty. This holds when a replacement host behind the
same endpoint reuses the generation number and session id before the event
stream has reported the replacement.

When a running turn's history cannot be read (the host is busy or the read
fails), the turn stays explicitly *unsynchronized*: its updates are not
folded (that would start it without its prefix and with wrong ids), and the
read is retried with backoff (250 ms up to 5 s) on the next event of that
turn or on a timer, without needing a reconnect. If the turn ends first, its
end says that its items could not be read rather than showing an empty reply.
Within these rules a controller that joins mid-turn or overflows the log
converges on the host's text and ids; this is tested against the host's own
fold and a scripted gateway, not against a real agent.

At most 16 event streams are open per host; a stream whose controller went
away releases its slot immediately, even while idle. A controller is healthy
only while its stream is open: a refused stream (the limit is reached) or a
failed one leaves it unhealthy with conservative capabilities even though a
snapshot is still readable, until a stream actually opens.

Session working directories are host-admitted:

- A new session's cwd must be an existing absolute directory and is
  canonicalized; the association is stored once and never changed by later
  reports.
- When an existing session is reopened, the stored cwd always wins. A cwd
  reported by `session/list` is accepted only for sessions with no stored
  entry, and is validated and canonicalized the same way.
- A reopen (`session/load` or `session/resume`) is provisional until the
  agent answers successfully. While it is pending the session grants no
  *new* authority: no prompts, no option changes, and a directory that is
  only listed by the agent is not readable. A failed reopen revokes the
  admission and discards any partial replay.
- Authority this agent source *already persisted* is kept: a session
  recorded earlier keeps its stored directory for conversation file links,
  also while a new reopen of it is pending or after one failed.

The per-host store lives at `<state>/acp/<hex(name)>/<source>.json` (format
version 2, mode 0600, at most 500 entries). `<source>` is a keyed SHA-256 of
the resolved program path and the exact argument list; the key is a random
per-device file `<state>/acp/source-key` (mode 0600), so the file name reveals
nothing about the arguments and the store never contains them. Reusing a host
name with a different program or arguments therefore starts with an empty
store; no session or directory carries over. The host never writes on its
event path: admissions and titles are queued under the state lock (so their
order matches the state's) and coalesced to one pending change per session —
a burst of titles is one write of the newest — with at most 500 pending
sessions. A title reported while a reopen is pending is persisted with the
confirmed admission — including an explicit clear (`title: null`), which
removes a stored title; a reopen with no title signal leaves the stored
title as it is. One worker applies each batch through a unique temporary
file and an atomic rename; an unreadable store is moved aside
(`*.corrupt-<time>`) rather than treated as empty-and-valid. Version 1
stores (`sessions.json`) are not read.

## Setting up an agent

1. Open **Host this device**, choose **ACP agent**.
2. Pick the OpenCode preset or **Custom**; for a custom agent enter the
   program (name or full path) and one argument per row.
3. The dialog locates the program before starting. Start hosting.

The argv is stored in plain text in the App's preferences and is visible in
the dialog (the host's session store keeps only the keyed digest). **Do not
put secrets in arguments**; configure credentials with the agent's own
configuration. Each saved custom agent gets a random profile id, so two
agents whose names slug the same way (for example non-Latin names) are kept
separately.

Saved agents and "restore hosting on launch" are additive preference keys
(`acpAgents`, `autoHostAcp`); older versions ignore them.

Attachments follow the agent's capabilities everywhere they can enter the
composer — the image menu, the file picker, drag-and-drop and clipboard
paste: an agent that did not advertise image prompts gets no image (a short
notice says so; documents and text paste still work). Read-only views
(Guardian reviewers, a session another writer holds, child sessions) take no
attachment. A result that arrives later — a picker, a clipboard read, a file
read before its upload — is admitted again when it arrives, against the
draft chosen when the user acted: nothing enters (or is uploaded) once that
view turned read-only, its agent host was lost or replaced, or image support
went away. A document still being read when its conversation is left stays
in its original draft as a retryable attachment; no upload starts from a
view whose ownership subscriptions have ended. Uploads already dispatched
can finish in their original draft. Before uploading, the bridge checks the
account/relay context captured before the file read, and refuses a changed
context without transmitting the bytes.

## Accounts and older backends

ACP hosting works in self-host mode with a relay key and in hosted-account
mode with the normal Pocket-Codex account login. Agent authentication is
separate and uses the agent’s own CLI. Service names are claimed in one table shared by Codex, OpenCode and
ACP on a device, so two providers can never start under the same name. A
name stays claimed until its host has fully stopped, including relay and
listener cleanup and "stop all"; the same provider can still restart under
its own name.

On the controller, ACP connections, their relay subscriptions and the
relay meta tunnels (host files, uploads, file links) belong to the transport
context they were resolved in (signed-in account and relay, or self-host
relay and key). One revision orders all of it: a resolution captures it
before reading the configuration and becomes current only if it is
unchanged when it is observed (checked under the same lock a configuration
change takes), so a resolution that read the previous configuration never
replaces the newer context, restarts its credential refresher or drops its
state — it resolves again. An account transport takes relay, credential and
namespace from one credential answer. Service keys namespaced to another
account are refused, also when resolving this device's own loopback ACP
services. Changing account, relay or key, or signing out, drops every ACP
connection and relay meta tunnel of the old context; a credential refresh
for the same account keeps them. A connection or tunnel being established
across the change is registered only while its revision is still current,
checked under the registry's lock; two callers creating the same tunnel at
once end with one registered tunnel and the other stopped. These rules are
tested for the ACP engine and the meta tunnels; the native Codex loopback
meta lookup is unchanged and keyed by service key as before.

Connection health comes from the event stream, not from a task being alive:
after the host stops answering, the bridge emits `acp/host/state`
`{"connected": false}`, withdraws pending permission cards and reports
unnegotiated capabilities, and the session screen runs its normal bounded
reconnect. A connect succeeds only once the event stream is open (a host at
its stream limit is reported as such). A replaced local host (new port or new
incarnation) ends the old stream rather than retrying the old port forever; a
replaced host incarnation also makes the session screen re-read the
transcript.

The bridge asks the backend for `include_acp=true`; an older backend ignores
the parameter and does not list `acp:` services, so remote discovery of ACP
hosts needs an updated backend, while relay-key mode and the local host work
regardless.

## Verification status

The tests use an in-repo mock agent
(`crates/pocket-codex-host-svc/examples/mock_acp_agent.rs`) with three
capability profiles (minimal; load + list + resume + options; list + resume
without load). The extra profiles
only show that the protocol layer is not tied to one agent; they are not
real agents. It is a Cargo example: `cargo test` (and
`cargo test --workspace`) builds it next to the test binaries, and it is
never installed or shipped. The integration tests in
`crates/pocket-codex-host-svc/tests/acp_host.rs` run it as a real
subprocess and fail with an explicit message, rather than skipping, when it
was not built (for example `cargo test --test acp_host` alone; add
`--examples`); so do the host unit tests that need it (the cancel/turn race
and the delivery budgets against an agent that stops reading, in
`src/acp/host.rs`). Bridge recovery, refused-stream health and replacement
history are tested with a scripted gateway over real loopback HTTP; the
resolution race with a real resolver against fixture configurations; tunnel
ownership with real loopback listeners; quit and publication ownership with
the production tables and agents that are never launched. Flutter tests are
in `apps/flutter/test/acp_*_test.dart` and `attachment_gate_test.dart`.

The automated tests of owned hosting and its process handling (process
groups, `waitid` exit detection) have been run on macOS; other Unix desktops
share the code, but their lifecycle and runtime paths have not all been
exercised. Windows hosting stays disabled.
No real OpenCode binary was used, so the OpenCode preset has not been
verified against `opencode acp`; only the documented command line is
assumed.
