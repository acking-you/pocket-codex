# Native image generation in remote sessions

Ask the connected Codex to generate an image in the normal composer. Pocket-Codex
uses the external Codex runtime's native image tool; availability follows that
runtime's model, provider, account and feature configuration. Plan mode is under
**Turn settings → Advanced settings** on desktop and mobile. Explicitly saved
Plan sessions remain supported and keep their active-mode indicator.

Generated images stay visible outside the folded tool activity. While the native
item is running, the controller shows an animated image placeholder without an
invented percentage. Downloading its artifact has a separate loading state.
Reduced-motion settings disable the animation. A failed generation or an
unconfirmed/incomplete historical item does not spin indefinitely. History uses
the active turn ID from the same server snapshot as the running status; an
unrelated new turn cannot restart an old image's spinner. Usage-limit failures
have a distinct message. Read-only session views display completed native images
through the selected host too; their legacy rollouts lack active turn identity,
so incomplete results stay inactive until authoritative progress is available.

Tap a completed thumbnail for the existing zoomable viewer. Desktop users can
save from that viewer or the thumbnail. Mobile viewing is supported; saving to a
mobile photo library is not implemented. Failed artifact downloads retain the
filename and an explicit Retry action, without an automatic request loop.

## Transport and compatibility

- `item/started` and `item/completed` carry `imageGeneration` snapshots. The
  bridge maps `savedPath` into the existing image-reference field. When no path
  exists, it uses the inline base64 `result` instead, bounded to 8 MiB decoded
  size before DTO construction. Result bytes never become transcript text or
  duplicate the image field through the raw event payload.
- The same mapping applies to reopened and paginated history. Metadata-only
  monitoring updates the image references of existing rows, so a generation
  started by another writer can finish without changing its row identity.
- The host recognizes both the core `ImageGeneration` rollout item and the
  exact extension kind `image_gen.generation`. `/fs/thread-image` authorizes
  the recorded artifact path. Paths mentioned only in a prompt, reply or
  arbitrary tool output do not authorize this endpoint.
- Saved artifacts outside shared project roots require the updated host meta
  service. Older hosts retain root-confined file fallback. A removed or refused
  artifact stays visibly unavailable and retryable. Opaque `fileId` references
  remain unavailable placeholders and never become filesystem reads.
- Preview reads are capped at 8 MiB. Visible image strips own their downloaded
  bytes and discard them when their host/thread scope changes; they do not share
  a global filename cache. Remote reads still reuse the existing bounded on-disk
  preview cache under the App's shared history budget; it is not an offline archive.

This feature covers image output in Pocket-Codex remote sessions. It does not
enable Codex's separate `remoteControl/*` pairing service.

## Verification

A live check on 2026-09-26 used an isolated external Codex 0.156.1 process and
temporary `CODEX_HOME`/working directory. Its native image tool emitted the
`in_progress` and `completed` lifecycle edges and saved a 787,265-byte PNG. The
real Pocket-Codex meta HTTP router returned exactly the same bytes and refused an
unreferenced path. Captured native events passed the bridge's live/history mapping
check. No active Pocket-Codex host was restarted or reconfigured.

The opt-in checks consume an existing isolated generation; they do not issue
another model request. Set these paths to your own capture and artifact:

```sh
export CODEX_HOME=/path/to/isolated/codex-home
export PCX_IMAGE_THREAD_ID=<generated-thread-id>
export PCX_IMAGE_OUTPUT=/path/to/generated.png
export PCX_IMAGE_CAPTURE=/path/to/app-server-events.jsonl
cargo test -p pocket-codex-host-svc --test projects_http \
  native_generated_image_round_trips_over_meta_http -- --ignored --nocapture
cargo test -p pocket_codex_bridge native_image_capture_maps_live_and_restored_artifacts \
  -- --ignored
cd apps/flutter
fvm flutter test test/generated_image_card_test.dart \
  --dart-define=PCX_IMAGE_OUTPUT="$PCX_IMAGE_OUTPUT"
```

Normal tests cover live completion, history, remote monitoring, permission
boundaries, transient download failure/retry, host/thread isolation, and light/dark
layouts at 360, 800 and 1280 pixels. They also verify both desktop and mobile
Advanced settings, including existing saved Plan behavior.
