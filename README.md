# Token Guard

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
`model` field in the request body, within the endpoint's format family
(`/v1/chat/completions` → OpenAI-format providers; `/v1/messages` → Anthropic).
Falls back to the default provider for that family. `GET /v1/models` returns the
merged model list.

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
OPENAI_BASE_URL=http://localhost:3742
OPENAI_API_KEY=dummy   # ignored; the proxy injects your stored key
```

## Status

v0.1.0 — prototype. Core proxy + model routing + SSE passthrough + logging +
tray. See `docs/POSITIONING.md` for the privacy/security/speed positioning and
the product brief for the roadmap.

## License

Proprietary. All rights reserved. See `LICENSE`.
