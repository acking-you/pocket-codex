# macOS GUI host startup

Finder and Dock launches do not inherit a terminal's PATH. An explicitly
selected npm-installed Codex can therefore be found, yet fail with
`env: node: No such file or directory` when Node is installed through nvm.

The macOS Runner resolves PATH from the user's interactive login shell before
creating the Flutter engine. Only PATH is imported; inherited entries are
retained after the shell's entries. No Node or Codex runtime is bundled, and
no shell configuration or user preferences are rewritten.

Shell startup output is discarded, with PATH captured separately in a private
temporary directory. A failed shell or a three-second timeout leaves the
inherited environment unchanged. A hung shell process is killed and reaped.

Run the native regression checks independently of Flutter:

```sh
cd apps/flutter/macos
xcrun swiftc -warnings-as-errors Runner/ShellEnvironment.swift \
  RunnerTests/ShellEnvironmentCheck.swift -o /tmp/pocket-shell-environment-check
/tmp/pocket-shell-environment-check
```

The checks reproduce an npm-style `#!/usr/bin/env node` launch with a sparse
Finder PATH, then verify the repaired launch, paths containing spaces,
inherited entries, deduplication, noisy startup files, failure, empty output,
and a startup script that ignores SIGTERM. Full release verification also
requires building the macOS Flutter app and launching the resulting bundle
from Finder with an external Codex installation.
