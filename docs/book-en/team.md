# Team sharing over Tailscale

One member of your team hosts Token Guard; every other member points their AI
clients at the host's Tailscale address. No cloud account, no per-seat fees —
the gateway, your keys, and the logs stay on the host's machine.

## Why Tailscale?

Tailscale creates an encrypted private network (a *tailnet*) between your
devices. Token Guard is **not** reachable from the LAN or the internet — it
either binds only to the host's Tailscale IP and loopback, or (see below)
stays on loopback behind `tailscale serve`. Every teammate needs Tailscale
installed and signed in to the same tailnet to reach the gateway.

## Two exposure modes

Token Guard picks the mode automatically when you enable sharing:

- **Direct** — the normal case. The gateway binds to the host's Tailscale IP
  (`100.x.x.x`) plus loopback. Teammates use `http://<tailscale-ip>:3742/v1`.
- **Serve** — fallback for hosts where Tailscale runs in *userspace
  networking* mode (common in WSL) and has no `100.x` interface. The gateway
  stays on loopback and a `tailscale serve` route exposes it at the `/tg`
  path: `https://<host>.ts.net/tg/v1`. The path prefix keeps the route
  independent of anything else served on the same host, and only tailnet
  devices can reach it.

`tokenguard share status` shows which mode is active.

## Set up the host

1. Install and sign in to [Tailscale](https://tailscale.com) on the host.
2. Start Token Guard (GUI or `tokenguard start`).
3. Enable sharing:

   **CLI**
   ```bash
   tokenguard share on
   ```

   **GUI** — Settings → *Share with team over Tailscale*.

   The command prints the team endpoint, for example
   `http://100.100.100.5:3742/v1` (direct mode) or
   `https://my-host.tail1234.ts.net/tg/v1` (serve mode).

4. Restart the app so the gateway binds to the tailnet address (direct mode
   only — serve mode needs no restart).

## Connect teammates

Each teammate needs:

1. Tailscale installed and signed in to the same tailnet.
2. A **project label key** (`tg_...`) created on the host machine — in the
   GUI's Projects tab or via `tokenguard project add`. The label key is what
   authenticates requests and tags them to a project.

Then point any client at the team endpoint:

```bash
OPENAI_BASE_URL=http://100.100.100.5:3742/v1
OPENAI_API_KEY=tg_team-project
```

Anthropic and Gemini SDKs work too:

```bash
ANTHROPIC_BASE_URL=http://100.100.100.5:3742
ANTHROPIC_API_KEY=tg_team-project
```

```bash
GEMINI_API_KEY=tg_team-project
# base URL: http://100.100.100.5:3742/v1beta
```

## What teammates can and cannot do

- **Can** use every configured provider through the 4 × 4 conversion — a
  teammate sending OpenAI-shaped requests can call Anthropic or Gemini models.
- **Can** be tracked: their requests are tagged to the project whose label key
  they use and count against that project's budgets and limits.
- **Cannot** see your other projects' label keys, your real provider keys, or
  the usage history of projects they don't have a key for.
- **Cannot** change settings — the gateway is read-only for teammates.

## Controlling access

- Create one project per teammate (or per team) so you can revoke a single
  person by deleting their project.
- Set per-project budgets and limits — a runaway teammate hits their cap, not
  your whole gateway.
- `tokenguard share off` (or the GUI toggle) disconnects everyone at once.

## CLI reference

```bash
tokenguard share on      # enable, prints the team endpoint
tokenguard share off     # disable (loopback only, removes the serve route)
tokenguard share status  # show state, mode, and team endpoint
tokenguard settings set-share-tailscale true   # same as `share on`
```