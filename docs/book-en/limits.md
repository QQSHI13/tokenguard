# Limits & subscriptions

Limits are local counters that help you stay inside your budget or subscription plan. Token Guard can cap:

- **Money** ($)
- **Tokens** (prompt + completion)
- **Requests** (count)
- **Requests per minute** (rate)
- **Time** (wall-clock seconds)

Each limit has:

- A **period** — one-time, hourly, daily, weekly, monthly, or custom seconds.
- A **scope** — global, per provider, or per project.
- An **action** — warn, block, or pause the proxy.

## Subscription-style plans

If your provider plan includes "1 M tokens per month" or "$100 per month", create a matching limit with **Monthly** period and the right cap. Token Guard will warn or stop you before you exceed it.

## Pausing the proxy

A limit with action **Pause** will flip the proxy into a paused state when hit. All new requests are rejected until you resume from the tray icon or the Dashboard.
