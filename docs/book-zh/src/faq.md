# 常见问题

## 为什么花费显示 $0.0000？

- 请求是非流式的吗？非流式响应会返回 `usage` 字段，通常能正常统计。
- 模型不在内置价格表中？请在服务商设置中填写输入/输出价格。
- 代理没有真正走 Token Guard？检查 `OPENAI_BASE_URL` 或 `ANTHROPIC_BASE_URL` 是否指向 `http://localhost:3742`。

## 为什么提示"invalid project key"？

你使用的 API 密钥必须是一个已在 Token Guard 中配置的项目标签密钥，而不是真实的服务商密钥。

## Token Guard 会记录我的提示内容吗？

不会。它只记录元数据：服务商、模型、词元数、费用、持续时间和项目标签。
