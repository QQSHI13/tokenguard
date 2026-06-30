# Quick start

1. **Add a provider** in the **Routing** tab.
   - Enter the provider name, base URL, and format (OpenAI or Anthropic).
   - Paste your real API key — it is stored in the OS keychain.
2. **Create a project** in the **Caps** tab.
   - Give it a name and a throwaway **label key**.
   - This label key is what you put in your agent config as `OPENAI_API_KEY` or `ANTHROPIC_API_KEY`.
3. **Point your agent at Token Guard.**
   ```bash
   OPENAI_BASE_URL=http://localhost:3742/v1
   OPENAI_API_KEY=<your-project-label-key>
   ```
   For Anthropic clients:
   ```bash
   ANTHROPIC_BASE_URL=http://localhost:3742
   ANTHROPIC_API_KEY=<your-project-label-key>
   ```
4. **Send one request.** The Token Guard Dashboard will show the spend, tokens, and model used.
