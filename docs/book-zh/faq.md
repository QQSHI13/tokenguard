# 常见问题

## 为什么花费显示 $0.0000？

- 请求是非流式的吗？非流式响应会返回 `usage` 字段，通常能正常统计。
- 模型不在内置价格表中？请在服务商设置中填写输入/输出价格。
- 代理没有真正走 Token Guard？检查 `OPENAI_BASE_URL`、`ANTHROPIC_BASE_URL` 或 `GEMINI_BASE_URL` 是否指向 `http://localhost:3742`。

## 为什么提示"invalid project key"？

你使用的 API 密钥必须是一个已在 Token Guard 中配置的项目标签密钥，而不是真实的服务商密钥。

## Token Guard 会记录我的提示内容吗？

不会。它默认只记录元数据：服务商、模型、词元数、费用、持续时间和项目标签。请求/响应体记录是可选的，默认关闭。

## 我能用 OpenAI SDK 调用 Anthropic 服务商吗？

可以。Token Guard 会在 OpenAI、Anthropic 和 Google Gemini 的请求/响应格式之间转换。选择你喜欢的 SDK 形状，配置任意支持的服务商即可。

## 许可证解锁了什么？

免费版会显示一个小的支持横幅，并需要手动更新。一次性许可证可以移除横幅、启用自动更新，并支持两台设备。
