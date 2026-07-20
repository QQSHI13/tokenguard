# 如何使用 Token Guard

本指南带你完成从零开始的典型设置。

## 1. 安装并启动

从 GitHub 下载适合你平台的最新版本。首次启动时：

- 代理在 `http://localhost:3742` 启动。
- 绿色盾牌托盘图标出现。
- Dashboard 窗口打开。

## 2. 添加第一个服务商

1. 打开 **Config** 标签页。
2. 点击 **Add provider**。
3. 填写：
   - **名称：** 例如 `openai`
   - **基础 URL：** 例如 `https://api.openai.com`
   - **格式：** `openai`
   - **认证头：** `Bearer`
   - **API 密钥：** 你的真实 OpenAI 密钥
4. 至少添加一个模型，例如本地名称 `gpt-4o`，服务商名称 `gpt-4o-2024-08-06`。
5. 保存。

API 密钥存储在系统钥匙串中。Token Guard 不会将其保存到磁盘。

## 3. 创建项目

1. 在 **Config** 标签页中点击 **Projects**。
2. 点击 **Add project**。
3. 输入名称，例如 `cursor`。
4. 生成或输入标签密钥，例如 `tg_cursor_key`。

这个标签密钥就是粘贴到编辑器或代理中的值。它不是秘密。

## 4. 将客户端指向 Token Guard

在启动代理前设置环境变量：

```bash
export OPENAI_BASE_URL=http://localhost:3742/v1
export OPENAI_API_KEY=tg_cursor_key
```

或在 Cursor、Claude Code、Continue.dev 或任何 OpenAI 兼容客户端中配置相应设置。

## 5. 发送测试请求

运行一次聊天补全，或向你的编码代理提问。Dashboard 应显示：

- 服务商
- 模型
- 提示/补全令牌数
- 估算成本
- 项目标签

## 6. 设置限额

1. 进入 **Limits** 标签页。
2. 点击 **Add limit**。
3. 选择预设或自定义。
4. 选择动作：警告、阻止或暂停。

当限额超过阈值时，托盘图标会变成黄色或红色。

## 7. 监控用量

**Dashboard** 显示：

- 今日支出
- 最近请求
- 每日和每月用量图表
- 按成本排名的项目

使用日期范围和筛选器查看特定服务商或项目。

## 8. 暂停与恢复

- 左键点击托盘图标切换暂停/恢复。
- 暂停时，新请求收到 HTTP 503。
- 可随时从托盘或 Dashboard 头部恢复。

## 9. 更新价格

如果某个模型的成本缺失或错误，编辑服务商并设置**输入 $/1K**、**输出 $/1K** 和可选的**缓存输入 $/1K**。Token Guard 会使用这些覆盖值替代内置表。

你也可以向 GitHub 仓库的 `pricing.json` 贡献更新后的价格。
