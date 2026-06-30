# Privacy Policy

**Effective date:** 2026-06-30

**Token Guard** is a local desktop application. This policy explains what data we
do and do not handle.

## What Token Guard does

Token Guard runs a small HTTP proxy on your own computer (`127.0.0.1`) and logs
metadata about the LLM API calls you make — such as the provider name, model
name, token counts, estimated cost, and timestamp — to a local SQLite database
on your device.

## What we do not collect

- **No account.** You do not create a Token Guard account.
- **No cloud service.** There is no Token Guard server that receives your data.
- **No telemetry or analytics.** We do not track usage, crashes, or performance.
- **No prompts or completions.** The content of your requests and responses is
  streamed through the proxy and is never written to disk or transmitted to us.
- **No API keys in the app.** Your provider API keys are stored in the OS-native
  secret store (Windows Credential Manager, macOS Keychain, or Linux Secret
  Service), not in Token Guard files or our code.

## Data that stays on your device

All information Token Guard records is stored locally in your user profile:

- App data directory: SQLite database and settings.
- OS keychain: provider API keys.

You can delete this data at any time by using the in-app delete option or by
removing the app data directory.

## Network connections

Token Guard makes only the network connections you explicitly configure:

1. **Your chosen LLM provider** — the proxy forwards your requests to the
   provider endpoints you add.
2. **Paid edition updater** — if you use a paid direct-download build, the app
   may check the update endpoint listed in your private runtime config. No
   personal data is sent.
3. **Microsoft Store license check** — Store-installed builds may contact
   Microsoft’s licensing service as part of normal Store operation.

## Contact

If you have questions about this policy, open an issue in the GitHub repository
or contact the maintainer listed in the repository profile.
