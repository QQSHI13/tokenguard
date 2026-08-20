# Team sharing over Tailscale

One member of your team hosts Token Guard; every other member points their AI
clients at the host's Tailscale address. No cloud account, no per-seat fees —
the gateway, your keys, and the logs stay on the host's machine.

## Why Tailscale?

Tailscale creates an encrypted private network (a *tailnet*) between your
devices. Token Guard binds **only** to the host's Tailscale IP — it is not
reachable from the LAN or the internet. Every teammate needs Tailscale
installed and signed in to the same tailnet to reach the gateway.

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
   `http://100.100.100.5:3742/v1`.

4. Restart the app so the gateway binds to the tailnet address.

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
tokenguard share off     # disable (loopback only)
tokenguard share status  # show state and tailnet IP
tokenguard settings set-share-tailscale true   # same as `share on`
```