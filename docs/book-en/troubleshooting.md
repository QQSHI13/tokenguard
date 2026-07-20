# Troubleshooting

## "invalid or missing project key"

The API key in your agent config must be a project **label key** from Token Guard, not the real provider key.

1. Open the **Projects** tab.
2. Copy the label key for the project you want to use.
3. Paste it into your agent as `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, or `x-api-key`.

## Requests are not showing up in the Dashboard

1. Check that the base URL in your agent points to Token Guard:
   - OpenAI-compatible: `http://localhost:3742/v1`
   - Anthropic: `http://localhost:3742`
   - Gemini: `http://localhost:3742/v1beta`
2. Check that Token Guard is running and the tray icon is visible.
3. Send a non-streaming request first; streaming requests are logged after the stream ends.

## Cost shows $0.0000

1. Open the provider and fill in **input $/1K** and **output $/1K** for the model.
2. If the model is in the built-in `pricing.json` table, costs are filled automatically. Unknown models default to $0.
3. Check that the response includes a `usage` field. Some proxy providers strip usage.

## "no API key stored for provider"

Add the provider key in the **Providers** tab. Keys are stored in the OS keychain, not in the database.

## Keychain test fails

- **Windows:** make sure Credential Manager is running and you are not in a sandbox that blocks Win32 credential APIs.
- **macOS:** Keychain Access must be available and the app must have keychain entitlements.
- **Linux/WSL:** install a secret-service provider such as `gnome-keyring-daemon` or KeePassXC with secret-service integration.

## Provider returns 429 / 5xx

Token Guard retries transient failures with exponential backoff. If a **fallback provider** is configured, it tries that once after the retries fail.

## Tray icon is orange

The proxy is paused. Left-click the tray icon to resume, or click **Resume proxy** in the tray menu.

## Tray icon is red or yellow

A limit has crossed its cap (red) or warning threshold (yellow). Open the **Limits** tab to see which one.

## High memory or CPU

Token Guard keeps request counters and a small SQLite pool in memory. If you see high usage:

1. Check the **Logs** tab for stuck or extremely long requests.
2. Lower **log retention days** in Settings to keep the database small.
3. Restart the app.

## Can't reach Token Guard from another device

By default the proxy binds to `127.0.0.1`. Enable **Expose proxy to LAN** in Settings and restart the app. The proxy URL will switch to your machine's local IP.

Only enable this on trusted networks.
