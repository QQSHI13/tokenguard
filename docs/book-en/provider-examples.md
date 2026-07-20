# Provider setup examples

Token Guard supports OpenAI, Anthropic, and Google Gemini request formats. The provider **format** tells Token Guard how to talk to the upstream API; the **auth scheme** tells it how to send the key.

## OpenAI

| Field | Value |
|---|---|
| Format | `openai` |
| Base URL | `https://api.openai.com` |
| Auth | `Bearer` |
| Models | `gpt-4o`, `gpt-4o-mini`, `gpt-5-chat-latest`, etc. |

Add each model you plan to use. If your agent sends `gpt-4o` but OpenAI expects a dated snapshot, set the **provider name** to `gpt-4o-2024-08-06` while keeping the **local name** as `gpt-4o`.

## Anthropic

| Field | Value |
|---|---|
| Format | `anthropic` |
| Base URL | `https://api.anthropic.com` |
| Auth | `X-API-Key` |
| Models | `claude-sonnet-4-5`, `claude-opus-4-5`, `claude-haiku-4-5`, etc. |

Token Guard automatically adds the `anthropic-version: 2023-06-01` header.

## Google Gemini

| Field | Value |
|---|---|
| Format | `google` |
| Base URL | `https://generativelanguage.googleapis.com` |
| Auth | `X-Goog-API-Key` |
| Models | `gemini-2.5-pro`, `gemini-2.5-flash`, etc. |

Gemini routes by URL path, so the model name in your request body is ignored; the path determines the model.

## DeepSeek

| Field | Value |
|---|---|
| Format | `openai` (DeepSeek is OpenAI-compatible) |
| Base URL | `https://api.deepseek.com` |
| Auth | `Bearer` |
| Models | `deepseek-chat`, `deepseek-reasoner`, `deepseek-v4-pro` |

## xAI Grok

| Field | Value |
|---|---|
| Format | `openai` |
| Base URL | `https://api.x.ai` |
| Auth | `Bearer` |
| Models | `grok-4.20-0309-reasoning`, `grok-build-0.1`, etc. |

## Mistral / Codestral

| Field | Value |
|---|---|
| Format | `openai` |
| Base URL | `https://api.mistral.ai` |
| Auth | `Bearer` |
| Models | `codestral-latest`, `mistral-large-latest`, `magistral-medium-latest`, etc. |

## Groq

| Field | Value |
|---|---|
| Format | `openai` |
| Base URL | `https://api.groq.com/openai` |
| Auth | `Bearer` |
| Models | `llama-3.3-70b-versatile`, `qwen/qwen3-32b`, etc. |

## Azure OpenAI

| Field | Value |
|---|---|
| Format | `openai` |
| Base URL | `https://<your-resource>.openai.azure.com/openai/deployments/<deployment>` |
| Auth | `Bearer` or `API-Key` |
| Models | Match your deployment names |

If Azure expects the `api-key` header, choose **API-Key** as the auth scheme.

## Local / Ollama

| Field | Value |
|---|---|
| Format | `openai` (Ollama exposes an OpenAI-compatible endpoint) |
| Base URL | `http://localhost:11434/v1` |
| Auth | `Bearer` |
| API key | any non-empty string (Ollama ignores it) |
| Models | `llama3`, `qwen2.5`, etc. |

Set input/output costs to `0` so Token Guard reports local inference as free.

## OpenRouter

| Field | Value |
|---|---|
| Format | `openai` |
| Base URL | `https://openrouter.ai/api/v1` |
| Auth | `Bearer` |
| Models | `anthropic/claude-sonnet-4`, `openai/gpt-4o`, etc. |

## Fetching the model list

If a provider supports `/v1/models`, click **Fetch /v1/models** in the Providers tab to populate models automatically. You can still edit names and costs afterward.
