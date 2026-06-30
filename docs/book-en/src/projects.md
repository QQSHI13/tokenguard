# Projects & API keys

A **project** is just a label. It lets you separate spend and usage by source — for example "Cursor", "Claude Code", or "production bot".

The **label key** is the value you set as the API key in your agent. Token Guard maps that label to the project name, then swaps in the real provider key from your keychain before forwarding the request.

## Security

- The label key can be any string. It is not secret.
- The real API key never appears in your agent config.
- If no configured project matches the incoming key, Token Guard rejects the request with HTTP 401.
