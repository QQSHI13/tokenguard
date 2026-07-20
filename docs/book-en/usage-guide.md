# How to use Token Guard

This guide walks through a typical setup from scratch.

## 1. Install and launch

Download the latest release for your platform from GitHub. On first launch:

- The proxy starts on `http://localhost:3742`.
- A green shield tray icon appears.
- The Dashboard window opens.

## 2. Add your first provider

1. Open the **Config** tab.
2. Click **Add provider**.
3. Fill in:
   - **Name:** e.g. `openai`
   - **Base URL:** e.g. `https://api.openai.com`
   - **Format:** `openai`
   - **Auth header:** `Bearer`
   - **API key:** your real OpenAI key
4. Add at least one model, e.g. local name `gpt-4o`, provider name `gpt-4o-2024-08-06`.
5. Save.

The API key is stored in your OS keychain. Token Guard never saves it to disk.

## 3. Create a project

1. In the **Config** tab, click **Projects**.
2. Click **Add project**.
3. Give it a name, e.g. `cursor`.
4. Generate or type a label key, e.g. `tg_cursor_key`.

This label key is what you paste into your editor or agent. It is not secret.

## 4. Point your client at Token Guard

Set the environment variables before launching your agent:

```bash
export OPENAI_BASE_URL=http://localhost:3742/v1
export OPENAI_API_KEY=tg_cursor_key
```

Or configure the equivalent settings in Cursor, Claude Code, Continue.dev, or any OpenAI-compatible client.

## 5. Send a test request

Run a chat completion or ask your coding agent a question. The Dashboard should show:

- provider
- model
- prompt/completion tokens
- estimated cost
- project tag

## 6. Set a limit

1. Go to the **Limits** tab.
2. Click **Add limit**.
3. Pick a preset or build your own.
4. Choose the action: warn, block, or pause.

The tray icon will turn yellow or red when a limit crosses its threshold.

## 7. Monitor usage

The **Dashboard** shows:

- today's spend
- recent requests
- daily and monthly usage charts
- top projects by cost

Use the date range and filters to drill into specific providers or projects.

## 8. Pause and resume

- Left-click the tray icon to toggle pause/resume.
- When paused, new requests receive HTTP 503.
- Resume at any time from the tray or the Dashboard header.

## 9. Update pricing

If a model's cost is missing or wrong, edit the provider and set **input $/1K**, **output $/1K**, and optionally **cached input $/1K**. Token Guard uses these overrides instead of the built-in table.

You can also contribute updated prices to `pricing.json` in the GitHub repo.
