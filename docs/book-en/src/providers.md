# Providers & model aliases

A **provider** is an upstream LLM API endpoint. Token Guard supports the OpenAI and Anthropic request formats.

## Model aliases

Each provider model has two names:

- **Local name** — what you send in your agent (`gpt-4o`, `claude-sonnet-4`, etc.).
- **Provider name** — what the upstream API expects (`gpt-4o-2024-08-06`, `claude-sonnet-4-20250514`, etc.).

Aliases let you keep your agent config clean while Token Guard forwards the exact model ID the provider needs.

## Cost overrides

Provider pricing changes often. For accurate spend tracking, set the exact **input $/1K tokens** and **output $/1K tokens** for each provider. If you leave them blank, Token Guard falls back to a small built-in price table.
