# 服务商配置示例

Token Guard 支持 OpenAI、Anthropic 和 Google Gemini 三种请求格式。服务商的**格式**决定 Token Guard 如何与上游 API 通信；**认证方式**决定如何发送密钥。

## OpenAI

| 字段 | 值 |
|---|---|
| 格式 | `openai` |
| 基础 URL | `https://api.openai.com` |
| 认证 | `Bearer` |
| 模型 | `gpt-4o`、`gpt-4o-mini`、`gpt-5-chat-latest` 等 |

添加你计划使用的每个模型。如果你的代理发送 `gpt-4o`，而 OpenAI 期望 dated snapshot，可将**本地名称**设为 `gpt-4o`，**服务商名称**设为 `gpt-4o-2024-08-06`。

## Anthropic

| 字段 | 值 |
|---|---|
| 格式 | `anthropic` |
| 基础 URL | `https://api.anthropic.com` |
| 认证 | `X-API-Key` |
| 模型 | `claude-sonnet-4-5`、`claude-opus-4-5`、`claude-haiku-4-5` 等 |

Token Guard 会自动添加 `anthropic-version: 2023-06-01` 请求头。

## Google Gemini

| 字段 | 值 |
|---|---|
| 格式 | `google` |
| 基础 URL | `https://generativelanguage.googleapis.com` |
| 认证 | `X-Goog-API-Key` |
| 模型 | `gemini-2.5-pro`、`gemini-2.5-flash` 等 |

Gemini 通过 URL 路径路由模型，因此请求体中的模型名称会被忽略；路径决定实际模型。

## DeepSeek

| 字段 | 值 |
|---|---|
| 格式 | `openai`（DeepSeek 兼容 OpenAI） |
| 基础 URL | `https://api.deepseek.com` |
| 认证 | `Bearer` |
| 模型 | `deepseek-chat`、`deepseek-reasoner`、`deepseek-v4-pro` |

## xAI Grok

| 字段 | 值 |
|---|---|
| 格式 | `openai` |
| 基础 URL | `https://api.x.ai` |
| 认证 | `Bearer` |
| 模型 | `grok-4.20-0309-reasoning`、`grok-build-0.1` 等 |

## Mistral / Codestral

| 字段 | 值 |
|---|---|
| 格式 | `openai` |
| 基础 URL | `https://api.mistral.ai` |
| 认证 | `Bearer` |
| 模型 | `codestral-latest`、`mistral-large-latest`、`magistral-medium-latest` 等 |

## Groq

| 字段 | 值 |
|---|---|
| 格式 | `openai` |
| 基础 URL | `https://api.groq.com/openai` |
| 认证 | `Bearer` |
| 模型 | `llama-3.3-70b-versatile`、`qwen/qwen3-32b` 等 |

## Azure OpenAI

| 字段 | 值 |
|---|---|
| 格式 | `openai` |
| 基础 URL | `https://<your-resource>.openai.azure.com/openai/deployments/<deployment>` |
| 认证 | `Bearer` 或 `API-Key` |
| 模型 | 与部署名称一致 |

如果 Azure 期望 `api-key` 请求头，认证方式请选择 **API-Key**。

## 本地 / Ollama

| 字段 | 值 |
|---|---|
| 格式 | `openai`（Ollama 提供 OpenAI 兼容端点） |
| 基础 URL | `http://localhost:11434/v1` |
| 认证 | `Bearer` |
| API 密钥 | 任意非空字符串（Ollama 会忽略） |
| 模型 | `llama3`、`qwen2.5` 等 |

将输入/输出成本设为 `0`，Token Guard 会将本地推理费用报告为 0。

## OpenRouter

| 字段 | 值 |
|---|---|
| 格式 | `openai` |
| 基础 URL | `https://openrouter.ai/api/v1` |
| 认证 | `Bearer` |
| 模型 | `anthropic/claude-sonnet-4`、`openai/gpt-4o` 等 |

## 自动获取模型列表

如果服务商支持 `/v1/models`，点击 Providers 标签页的 **Fetch /v1/models** 可自动填充模型。之后仍可编辑名称和成本。
