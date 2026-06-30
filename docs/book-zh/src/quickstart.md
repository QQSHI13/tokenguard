# 快速开始

1. 在 **Routing（路由）** 标签中添加一个服务商。
   - 输入服务商名称、基础 URL 和格式（OpenAI 或 Anthropic）。
   - 粘贴你的真实 API 密钥——它会保存在系统钥匙串中。
2. 在 **Caps（限额）** 标签中创建一个项目。
   - 给它一个名称和一个一次性的**标签密钥**。
   - 这个标签密钥就是你在代理配置中填写的 `OPENAI_API_KEY` 或 `ANTHROPIC_API_KEY`。
3. 将代理指向 Token Guard。
   ```bash
   OPENAI_BASE_URL=http://localhost:3742/v1
   OPENAI_API_KEY=<你的项目标签密钥>
   ```
   Anthropic 客户端：
   ```bash
   ANTHROPIC_BASE_URL=http://localhost:3742
   ANTHROPIC_API_KEY=<你的项目标签密钥>
   ```
4. 发送一个请求。Token Guard 的 Dashboard 会显示花费、词元和模型。
