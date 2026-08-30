# Deploying the Pocket-Codex hosted backend

The backend serves the account HTTP API and nothing else. It authenticates a
client with GitHub, then hands it a **short-lived pb-mapper credential**; the
client takes that credential to the relay and talks to it **directly**. The
backend never carries client traffic.

```
                 lb7666.top
  ┌──────────────────────────────────────────────┐
  │ pocket-codex-backend                          │
  │   HTTP API  :8443  (TLS)  /auth/* /v1/*       │
  │     │  holds the relay ADMIN key              │
  │     │  mints per-account credentials          │
  │     ▼                                          │
  │ pb-mapper-server  :7666   (publicly reachable) │
  └──────────────────────────────────────────────┘
        ▲                              ▲
        │ 1. GET /v1/relay (JWT)       │ 2. register / connect
        │    → pbmt1_… credential      │    (every byte, direct)
     client ─────────────────────────────┘
```

## How isolation works, and what changed

Each account gets its own temporary credential, and the **relay** confines it to
that account's namespace — a credential cannot see or address another account's
services whatever key string it presents. The backend decides the namespace from
the verified JWT, so a client cannot ask for someone else's.

This replaces an earlier design where the backend tunnelled every byte through a
broker on `:7900` and the relay was firewalled to loopback. Two consequences to
be aware of when upgrading:

- **`:7666` must now be publicly reachable.** The old "firewall the relay to
  loopback (MANDATORY)" rule is deliberately gone; isolation is credential-based
  rather than network-based.
- **`--legacy-protocol deny` is REQUIRED.** The pre-v2 protocol lets any
  key-holder register any key, which is exactly what the loopback firewall was
  protecting against. With `:7666` open, allowing legacy would let one account's
  credential register into another's namespace. This is not a hardening
  nice-to-have; the design is unsound without it.

`:7900` is no longer used and can be closed.

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
3. **pb-mapper 0.5+** on the relay, and its 32-byte **administrator** key
   (`PCX_MSG_HEADER_KEY`). `deploy.sh` copies it out of
   `/var/lib/pb-mapper/auth/admin.key` for you when run with `sudo`, and replaces
   a stale pre-0.5 key if one is already configured.

   The copy is deliberate. That file is `0600 root` and the backend runs as the
   unprivileged `pcx`, so it cannot be read at runtime; the alternative — widening
   the relay's own key file — would expose it to every local user. The backend
   still *tries* the path first, which is what makes a same-user or root
   deployment configuration-free.

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

## Relay configuration (REQUIRED)

```bash
# Deny the pre-v2 protocol. Without this, any credential can register any key —
# see "How isolation works" above.
pb-mapper admin legacy-protocol set deny

# Confirm:
pb-mapper admin auth-status
```

Then open the API and relay ports:

```bash
sudo ufw allow 8443/tcp   # account API
sudo ufw allow 7666/tcp   # relay (clients connect here directly)
```

`relay_addr` in `backend.toml` is both what the backend dials AND what `/v1/relay`
tells clients to dial, so it must resolve for them too — `lb7666.top:7666`, not
`127.0.0.1:7666`.

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

To check the credential handoff specifically:

```bash
# With $TOKEN from a `pocket-codex login` session (config.toml):
curl -fsS -H "Authorization: Bearer $TOKEN" https://lb7666.top:8443/v1/relay
# -> {"relay_addr":"lb7666.top:7666","credential":"pbmt1_…","expires_at":…,"namespace":"…"}
```

A `credential` that does NOT start with `pbmt1_` means the backend handed out its
administrator key — stop and fix the configuration before any client sees it.

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
  backend therefore terminates its **own** TLS on `:8443`, reusing Caddy's
  Let's Encrypt cert
  (`/var/lib/caddy/.local/share/caddy/certificates/.../lb7666.top/lb7666.top.{crt,key}`,
  copied to `/etc/pocket-codex/` so `pcx` can read them). Caddy renews that cert,
  so add a renewal hook that re-copies + `systemctl restart pocket-codex-backend`.
- **Cloud security group:** `:8443` must be opened in the **Tencent Cloud
  console** (a local `ufw` won't help) for external clients to reach the API.
  `:7666` is already open, which the direct-connect design needs. `:7900` was the
  old broker port and can be closed.
- **Relay key:** the relay manages its own administrator key at
  `/var/lib/pb-mapper/auth/admin.key`, `0600 root`. `deploy.sh` copies it into
  `backend.env` because `pcx` cannot read the original; this box was upgraded from
  0.2, where the same key also lived at
  `/var/lib/pb-mapper-server/msg_header_key`, so both paths currently hold the
  same value. A future relay key rotation changes only the 0.5 path — re-run
  `deploy.sh` after one.
- **Version skew is what broke this deployment before** (2026-08-30): a 0.2.x
  client against the 0.5.0 relay could not decode the relay's structured
  over-quota error, read a permanent refusal as a transport fault, and reconnected
  without backoff until every connection slot was full. The whole client side is
  pinned to the published `pb-mapper` crate now so the two move together — but
  after upgrading the relay, restart the backend rather than only retiring
  connections: pools refill within seconds.
