#!/usr/bin/env bash
# Install/upgrade the Pocket-Codex hosted backend on a Linux server.
#
# Run ON THE SERVER as a sudoer (e.g. after `ssh ubuntu@lb7666.top`). It is
# idempotent: safe to re-run for upgrades. It does NOT fill in secrets or touch
# the firewall — see the printed next steps (and deploy/README.md).
#
# Usage:
#   sudo ./deploy.sh /path/to/pocket-codex-backend          # install a prebuilt binary
#   sudo ./deploy.sh --build /path/to/pocket-codex-repo      # build from source then install
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_DEST=/usr/local/bin/pocket-codex-backend
ETC=/etc/pocket-codex
VAR=/var/lib/pocket-codex
UNIT=/etc/systemd/system/pocket-codex-backend.service

if [[ "${1:-}" == "--build" ]]; then
  REPO="${2:?usage: sudo ./deploy.sh --build <repo-path>}"
  echo "==> building release backend from $REPO"
  ( cd "$REPO" && cargo build --release -p pocket-codex-backend )
  BIN_SRC="$REPO/target/release/pocket-codex-backend"
else
  BIN_SRC="${1:?usage: sudo ./deploy.sh <binary-path> | --build <repo-path>}"
fi
[[ -f "$BIN_SRC" ]] || { echo "binary not found: $BIN_SRC" >&2; exit 1; }

echo "==> user + directories"
id -u pcx >/dev/null 2>&1 || useradd --system --home "$VAR" --shell /usr/sbin/nologin pcx
install -d -o pcx -g pcx -m 0750 "$VAR"
install -d -m 0755 "$ETC"

echo "==> config templates (only if absent — never clobber live secrets)"
[[ -f "$ETC/backend.toml" ]] || install -m 0644 "$HERE/backend.toml.example" "$ETC/backend.toml"
if [[ ! -f "$ETC/backend.env" ]]; then
  install -m 0600 -o pcx -g pcx "$HERE/backend.env.example" "$ETC/backend.env"
  NEEDS_SECRETS=1
fi

# Even on an upgrade re-run, refuse to claim success while secrets are still the
# shipped placeholders — the backend now fails closed on them at startup.
if grep -q 'replace-with' "$ETC/backend.env" 2>/dev/null; then
  NEEDS_SECRETS=1
fi

# If the relay runs with --use-machine-msg-header-key, adopt its cached key so
# the backend's loopback pb-mapper calls authenticate (the pb-mapper *default*
# key would NOT match a machine-derived one).
RELAY_KEY=/var/lib/pb-mapper-server/msg_header_key
if [[ -f "$RELAY_KEY" ]] && ! grep -q '^PCX_MSG_HEADER_KEY=' "$ETC/backend.env"; then
  echo "PCX_MSG_HEADER_KEY=$(tr -d '\n' < "$RELAY_KEY")" >> "$ETC/backend.env"
  echo "==> adopted relay machine key from $RELAY_KEY"
fi

echo "==> binary + unit"
install -m 0755 "$BIN_SRC" "$BIN_DEST"
install -m 0644 "$HERE/pocket-codex-backend.service" "$UNIT"
systemctl daemon-reload

echo
echo "==> done. Next steps:"
if [[ "${NEEDS_SECRETS:-0}" == "1" ]]; then
  echo "  ⚠ SECRETS NOT SET — the backend will REFUSE TO BOOT until you fill these:"
  echo "  1. Fill secrets:  sudoedit $ETC/backend.env"
  echo "     (PCX_JWT_SECRET = openssl rand -hex 32 (>=32 bytes);"
  echo "      PCX_GITHUB_CLIENT_ID = your GitHub OAuth app client id."
  echo "      PCX_MSG_HEADER_KEY is adopted automatically — set it only for a non-machine-keyed relay.)"
fi
echo "  2. TLS certs readable by the pcx user in $ETC/ (tls_cert/tls_key in $ETC/backend.toml):"
echo "     copy certbot/Caddy PEMs into $ETC/ (0640 pcx:pcx) via a renewal hook that restarts the unit."
echo "  3. Firewall the relay to loopback:  sudo ufw deny 7666/tcp"
echo "     Open API + broker:               sudo ufw allow 8443/tcp && sudo ufw allow 7900/tcp"
echo "  4. Start:  sudo systemctl enable --now pocket-codex-backend"
echo "     Watch:   journalctl -u pocket-codex-backend -f"
echo "     Check:   curl -fsS https://lb7666.top:8443/healthz   # -> ok"
