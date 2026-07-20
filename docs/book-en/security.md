# Security & privacy

## What leaves your machine

Only the requests you choose to send to your configured providers. Token Guard forwards bytes to the upstream API and records metadata locally.

## What is logged

The `logs` table stores:

- timestamp
- provider name
- model name
- prompt tokens
- completion tokens
- estimated cost
- duration
- project tag
- HTTP status

No prompts, completions, embeddings, or other request/response bodies are logged unless you explicitly enable body logging.

## Where API keys live

Provider API keys are stored in the OS keychain:

- Windows: Credential Manager
- macOS: Keychain
- Linux: D-Bus Secret Service (GNOME Keyring, KeePassXC, KWallet)

Keys are never written to the SQLite database, `settings.json`, or any config file.

## Project label keys

The value you set as `OPENAI_API_KEY` in your agent is a **label key**, not a secret. It only maps the request to a project. If someone sees it, they cannot access your provider account.

If a request arrives with a label key that does not match any project, Token Guard rejects it with HTTP 401 before forwarding anything.

## Network binding

By default the proxy binds to `127.0.0.1`, so it is unreachable from other devices on the network. LAN exposure is opt-in.

## Updates

Token Guard checks GitHub Releases for updates. No usage data, prompts, or telemetry are sent during an update check.

## License validation

License keys are validated against a Cloudflare Worker. The worker receives the key and a device fingerprint; it does not receive prompts, usage, or provider keys.
