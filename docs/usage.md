# Pocket-Codex usage guide (GitHub account mode)

> 中文版：[`docs/usage.zh-CN.md`](usage.zh-CN.md)

The simplest way to use Pocket-Codex: **sign in with a GitHub account**. One
machine stays logged in to Codex; every other device drives it after signing in
to the **same** account. There is no relay address or shared key to manage — the
backend brokers your services for you.

> Running your own relay instead? See the *Self-host (advanced)* section of the
> [README](../README.md). Hosting the backend yourself? See
> [`deploy/README.md`](../deploy/README.md).

---

## 0. Before you start

- A **`pocket-codex-backend`** must be reachable. Someone runs it once (you, or
  whoever owns your server). The CLI and app use a built-in default backend; to
  point at your own, pass `--backend https://your-host` to `login` (it is
  remembered) or set it in the app's advanced settings.
- The **host machine** (the one that exposes Codex) needs a working `codex`
  binary on `PATH`. Client devices do **not** need Codex installed — except for
  the CLI `connect` path, which runs a local `codex --remote`.

---

## 1. Sign in with GitHub

### App

Launch the app → **Sign in with GitHub**. It shows a short code and opens
`github.com/login/device`; enter the code and authorize. You land on the home
screen, which shows the account you signed in as.

### CLI

```bash
pocket-codex login
```

This prints a verification URL and a one-time code. Open the URL, enter the code,
authorize. Confirm with:

```bash
pocket-codex account     # → signed in as @you, mode = account
```

---

## 2. Host your Codex

On the machine that has `codex` installed and logged in:

```bash
pocket-codex serve
```

This starts (or reuses) the local `codex app-server` and registers it under your
account. Leave it running — this is what your other devices connect to.

Useful options:

- `--name work` — run more than one (each service gets its own name).
- `--proxy http://…` — reach `chatgpt.com` through an upstream proxy.

---

## 3. Drive it from any device

Sign in to the **same** GitHub account on the other device, then:

### App (recommended)

The home screen lists your app-server with a live health dot. Tap it, then:

- **New conversation** (the **＋** button); pick a working directory.
- Type a prompt and send. **Thinking** and **tool calls** stream in live with an
  elapsed timer.
- Composer controls: **model**, **approval mode**, **working directory**,
  **plan mode**, and **reasoning effort**.
- **Approvals** — when Codex asks to run a command or apply a patch, approve or
  deny it inline.
- **Stop** interrupts a running turn.
- Re-open a conversation any time to see its history — and live progress if a
  turn is still running.

### CLI

```bash
pocket-codex services list      # see your registered services
pocket-codex connect            # subscribe; prints the exact codex command
codex --remote ws://127.0.0.1:28080
```

`connect` exposes the remote app-server on a local port and prints the
`codex --remote …` line to run a normal Codex session against it. Pick a specific
host/instance with `--device <id>` / `--name <name>` when you have more than one.

---

## 4. Use it as an OpenAI-compatible API (optional)

Reach your Codex login as a standard Responses API from any device.

On the host:

```bash
pocket-codex api serve
```

On any device:

```bash
pocket-codex api connect        # prints a local model_providers config snippet
```

Point any OpenAI-compatible tool at the printed local endpoint (it serves
`/v1/responses`). In the app, the API service appears on the home screen with its
own health indicator.

---

## 5. Status & sign out

```bash
pocket-codex account            # who you are + transport mode
pocket-codex status             # local serve / api / codex state
pocket-codex stop               # stop the local serve / api
pocket-codex logout             # revoke the session + clear the local token
```

In the app: **Settings → Account → Sign out**.

---

## Troubleshooting

- **"app-server unreachable" in the app** — the host's `pocket-codex serve` isn't
  running or lost its connection. (Re)start it on the host. The app re-probes
  periodically; the refresh control forces an immediate check.
- **Login expired** — run `pocket-codex login` again. While a session is valid it
  refreshes automatically, so you normally only log in once per device.
- **Multiple hosts or services** — give each a `--name`, then select it with
  `--name` / `--device` on the CLI, or by tapping the right service in the app.
