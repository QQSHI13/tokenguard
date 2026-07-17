# Providers & model aliases

A **provider** is an upstream LLM API endpoint. Token Guard supports OpenAI, Anthropic, and Google Gemini request formats.

## 3 × 3 conversion

You can call Token Guard using any of the three SDK shapes. Token Guard will convert the request to the provider’s native format and convert the response back:

- OpenAI SDK → Anthropic provider
- Anthropic SDK → OpenAI provider
- OpenAI/Anthropic SDK → Google Gemini provider
- and any other combination

Text, images, tools, `stop`, `top_p`, `max_tokens`, `temperature`, and streaming flags are converted when structurally compatible.

## Model aliases

Each provider model has two names:

- **Local name** — what you send in your agent (`gpt-4o`, `claude-sonnet-4`, `gemini-1.5-pro`, etc.).
- **Provider name** — what the upstream API expects (`gpt-4o-2024-08-06`, `claude-sonnet-4-20250514`, etc.).

Aliases let you keep your agent config clean while Token Guard forwards the exact model ID the provider needs.

## Fallback provider

Each provider can optionally specify a fallback provider. If the primary provider returns a 5xx/429 or a network error, Token Guard retries a few times and then tries the fallback once. The fallback can use a different format — conversion is applied automatically.

## Cost overrides

Provider pricing changes often. For accurate spend tracking, set the exact **input $/1K tokens** and **output $/1K tokens** for each provider. If you leave them blank, Token Guard falls back to a small built-in price table.
