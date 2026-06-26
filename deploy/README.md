# Deploying the Pocket-Codex hosted backend

The backend is one self-contained binary that serves the account HTTP API and
the broker, and bridges authenticated clients to a **loopback** pb-mapper relay
holding the real `MSG_HEADER_KEY`. Clients only ever hold a GitHub-issued session
token; they never see the relay key and can never reach the relay directly.

```
                 lb7666.top
  ┌──────────────────────────────────────────────┐
  │ pocket-codex-backend                          │
  │   HTTP API  :8443  (TLS)  /auth/* /v1/*       │
  │   broker    :7900  (TLS)                       │
  │     │  speaks pb-mapper (loopback, real key)   │
  │     ▼                                          │
  │ pb-mapper-server 127.0.0.1:7666  (loopback!)   │
  └──────────────────────────────────────────────┘
```

## Prerequisites (provided by you)

1. **GitHub OAuth App** with *Device Flow* enabled → its **client id**
   (`PCX_GITHUB_CLIENT_ID`). The device flow uses no client secret.
2. A TLS certificate for the host clients connect to (e.g. `lb7666.top`). The
   simplest path is certbot:
   ```
   sudo certbot certonly --standalone -d lb7666.top
   ```
   (or reuse an existing cert / a reverse proxy — see "TLS options").
3. The existing relay's 32-byte `MSG_HEADER_KEY` (`PCX_MSG_HEADER_KEY`).

## One-time server setup

```bash
sudo useradd --system --home /var/lib/pocket-codex --shell /usr/sbin/nologin pcx || true
sudo mkdir -p /var/lib/pocket-codex /etc/pocket-codex
sudo chown pcx:pcx /var/lib/pocket-codex

# Config + secrets
sudo cp backend.toml.example  /etc/pocket-codex/backend.toml
sudo cp backend.env.example   /etc/pocket-codex/backend.env
sudo "$EDITOR" /etc/pocket-codex/backend.env   # fill in the three secrets
sudo chmod 600 /etc/pocket-codex/backend.env
sudo chown pcx:pcx /etc/pocket-codex/backend.env

# Binary + unit
sudo install -m755 pocket-codex-backend /usr/local/bin/pocket-codex-backend
sudo cp pocket-codex-backend.service /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now pocket-codex-backend
```

## Firewall the relay to loopback (MANDATORY)

The per-account isolation guarantee collapses if any key-holder can reach the
relay directly. Bind/allow `:7666` to localhost only:

```bash
# If pb-mapper-server can bind a specific address, prefer 127.0.0.1:7666.
# Otherwise block it at the firewall:
sudo ufw deny 7666/tcp
```

Open only the API and broker ports to the world:

```bash
sudo ufw allow 8443/tcp
sudo ufw allow 7900/tcp
```

## TLS options

- **files** (recommended): `tls_mode = "files"` + `tls_cert`/`tls_key` pointing
  at Let's Encrypt PEMs. The backend reloads them on restart; add a certbot
  deploy hook that runs `systemctl restart pocket-codex-backend`.
- **plain**: `tls_mode = "plain"` — no TLS. Only for a local smoke test or
  behind a TLS-terminating proxy; **never** expose plain to the internet (the
  session token would travel in cleartext).

## Smoke test

```bash
curl -fsS https://lb7666.top:8443/healthz        # -> ok
journalctl -u pocket-codex-backend -f            # watch logs
```

Then from a client: `pocket-codex login` → `pocket-codex serve`, and on another
device sign in with the **same** GitHub account and open the registered service.

## Building the binary

On the server (has Rust):
```bash
cargo build --release -p pocket-codex-backend
# target/release/pocket-codex-backend
```
Or cross-compile a static musl binary (see `.github/workflows/release.yml` for
the `cross` setup) and `scp` it over.
