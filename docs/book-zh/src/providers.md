# 服务商与模型别名

**服务商**是上游 LLM API 端点。Token Guard 支持 OpenAI、Anthropic 和 Google Gemini 三种请求格式。

## 3 × 3 转换

你可以用任意一种 SDK 形状调用 Token Guard。Token Guard 会把请求转换为服务商的原生格式，并把响应转换回来：

- OpenAI SDK → Anthropic 服务商
- Anthropic SDK → OpenAI 服务商
- OpenAI/Anthropic SDK → Google Gemini 服务商
- 以及其它任意组合

文本、图片、工具、`stop`、`top_p`、`max_tokens`、`temperature` 和流式标志在结构兼容时都会被转换。

## 模型别名

每个服务商模型有两个名称：

- **本地名称**——你在代理中发送的名称（如 `gpt-4o`、`claude-sonnet-4`、`gemini-1.5-pro`）。
- **服务商名称**——上游 API 期望的模型 ID（如 `gpt-4o-2024-08-06`、`claude-sonnet-4-20250514`）。

别名让你保持代理配置简洁，同时 Token Guard 会转发服务商需要的精确模型 ID。

## 备用服务商

每个服务商可以指定一个备用服务商。如果主服务商返回 5xx/429 或网络错误，Token Guard 会先重试几次，然后尝试备用一次。备用服务商可以使用不同格式——转换会自动应用。

## 费用覆盖

服务商定价经常变化。为了精确追踪花费，请为每个服务商设置准确的**输入 $/1K 词元**和**输出 $/1K 词元**。如果留空，Token Guard 会回退到内置价格表。
