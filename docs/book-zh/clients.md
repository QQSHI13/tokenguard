# 客户端集成示例

Token Guard 适用于任何允许覆盖基础 URL 和 API 密钥的客户端。

## 环境变量

### OpenAI 兼容客户端

```bash
export OPENAI_BASE_URL=http://localhost:3742/v1
export OPENAI_API_KEY=tg_cursor_key
```

### Anthropic 客户端

```bash
export ANTHROPIC_BASE_URL=http://localhost:3742
export ANTHROPIC_API_KEY=tg_claude_code_key
```

### Google Gemini 客户端

```bash
export GEMINI_BASE_URL=http://localhost:3742/v1beta
export GEMINI_API_KEY=tg_gemini_key
```

## Python（OpenAI SDK）

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

运行 `claude` 前设置：

```bash
export ANTHROPIC_BASE_URL=http://localhost:3742
export ANTHROPIC_API_KEY=tg_claude_code_key
```

## Cursor

1. 打开 **Cursor Settings** → **Models**。
2. 将 **OpenAI API Base** 设为 `http://localhost:3742/v1`。
3. 将 **OpenAI API Key** 设为你的项目标签密钥。

## Continue.dev

添加到 `~/.continue/config.json`：

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

## Ollama / 本地模型

将任何 OpenAI 兼容的本地客户端指向：

```bash
export OPENAI_BASE_URL=http://localhost:3742/v1
export OPENAI_API_KEY=local
```

确保 Token Guard 中 Ollama 服务商的基础 URL 为 `http://localhost:11434/v1`。

## 无需更改客户端即可切换服务商

因为 Token Guard 按 `model` 路由，你可以保持客户端配置不变，只在 Token Guard UI 中更改模型所属的服务商。

例如，`gpt-4o` 当前路由到 OpenAI，你可以在服务商模型列表中将其重新分配给 Azure OpenAI，客户端无需更改。
