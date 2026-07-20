# 高级功能

## 回退服务商

在服务商表单中选择另一个作为回退。当主服务商返回 5xx、429 或网络错误时，Token Guard 会将同一请求转发到回退服务商。格式转换会自动应用，因此 OpenAI 格式服务商可以回退到 Anthropic 格式服务商。

## 自动导出

Token Guard 可以定期将所有请求日志导出为 CSV。

1. 在 **Settings** 中设置 **Auto-export folder**。
2. 设置 **Auto-export interval (days)**。
3. 如果间隔已过，下次启动时会自动运行导出。

CSV 包含：时间戳、服务商、模型、提示令牌、补全令牌、成本、项目标签。

## Webhook 通知

当限额被触发或达到警告阈值时，Token Guard 可以向 Webhook URL 发送 POST。Payload 包含限额名称、指标、已用值和上限。

在 **Settings** 中设置 Webhook URL，可接入 Slack、Discord、n8n 或你自己的服务。

## 局域网暴露

默认情况下代理监听 `127.0.0.1:3742`。在 Settings 中启用 **Expose proxy to LAN** 可绑定到 `0.0.0.0`，Dashboard 会显示本机局域网 IP 作为代理地址。

只在受信任的网络上启用。Token Guard 除了项目标签密钥外不提供额外认证。

## 日志保留

在 Settings 中设置 **Log retention (days)**，自动删除旧日志和审计事件。设为 `0` 表示永久保留。

## 自动启动

在 Settings 中启用 **Start on login**，登录时自动启动 Token Guard。

## 自动更新

授权版本可以自动检查 GitHub Releases 更新。在 Settings 中设置 **Auto-check interval**，或点击 **Check for updates** 手动检查。

## 备份与恢复

在 **Settings** 中使用 **Backup database** 将 SQLite 文件复制到指定路径。使用 **Restore database** 用备份文件替换当前数据库并重启应用。

## 请求/响应体日志

默认情况下 Token Guard 只记录元数据。调试时可启用请求/响应体日志，此时请求体和响应体会存储在 `logs` 表中。
