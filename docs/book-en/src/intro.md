> The local LLM gateway. Your keys, your machine, your tokens.

Token Guard sits between your coding agent and the LLM APIs you already use. It logs metadata (tokens, model, cost) to a local SQLite database and shows real-time spend in your system tray.

**3 × 3 format conversion.** Token Guard accepts requests in the OpenAI, Anthropic, or Google Gemini SDK shape and forwards them to any configured provider, converting requests, responses, and streaming chunks as needed. You can keep using the SDK you already have with the provider you want.

**Nothing leaves your machine except the API calls you choose to send.** Token Guard has no cloud account, no telemetry, and no external billing service. API keys are stored in your OS keychain, not on disk.
