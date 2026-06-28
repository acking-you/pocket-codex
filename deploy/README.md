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
   *Optional — to also enable the browser-redirect (web) login:* in the same
   OAuth App, set the **Authorization callback URL** to
   `https://<host:port>/auth/web/callback` (e.g.
   `https://lb7666.top:8443/auth/web/callback`) and generate a **client secret**,
   then set `PCX_GITHUB_CLIENT_SECRET` (backend.env) + `public_url` (backend.toml)
   to that same base. One OAuth App + one client id serves both flows; leaving the
   secret unset keeps the web flow off (device flow still works).
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

# Config + secrets (or just run ./deploy.sh, which does all of this correctly).
sudo install -m 0644 backend.toml.example /etc/pocket-codex/backend.toml
# Create the secret file 0600 + pcx-owned BEFORE typing any secret into it, and
# use sudoedit (sudo resets $EDITOR's environment):
sudo install -m 0600 -o pcx -g pcx backend.env.example /etc/pocket-codex/backend.env
sudoedit /etc/pocket-codex/backend.env   # set PCX_JWT_SECRET + PCX_GITHUB_CLIENT_ID

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

## Notes from the lb7666.top deployment

Validated on the live server (Tencent Cloud Ubuntu, 2 GB RAM):

- **Caddy** owns `:80`/`:443` (TLS for `lb7666.top` → an existing app). The
  backend therefore terminates its **own** TLS on `:8443` (API) and `:7900`
  (broker), reusing Caddy's Let's Encrypt cert
  (`/var/lib/caddy/.local/share/caddy/certificates/.../lb7666.top/lb7666.top.{crt,key}`,
  copied to `/etc/pocket-codex/` so `pcx` can read them). Caddy renews that cert,
  so add a renewal hook that re-copies + `systemctl restart pocket-codex-backend`.
- **Cloud security group:** only `:80`/`:443`/`:7666` were open. `:8443`+`:7900`
  must be opened in the **Tencent Cloud console** (a local `ufw` won't help) for
  external clients to reach the API + broker.
- **Relay key:** the relay runs `--use-machine-msg-header-key`; `deploy.sh`
  adopts it from `/var/lib/pb-mapper-server/msg_header_key` automatically.
- **Verified on the box** (loopback, since the cloud SG blocks the ports
  externally): `GET /healthz` (TLS), `GET /v1/me` (a JWT), `GET /v1/services`
  (relay query with the matching key), and a full broker
  register→relay→subscribe→echo via
  `cargo run -p pocket-codex-broker-client --example broker_smoke`.
