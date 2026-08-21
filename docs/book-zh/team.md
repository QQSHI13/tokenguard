# 通过 Tailscale 与团队共享

团队中的一名成员托管 Token Guard，其他成员将自己的 AI 客户端指向主机的 Tailscale 地址。无需云账户、无需按席位付费——网关、你的密钥和日志都保留在托管者的机器上。

## 为什么选择 Tailscale？

Tailscale 会在你的设备之间创建加密的私有网络（*tailnet*）。Token Guard **无法**从局域网或互联网访问——它要么只绑定到主机的 Tailscale IP 和回环地址，要么（见下文）留在回环地址后面由 `tailscale serve` 转发。每位团队成员都需要安装 Tailscale 并登录到同一 tailnet 才能访问网关。

## 两种暴露模式

启用共享时，Token Guard 会自动选择模式：

- **直连**——常规情况。网关绑定到主机的 Tailscale IP（`100.x.x.x`）和回环地址。团队成员使用 `http://<tailscale-ip>:3742/v1`。
- **Serve**——当 Tailscale 以*用户空间网络*模式运行（WSL 中常见）、没有 `100.x` 接口时的回退方案。网关保持在回环地址上，由一条 `tailscale serve` 路由通过 `/tg` 路径暴露：`https://<host>.ts.net/tg/v1`。路径前缀让这条路由与同一主机上的其他服务互不冲突，且只有 tailnet 内的设备可以访问。

`tokenguard share status` 会显示当前使用的模式。

## 设置主机

1. 在主机上安装并登录 [Tailscale](https://tailscale.com)。
2. 启动 Token Guard（GUI 或 `tokenguard start`）。
3. 启用共享：

   **CLI**
   ```bash
   tokenguard share on
   ```

   **GUI** —— 设置 → *通过 Tailscale 与团队共享*。

   命令会打印团队端点，例如 `http://100.100.100.5:3742/v1`（直连模式）或 `https://my-host.tail1234.ts.net/tg/v1`（serve 模式）。

4. 重启应用，让网关绑定到 tailnet 地址（仅直连模式需要——serve 模式无需重启）。

## 连接团队成员

每位团队成员需要：

1. 安装 Tailscale 并登录到同一 tailnet。
2. 一个在主机上创建的**项目标签密钥**（`tg_...`）——在 GUI 的项目页或通过 `tokenguard project add` 创建。标签密钥用于认证请求并标记到项目。

然后将任何客户端指向团队端点：

```bash
OPENAI_BASE_URL=http://100.100.100.5:3742/v1
OPENAI_API_KEY=tg_team-project
```

Anthropic 和 Gemini SDK 同样可用：

```bash
ANTHROPIC_BASE_URL=http://100.100.100.5:3742
ANTHROPIC_API_KEY=tg_team-project
```

```bash
GEMINI_API_KEY=tg_team-project
# base URL: http://100.100.100.5:3742/v1beta
```

## 团队成员能做什么、不能做什么

- **能**使用所有已配置的服务商（通过 4 × 4 转换）——发送 OpenAI 格式请求的团队成员可以调用 Anthropic 或 Gemini 模型。
- **能被跟踪**：请求会标记到其所用标签密钥对应的项目，并计入该项目的预算和限额。
- **不能**看到其他项目的标签密钥、你的真实服务商密钥，或他们没有密钥的项目的使用历史。
- **不能**修改设置——团队成员对网关是只读的。

## 控制访问

- 为每位成员（或每个团队）创建一个项目，这样你可以通过删除某个项目来单独撤销某个人的访问。
- 设置按项目的预算和限额——失控的成员只会触及自己的上限，而不是整个网关。
- `tokenguard share off`（或 GUI 开关）可以一次性断开所有人。

## CLI 参考

```bash
tokenguard share on      # 启用并打印团队端点
tokenguard share off     # 禁用（仅回环，并移除 serve 路由）
tokenguard share status  # 显示状态、模式和团队端点
tokenguard settings set-share-tailscale true   # 等同于 `share on`
```