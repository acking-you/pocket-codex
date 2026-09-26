# Session file links

Click a Markdown file link or a document attachment chip to see its full host
path and choose **Preview** or **Download**. Opening the chooser does not read
the file. Absolute paths, `file:///` URIs, session-relative paths and editor
line suffixes such as `:12:3` or `#L12` are accepted. Relative paths use the
session's working directory, including the host's Windows path convention.

Preview displays UTF-8 text, code and Markdown source as selectable text, and
common raster image formats with zoom. HTML is shown as source rather than
executed. PDF, Office documents, archives and other binary formats are saved
for another application. The host sends at most 8 MiB for a preview, and text
display is limited to 1 MiB with a visible truncation notice. Download always
requests the complete file. Load failures retain an explicit retry action.

## Host and controller

For remote sessions, the controller requests the selected file through the
host's existing authenticated meta tunnel. Downloads stream to a unique private
temporary directory, then use the device's native Save As picker. Android uses
its document provider; iOS uses the document export picker. Desktop uses the
existing file selector. Staging files are removed after success, cancellation,
failure, or completion of a request whose dialog has been dismissed. An active
transfer finishes in the background if its dialog is dismissed.

An in-process host can be accessed directly. A separately running host on the
same machine must first prove access to a short-lived local filesystem
challenge: only the random filename is sent over the tunnel, while the token
remains on disk. The file is removed after the check. A localhost tunnel address
or matching device name alone does not prove that a host is on this device.
Unsupported or failed checks are treated as remote.

For a remote session, an HTTP(S) link to localhost, a loopback address or a
wildcard bind address displays pb-mapper mapping guidance. Register the web
service's port on the host and subscribe to that service on the controller,
then open the mapped local port with the original URL path/query. Pocket-Codex
does not create that port mapping automatically. For a verified same-device
host, the original web address opens directly in the system browser. Loopback
links never trigger an automatic favicon request.

## Additive meta API

- `GET /fs/thread-file?thread=<optional-id>&href=<original-link>&preview=true`
  returns a bounded raw-byte preview. Omit `preview` (or set it to `false`) for
  a streamed complete download. `x-file-size` is the full size; `Content-Length`
  describes the response before any negotiated compression. Responses specify
  `Cache-Control: no-store`. The client verifies length and removes incomplete
  downloads; a stalled body fails after 30 seconds.
- Files must be within configured project roots, within the session cwd after
  canonicalization, or explicitly referenced by the selected session's
  transcript. Missing/invalid sessions cannot grant access. Paths outside roots
  cannot be selected by providing a prefix of some other referenced filename.
  This endpoint is used only after an explicit Preview/Download action. The
  narrower `/fs/thread-image` authorization remains in place for automatic image
  thumbnails; model-written text does not grant automatic image reads.
- `GET /host/local-probe?id=<uuid>` reads only the corresponding UUID-named file
  from the current user's temporary `state_dir()/local-probes` directory and
  returns its UUID token or null. It cannot browse arbitrary paths or create
  files. No persistent machine identifier is transmitted.

Existing endpoints and app-server protocol fields remain unchanged. Remote file
links and streamed file-browser downloads require an updated host. Older hosts
show a retryable error; they do not fall back to reading the controller's disk.
