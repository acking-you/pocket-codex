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

# Adopt the relay's ADMINISTRATOR key so the backend can mint per-account
# credentials against it.
#
# Two paths, in order, because the key moved in pb-mapper 0.5:
#   - 0.5+: /var/lib/pb-mapper/auth/admin.key   (the root of the derivation)
#   - 0.2 : /var/lib/pb-mapper-server/msg_header_key  (the old shared key)
#
# The 0.5 file is 0600 root, and the backend runs as the unprivileged `pcx`, so
# it cannot read it directly — the key is copied into the 0600 pcx-owned
# backend.env instead. That is a deliberate second copy: the alternative is
# loosening the relay's own key file, which would expose it to every local user.
#
# An install upgraded from 0.2 may already carry the OLD key in backend.env. It
# is replaced when the two differ, because a stale shared key fails every admin
# RPC — and the failure surfaces as "issuing a relay credential" errors on
# /v1/relay rather than anything that names the key.
ADMIN_KEY=/var/lib/pb-mapper/auth/admin.key
LEGACY_KEY=/var/lib/pb-mapper-server/msg_header_key
RELAY_KEY=""
for candidate in "$ADMIN_KEY" "$LEGACY_KEY"; do
  if [[ -r "$candidate" ]]; then
    RELAY_KEY="$candidate"
    break
  fi
done

if [[ -n "$RELAY_KEY" ]]; then
  WANT="$(tr -d '\n' < "$RELAY_KEY")"
  HAVE="$(sed -n 's/^PCX_MSG_HEADER_KEY=//p' "$ETC/backend.env" | tail -1)"
  if [[ "$WANT" != "$HAVE" ]]; then
    # Rewrite in place rather than appending: a second assignment would leave the
    # old one in the file, and which wins is up to the reader.
    sed -i '/^PCX_MSG_HEADER_KEY=/d' "$ETC/backend.env"
    echo "PCX_MSG_HEADER_KEY=$WANT" >> "$ETC/backend.env"
    chmod 0600 "$ETC/backend.env"
    chown pcx:pcx "$ETC/backend.env" 2>/dev/null || true
    if [[ -n "$HAVE" ]]; then
      echo "==> replaced a stale relay key with the administrator key from $RELAY_KEY"
    else
      echo "==> adopted the relay administrator key from $RELAY_KEY"
    fi
  else
    echo "==> relay administrator key already current"
  fi
elif ! grep -q '^PCX_MSG_HEADER_KEY=' "$ETC/backend.env"; then
  # Neither file is readable and nothing is configured: the backend exits with
  # "relay_credential is required", so say why now rather than after it fails.
  echo "  ⚠ could not read the relay administrator key ($ADMIN_KEY)."
  echo "    Run this script with sudo, or set PCX_MSG_HEADER_KEY in $ETC/backend.env"
  echo "    by hand — the backend will not boot without it."
  NEEDS_SECRETS=1
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
  echo "      PCX_MSG_HEADER_KEY is the relay's ADMINISTRATOR key, adopted automatically when"
  echo "      this script can read it — set it by hand only for a relay on another host.)"
fi
echo "  2. TLS certs readable by the pcx user in $ETC/ (tls_cert/tls_key in $ETC/backend.toml):"
echo "     copy certbot/Caddy PEMs into $ETC/ (0640 pcx:pcx) via a renewal hook that restarts the unit."
echo "  3. Deny the pre-v2 relay protocol (REQUIRED):  pb-mapper admin legacy-protocol set deny"
echo "     Open API + relay:  sudo ufw allow 8443/tcp && sudo ufw allow 7666/tcp"
echo "  4. Start:  sudo systemctl enable --now pocket-codex-backend"
echo "     Watch:   journalctl -u pocket-codex-backend -f"
echo "     Check:   curl -fsS https://lb7666.top:8443/healthz   # -> ok"
