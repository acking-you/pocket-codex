# Guardian approvals and session hierarchy

The **Approve for me** mode presents automatic approval reviews in the parent
conversation. A card starts at **Model reviewing** and updates in place to the
approved, denied, timed-out or aborted result. It shows the reviewed action,
risk, authorization and rationale when supplied by Codex. Original data remains
available through the card's existing details and copy controls.

These cards are display-only. They never answer an approval or execute the
reviewed action. Ordinary manual approval prompts keep their existing controls.

Both the conversation sidebar and the session inventory use explicit parent
links. Parents have an expand control; children remain below their parent across
activity and project groups. Search and status filters retain matching children's
ancestors. Missing parents and malformed cycles leave sessions visible at the
root rather than hiding them.
Ancestors of running sessions expand automatically in both lists, so the actual
active child and its status remain visible.

Guardian reviewer sessions open through read-only history and monitoring, with a
link back to the parent. They cannot be resumed or taken over, including through
host force-resume routes. Automatic home selection chooses a normal root session.
The Rust resume boundary checks authoritative metadata too; direct FRB callers
cannot bypass read-only mode, and failed metadata reads never proceed to resume.

## Data flow

- Live `item/autoApprovalReview/started` and `/completed` events carry a
  `reviewId`, `turnId`, `targetItemId`, action and review. The bridge retains one
  `auto-review:<reviewId>` item and invalidates cached history on either update.
- Codex's ordinary history response omits successful Guardian assessments. The
  host history adapter therefore indexes the latest `guardian_assessment` record
  offset per review in the original rollout. It reads relevant records when
  returning an item page and attaches them to their target, or a user row in the
  same turn when that page has no target. Stable review IDs deduplicate overlap.
  Native document counts and cursors are unchanged. Existing source-replacement
  checks and controller cache quotas still apply; the index contains offsets,
  not another transcript copy.
- `parentThreadId` and `threadSource` travel through the thread and local-session
  APIs. Older rollout headers use `parent_thread_id`, `thread_source`, or explicit
  subagent source metadata. Titles are never used to infer ancestry or Guardian
  identity.

New fields are optional for older hosts. Restoring assessments that were never
observed live requires the updated host history service. The controller cannot
recover data omitted by an older host; live approvals still use native events.

## Verification

Rust coverage exercises lifecycle normalization, latest-record indexing,
replacement invalidation, turn/target isolation and metadata parsing. Flutter
coverage exercises parent/child expansion, cycles and missing parents, live card
replacement, and read-only Guardian navigation at phone, tablet and desktop
widths. `history_sync_codex` also checks approved assessment recovery with an
isolated real external Codex app-server across client and history-service restarts;
it sends no model requests.
