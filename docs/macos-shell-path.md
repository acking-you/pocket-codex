# macOS GUI host startup

Finder and Dock launches do not inherit a terminal's PATH. An explicitly
selected npm-installed Codex can therefore be found, yet fail with
`env: node: No such file or directory` when Node is installed through nvm.

The Rust host resolves PATH from the user's interactive login shell on demand
when locating or starting local Codex. Flutter's existing background bridge
tasks perform these operations: creating the window or connecting to a remote
host does not wait for a shell. No global environment mutation occurs. The same
resolved PATH is used to find Codex and explicitly passed to its child process,
including when the user selected an absolute npm launcher path.

Only PATH is imported, with inherited directories first to preserve existing
tool choices, followed by deduplicated shell directories. Terminal-attached
processes keep their environment as-is. The probe result is cached for the
process lifetime, so concurrent discovery and auto-host startup share it; restart
the App after changing shell configuration or correcting a failed probe.

Shell startup output is discarded, with PATH captured separately in a private
temporary directory and bounded to 128 KiB when read. A failed shell or a
three-second timeout logs a diagnostic and keeps the inherited environment.
The probe starts in a dedicated process group. Success, failure and timeout all
kill that group and reap its leader. The leader remains unreaped until group
cleanup, preventing its PID from being reused for an unrelated process group.
Normal foreground/background startup children are included; a program that
deliberately daemonizes into another session is outside this group.

Interactive login startup files still execute their commands. Resolving nvm or
similar shell-defined PATH entries requires this; the probe is not a sandbox
and cannot undo file writes or network requests from those scripts. No shell
configuration is rewritten and no Node or Codex runtime is bundled. Other App
subprocesses, Linux, Windows and mobile retain their original behavior.

Run the native checks on macOS (also required by the `macOS host environment`
job in CI):

```sh
cargo clippy -p pocket-codex-codex --all-targets --locked -- -D warnings
cargo test -p pocket-codex-codex --locked
```

The shared probe tests also run on Linux. They reproduce a sparse PATH and an
npm-style `#!/usr/bin/env node` launch; cover spaces, inherited precedence,
startup noise, invalid output and failures; and verify foreground/background
child cleanup without killing a separate process group. The native suite also
loads an isolated `.zshrc` with the system zsh. Finder launch verification with
an installed external Codex remains a manual platform check.
