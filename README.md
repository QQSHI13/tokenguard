# Token Guard

[![GitHub release](https://img.shields.io/github/v/release/QQSHI13/tokenguard)](https://github.com/QQSHI13/tokenguard/releases)
[![CI](https://img.shields.io/github/actions/workflow/status/QQSHI13/tokenguard/ci.yml?branch=main)](https://github.com/QQSHI13/tokenguard/actions/workflows/ci.yml)
[![License](https://img.shields.io/github/license/QQSHI13/tokenguard)](LICENSE)
[![Downloads](https://img.shields.io/github/downloads/QQSHI13/tokenguard/total)](https://github.com/QQSHI13/tokenguard/releases)

> The local LLM gateway. Your keys, your machine, your tokens.

A cross-platform desktop app (Tauri v2 + Rust) that runs a local HTTP proxy to
intercept and log LLM API calls, showing real-time cost in the system tray.

**The only LLM cost monitor that *cannot* see your prompts.** No cloud, no
account, no telemetry. The proxy forwards bytes to the provider you already call
and records only metadata (tokens, model, cost) to a local SQLite database.
API keys live in the OS keychain — never on disk, never in your code.

## Architecture

| Layer | Tech |
|---|---|
| Shell | Tauri v2 (native tray, webview settings window) |
| Proxy | Rust — axum server + reqwest streaming client |
| DB | SQLite (rusqlite, WAL) — local-first |
| Secrets | OS keychain via `keyring` (Win Credential Manager / macOS Keychain / Linux Secret Service) |
| Frontend | React 19 + Tailwind v4 (settings/dashboard only) |

## Routing

One base URL (`http://127.0.0.1:3742`). Requests are routed to a provider by the
`model` field in the request body, within the endpoint's format family:

- `/v1/chat/completions` and `/v1/responses` → OpenAI-format providers
- `/v1/completions` → OpenAI legacy completions
- `/v1/messages` → Anthropic Messages API
- `/v1beta/models/{model}:generateContent` and `:streamGenerateContent` → Gemini
  API (streaming via the method suffix or `?alt=sse`; `GET /v1beta/models` and
  `/v1beta/models/{model}` also work)

Falls back to the default provider for that family. `GET /v1/models` returns the
merged local model list. Any client format can be routed to any provider format —
requests, responses, and SSE streams are converted as needed (the "3 × 3").

### Model aliases

Each provider model has a **local name** (what you send) and an optional
**provider/remote name** (what the upstream API expects). For example, you can
send `"model": "claude-sonnet-4"` locally while the proxy forwards it as
`claude-sonnet-4-20250514` to Anthropic.

## Build & run

Requires: Rust (stable, MSVC on Windows), Node 18+, WebView2 (Windows).

```bash
npm install
cargo tauri dev
```

First run builds the Rust backend (slow). A window + green shield tray icon
appear. Add a provider in the **Providers** tab, then point any OpenAI-compatible
client at `http://localhost:3742`:

```bash
OPENAI_BASE_URL=http://localhost:3742/v1
OPENAI_API_KEY=dummy   # ignored; the proxy injects your stored key
```

### Keychain note

Token Guard stores API keys in the OS keychain:

- **Windows** → Credential Manager
- **macOS** → Keychain
- **Linux/WSL** → D-Bus Secret Service

If the in-app **Test keychain** button fails with *“No matching entry found in
secure storage”*:

- On **Windows**: ensure the *Credential Manager* service is running and you are
  not inside a sandbox/container that blocks Win32 credential APIs.
- On **macOS**: ensure Keychain Access is available and the app has keychain
  entitlements.
- On **Linux/WSL**: install and start a secret-service provider:
  - GNOME Keyring: `sudo apt install gnome-keyring` then
    `gnome-keyring-daemon --start --components=secrets`.
  - KeePassXC with secret-service integration enabled.
  - KWallet with the secret-service interface enabled.

**Note:** If you synced the repo from a Linux/WSL shell but are running
`cargo tauri dev` inside WSL, the compiled binary is a Linux binary and needs a
Linux secret-service provider. To use Windows Credential Manager, build and run
from PowerShell or CMD on Windows.

## Cost accuracy

Cost estimates use a small, built-in pricing table for common models. Provider
pricing changes frequently, so the estimate may drift until you set the exact
input/output price per provider in **Settings**. Token Guard never fetches
pricing from the internet.

## Limits & subscriptions

The **Limits** tab lets you set caps on:

- **Money** ($)
- **Tokens** (prompt + completion)
- **Requests** (count)
- **Time** (wall-clock seconds)

Each limit has a reset period (one-time, hourly, daily, weekly, monthly, or
custom seconds) and can be scoped globally, per provider, or per project.
When a limit is exceeded you can choose to:

- **Warn** — log and color the tray icon.
- **Block** — return HTTP 429 for subsequent requests.
- **Pause** — pause the proxy until you resume it.

This covers subscription-style APIs such as "5 hours per day", "1 000 requests
per day", or "1 M tokens per month". The legacy daily budget is automatically
migrated to a global daily money limit.

## Status

v0.1.0 — prototype. Core proxy + model routing + SSE passthrough + logging +
tray. See `PRIVACY.md` for how user data is handled.

## License

Apache-2.0. See `LICENSE`.
