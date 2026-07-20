# 故障排除

## "invalid or missing project key"

代理配置中的 API 密钥必须是 Token Guard 中项目的**标签密钥**，而不是真实的服务商密钥。

1. 打开 **Projects** 标签页。
2. 复制你要使用的项目的标签密钥。
3. 将其粘贴到代理中作为 `OPENAI_API_KEY`、`ANTHROPIC_API_KEY` 或 `x-api-key`。

## Dashboard 中没有请求记录

1. 检查代理的基础 URL 是否指向 Token Guard：
   - OpenAI 兼容：`http://localhost:3742/v1`
   - Anthropic：`http://localhost:3742`
   - Gemini：`http://localhost:3742/v1beta`
2. 检查 Token Guard 是否正在运行，托盘图标是否可见。
3. 先发送一个非流式请求；流式请求会在流结束后才记录。

## 成本显示为 $0.0000

1. 打开服务商设置，填写该模型的**输入 $/1K** 和**输出 $/1K**。
2. 如果模型在内置的 `pricing.json` 表中，成本会自动填充。未知模型默认 $0。
3. 检查响应是否包含 `usage` 字段。某些代理服务商可能会去掉 usage。

## "no API key stored for provider"

在 **Providers** 标签页中添加服务商密钥。密钥存储在系统钥匙串中，而不是数据库。

## 钥匙串测试失败

- **Windows：** 确保凭据管理器正在运行，且当前环境没有阻止 Win32 凭据 API 的沙盒。
- **macOS：** 钥匙串访问必须可用，且应用拥有钥匙串权限。
- **Linux/WSL：** 安装 secret-service 提供程序，例如 `gnome-keyring-daemon` 或开启 secret-service 集成的 KeePassXC。

## 服务商返回 429 / 5xx

Token Guard 会对临时故障进行指数退避重试。如果配置了**回退服务商**，重试失败后还会尝试回退一次。

## 托盘图标为橙色

代理已暂停。左键点击托盘图标恢复，或在托盘菜单中点击 **Resume proxy**。

## 托盘图标为红色或黄色

有限额已超出上限（红色）或达到警告阈值（黄色）。打开 **Limits** 标签页查看详情。

## 内存或 CPU 占用高

Token Guard 在内存中保留请求计数器和一个小型 SQLite 连接池。如果发现占用过高：

1. 检查 **Logs** 标签页是否有卡住或耗时极长的请求。
2. 在 Settings 中降低 **Log retention days**，保持数据库体积较小。
3. 重启应用。

## 其他设备无法访问 Token Guard

默认情况下代理绑定到 `127.0.0.1`。在 Settings 中启用 **Expose proxy to LAN** 并重启应用，代理将绑定到 `0.0.0.0`，Dashboard 会显示本机局域网 IP。

只在受信任的网络上启用此功能。
