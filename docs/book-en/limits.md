# Limits & subscriptions

Limits are local counters that help you stay inside your budget or subscription plan. Token Guard can cap:

- **Money** ($)
- **Tokens** (prompt + completion)
- **Input tokens** only
- **Output tokens** only
- **Cost per request** (a single request can never cost more than X)
- **Concurrent requests** (how many requests can be in flight at once)
- **Requests** (count)
- **Requests per minute** (rate)
- **Tokens per minute** (rate)
- **Time** (wall-clock seconds)

Each limit has:

- A **period** — one-time, hourly, daily, weekly, monthly, calendar week/month, or custom seconds. (Per-request metrics — cost per request, concurrent requests — ignore the period: they measure a single request, not a time window.)
- A **scope** — global, per provider, per project, or per model pattern.
- An **action** — warn, block, or pause the proxy.
- An optional **schedule** — only active during certain hours and days.
- Optionally, membership in a **limit group** so several limits can share one cap.

## Subscription-style plans

If your provider plan includes "1 M tokens per month" or "$100 per month", create a matching limit with **Monthly** period and the right cap. Token Guard will warn or stop you before you exceed it.

## Provider-style policies

Provider APIs enforce policies like "max 60 requests per minute", "max 10 concurrent requests", or "max 200K output tokens per request". Recreate those policies locally:

- `requests_per_minute` → the RPM your provider allows.
- `concurrent_requests` → the provider's max in-flight requests. Requests beyond the cap are blocked with HTTP 429 before they reach the provider.
- `output_tokens` → cap output per period (e.g. Gemini/Anthropic output quotas).
- `cost_per_request` → a hard ceiling on what any single request may cost.

## Pausing the proxy

A limit with action **Pause** will flip the proxy into a paused state when hit. All new requests are rejected until you resume from the tray icon or the Dashboard.
