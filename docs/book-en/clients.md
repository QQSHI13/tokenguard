# Client integration examples

Token Guard works with any client that lets you override the base URL and API key.

## Environment variables

### OpenAI-compatible clients

```bash
export OPENAI_BASE_URL=http://localhost:3742/v1
export OPENAI_API_KEY=tg_cursor_key
```

### Anthropic clients

```bash
export ANTHROPIC_BASE_URL=http://localhost:3742
export ANTHROPIC_API_KEY=tg_claude_code_key
```

### Google Gemini clients

```bash
export GEMINI_BASE_URL=http://localhost:3742/v1beta
export GEMINI_API_KEY=tg_gemini_key
```

## Python (OpenAI SDK)

```python
import openai

client = openai.OpenAI(
    base_url="http://localhost:3742/v1",
    api_key="tg_my_project_key",
)

response = client.chat.completions.create(
    model="gpt-4o",
    messages=[{"role": "user", "content": "hello"}],
)
print(response.choices[0].message.content)
```

## curl

```bash
curl http://localhost:3742/v1/chat/completions \
  -H "Authorization: Bearer tg_my_project_key" \
  -H "Content-Type: application/json" \
  -d '{"model":"gpt-4o","messages":[{"role":"user","content":"hello"}]}'
```

## Claude Code

Set before running `claude`:

```bash
export ANTHROPIC_BASE_URL=http://localhost:3742
export ANTHROPIC_API_KEY=tg_claude_code_key
```

## Cursor

1. Open **Cursor Settings** → **Models**.
2. Set **OpenAI API Base** to `http://localhost:3742/v1`.
3. Set **OpenAI API Key** to your project label key.

## Continue.dev

Add to `~/.continue/config.json`:

```json
{
  "models": [
    {
      "title": "Token Guard",
      "provider": "openai",
      "model": "gpt-4o",
      "apiBase": "http://localhost:3742/v1",
      "apiKey": "tg_my_project_key"
    }
  ]
}
```

## Ollama / local models

Point any OpenAI-compatible local client to:

```bash
export OPENAI_BASE_URL=http://localhost:3742/v1
export OPENAI_API_KEY=local
```

Make sure your Token Guard provider for Ollama uses `http://localhost:11434/v1` as its base URL.

## Switching providers without changing the client

Because Token Guard routes by `model`, you can keep the same client config and just change which provider owns the model in the Token Guard UI.

For example, if `gpt-4o` is currently routed to OpenAI, you can reassign it to Azure OpenAI by editing the provider model list. The client does not need to change.
