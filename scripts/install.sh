#!/bin/sh
# Pocket-Codex CLI installer for Linux and macOS.
#
# One-liner:
#   curl -fsSL https://raw.githubusercontent.com/acking-you/pocket-codex/main/scripts/install.sh | sh
#
# Downloads the `pocket-codex` binary from the latest GitHub release, matching
# your OS + CPU, and installs it to a bin dir on your PATH.
#
# Environment overrides:
#   POCKET_CODEX_VERSION       pin a release tag (e.g. v0.1.3); default: latest
#   POCKET_CODEX_BIN_DIR       install dir; default: $HOME/.local/bin
#   POCKET_CODEX_NO_MODIFY_PATH set to 1 to skip the PATH hint (still installs)
#
# POSIX sh; no bashisms. Depends only on: curl or wget, tar, uname, mkdir, mv.
set -eu

REPO="acking-you/pocket-codex"
BIN="pocket-codex"

info() { printf '\033[1;34m==>\033[0m %s\n' "$1"; }
warn() { printf '\033[1;33mwarning:\033[0m %s\n' "$1" >&2; }
err() { printf '\033[1;31merror:\033[0m %s\n' "$1" >&2; exit 1; }

# --- fetch helper: prefer curl, fall back to wget ---------------------------
if command -v curl >/dev/null 2>&1; then
  fetch() { curl -fsSL "$1"; }
  download() { curl -fsSL "$1" -o "$2"; }
elif command -v wget >/dev/null 2>&1; then
  fetch() { wget -qO- "$1"; }
  download() { wget -qO "$2" "$1"; }
else
  err "need curl or wget to download; please install one."
fi

command -v tar >/dev/null 2>&1 || err "need tar to unpack the archive."

# --- detect OS + CPU → Rust target triple -----------------------------------
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
  Linux)
    case "$arch" in
      x86_64 | amd64) target="x86_64-unknown-linux-musl" ;;
      aarch64 | arm64) target="aarch64-unknown-linux-musl" ;;
      armv7l | armv6l | armhf) target="armv7-unknown-linux-musleabihf" ;;
      *) err "unsupported Linux CPU: $arch (have x86_64 / aarch64 / armv7)" ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      x86_64) target="x86_64-apple-darwin" ;;
      arm64 | aarch64) target="aarch64-apple-darwin" ;;
      *) err "unsupported macOS CPU: $arch" ;;
    esac
    ;;
  *) err "unsupported OS: $os (this script is for Linux/macOS; use install.ps1 on Windows)" ;;
esac

# --- resolve the download URL for this target -------------------------------
# The release asset is `pocket-codex-cli-<tag>-<target>.tar.gz`. We match on the
# full target triple so a Linux target can't collide with the Windows one.
if [ -n "${POCKET_CODEX_VERSION:-}" ]; then
  ver="$POCKET_CODEX_VERSION"
  url="https://github.com/${REPO}/releases/download/${ver}/pocket-codex-cli-${ver}-${target}.tar.gz"
  info "installing pinned $ver for $target"
else
  info "resolving the latest release for $target"
  # Parse the release JSON without jq: pick the matching browser_download_url.
  url="$(
    fetch "https://api.github.com/repos/${REPO}/releases/latest" |
      grep browser_download_url |
      grep "pocket-codex-cli-" |
      grep "${target}.tar.gz" |
      head -n 1 |
      cut -d '"' -f 4
  )"
  [ -n "$url" ] || err "could not find a CLI asset for $target in the latest release."
fi

# --- download + unpack ------------------------------------------------------
tmp="$(mktemp -d "${TMPDIR:-/tmp}/pocket-codex.XXXXXX")"
# shellcheck disable=SC2064
trap "rm -rf '$tmp'" EXIT INT TERM

info "downloading $url"
download "$url" "$tmp/cli.tar.gz" || err "download failed."
tar -xzf "$tmp/cli.tar.gz" -C "$tmp" || err "unpack failed."

# The archive holds one dir `pocket-codex-cli-.../` containing the binary.
src="$(find "$tmp" -type f -name "$BIN" | head -n 1)"
[ -n "$src" ] || err "the archive did not contain a '$BIN' binary."
chmod +x "$src"

# --- install ----------------------------------------------------------------
bindir="${POCKET_CODEX_BIN_DIR:-$HOME/.local/bin}"
mkdir -p "$bindir" || err "could not create install dir: $bindir"
mv -f "$src" "$bindir/$BIN" || err "could not write to $bindir (try POCKET_CODEX_BIN_DIR=/some/writable/dir)."

info "installed $BIN → $bindir/$BIN"
"$bindir/$BIN" version 2>/dev/null || warn "installed, but '$BIN version' did not run cleanly."

# --- PATH hint --------------------------------------------------------------
if [ "${POCKET_CODEX_NO_MODIFY_PATH:-0}" != "1" ]; then
  case ":$PATH:" in
    *":$bindir:"*) : ;; # already on PATH
    *)
      warn "$bindir is not on your PATH."
      printf '  Add this line to your shell profile (~/.bashrc, ~/.zshrc, ~/.profile):\n'
      printf '    export PATH="%s:$PATH"\n' "$bindir"
      ;;
  esac
fi

info "done. Run '$BIN --help' to get started."
