# FAQ

## Why does my spend show $0.0000?

- Is the response non-streaming? Non-streaming responses include a `usage` field and are usually counted correctly.
- Is the model missing from the built-in price table? Fill in the input/output cost per 1K tokens in the provider settings.
- Is the agent actually routing through Token Guard? Check that `OPENAI_BASE_URL`, `ANTHROPIC_BASE_URL`, or `GEMINI_BASE_URL` points to `http://localhost:3742`.

## Why do I get "invalid project key"?

The API key in your agent config must be a project **label key** configured in Token Guard, not the real provider API key.

## Does Token Guard record my prompts?

No. Only metadata is logged: provider, model, token counts, estimated cost, duration, and project tag. Request/response body logging is optional and disabled by default.

## Can I use the OpenAI SDK with an Anthropic provider?

Yes. Token Guard converts between OpenAI, Anthropic, and Google Gemini request/response formats. Choose the SDK shape you prefer and configure any supported provider.

## What does the license unlock?

The free edition shows a small support banner and requires manual updates. A one-time license removes the banner, enables automatic updates, and covers two devices.
