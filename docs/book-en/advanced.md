# Advanced features

## Fallback provider

In the provider form, choose another provider as the fallback. If the primary returns 5xx, 429, or a network error, Token Guard forwards the same request to the fallback. Format conversion is applied automatically, so an OpenAI-format provider can fall back to an Anthropic-format one.

## Auto-export

Token Guard can periodically export all request logs to a CSV file.

1. In **Settings**, set **Auto-export folder**.
2. Set **Auto-export interval (days)**.
3. The export runs on the next launch if the interval has passed.

The CSV contains: timestamp, provider, model, prompt tokens, completion tokens, cost, and project tag.

## Webhook notifications

When a limit is hit or crosses its warning threshold, Token Guard can POST to a webhook URL. The payload includes the limit name, metric, used value, and cap.

Set the webhook URL in **Settings**. Use this to integrate with Slack, Discord, n8n, or your own service.

## LAN exposure

By default the proxy listens on `127.0.0.1:3742`. Enable **Expose proxy to LAN** in Settings to bind to `0.0.0.0`. The Dashboard will show the proxy URL using your machine's local IP.

Use this only on trusted networks. Token Guard does not add authentication beyond the project label key.

## Log retention

Set **Log retention (days)** in Settings to automatically delete old logs and audit events. Set to `0` to keep everything forever.

## Auto-start

Enable **Start on login** in Settings to launch Token Guard automatically.

## Auto-update

A licensed copy can check GitHub Releases for updates automatically. Set the **Auto-check interval** in Settings, or click **Check for updates** to check manually.

## Backup and restore

In **Settings**, use **Backup database** to copy the SQLite file to a chosen path. Use **Restore database** to replace the current database with a backup file and restart the app.

## Request/response body logging

Token Guard logs only metadata by default. Optional body logging can be enabled for debugging. When enabled, request and response bodies are stored in the `logs` table.
