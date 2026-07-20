# Limit recipes

Limits are local counters that warn, block, or pause traffic before a cap is exceeded. All periods are measured in UTC.

## Daily budget

Stop accidental runaway spending.

| Field | Value |
|---|---|
| Metric | Money |
| Cap | `50` |
| Period | Daily |
| Scope | Global |
| Action | Block |
| Warning at | `0.8` |

Result: Token Guard returns HTTP 429 once today’s spend reaches $50.

## Monthly token allowance

Mimic a provider plan that includes 1 M tokens per month.

| Field | Value |
|---|---|
| Metric | Tokens |
| Cap | `1000000` |
| Period | Monthly |
| Scope | Global |
| Action | Warn |
| Warning at | `0.9` |

Result: a tray notification when you cross 900 K tokens; requests keep flowing.

## Hourly request cap

Protect a shared key or a rate-limited endpoint.

| Field | Value |
|---|---|
| Metric | Requests |
| Cap | `100` |
| Period | Hourly |
| Scope | Provider |
| Scope ID | your provider |
| Action | Block |

Result: once 100 requests have gone to that provider this hour, new ones get 429.

## RPM limit

Prevent a burst from tripping the provider’s own rate limit.

| Field | Value |
|---|---|
| Metric | Requests per minute |
| Cap | `60` |
| Period | — (RPM always uses a 60-second rolling window) |
| Scope | Global |
| Action | Block |

Result: no more than 60 requests in any 60-second window.

## Time cap

Track wall-clock time for subscription-style plans such as "5 hours per day".

| Field | Value |
|---|---|
| Metric | Time (seconds) |
| Cap | `18000` (5 hours) |
| Period | Daily |
| Scope | Global |
| Action | Pause |

Result: when cumulative request duration hits 5 hours, the proxy pauses until you resume it.

## Project-specific budget

Keep one project from consuming the whole team budget.

| Field | Value |
|---|---|
| Metric | Money |
| Cap | `20` |
| Period | Daily |
| Scope | Project |
| Scope ID | your project |
| Action | Block |

Result: requests tagged with that project are blocked after $20 today.

## Scheduled limits

Use **active hours** and **active days** to make a limit apply only during work hours or weekdays. Times are in UTC.

Example: block more than $100 spend on weekdays between 09:00 and 17:00 UTC.

| Field | Value |
|---|---|
| Metric | Money |
| Cap | `100` |
| Period | Daily |
| Scope | Global |
| Action | Block |
| Active from | `09:00` |
| Active until | `17:00` |
| Active days | Mon–Fri only |
