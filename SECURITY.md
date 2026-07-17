# Security Policy

## Supported versions

Only the latest release receives security fixes. Token Guard is pre-1.0 and
moves fast; please stay on the newest version.

| Version | Supported |
| ------- | --------- |
| Latest release | ✅ |
| Older releases | ❌ |

## Reporting a vulnerability

Please **do not** open a public GitHub issue for security reports.

Email **qingquanshi65@gmail.com** with:

- A description of the issue and its impact
- Steps to reproduce or a proof of concept
- The Token Guard version and OS you tested on

You should get an acknowledgement within a few days. If the report is
confirmed, we will coordinate a fix and a disclosure timeline with you.

## Localhost trust model (please read before reporting)

Token Guard is a **local** tool. Its HTTP proxy binds to `127.0.0.1` by
default and intentionally **trusts every client that can reach it on
loopback**:

- The `tg_<project-label>` "API key" a client sends is used to **tag spend to
  a project** for cost accounting. It is *not* an authentication credential.
  Any local process can pick any label, and there is no secret to brute-force
  — by design.
- Your real provider API keys never leave the OS keychain except inside
  requests sent directly to the configured provider. The proxy never returns
  them over its HTTP interface.
- Limit and budget enforcement is advisory for your own agents, not a
  security boundary against other software running as your user.

**Not a vulnerability:** "another local process can route requests through
the proxy / claim my project label / see my local usage data." That is the
documented trust model — the same trust you already grant any process running
as your user. Reports along these lines will be closed as working as
intended.

**In scope and very welcome:**

- The proxy or app exposing provider API keys (keychain contents) anywhere
  they shouldn't be: logs, HTTP responses, error messages, exported files.
- The proxy accepting connections from non-loopback addresses when
  `expose_to_lan` is **disabled**, or behaving unexpectedly when it is
  enabled (LAN exposure is an explicit opt-in setting).
- The updater installing or launching anything that fails the published
  sha256 for the release asset, or downloading over a channel that can be
  tampered with.
- Anything that corrupts or exfiltrates the local SQLite database from
  outside the app.
- Website/license-worker issues (key enumeration, webhook forgery, etc.).

## Hardening notes

- Keep `expose_to_lan` **off** unless you specifically need LAN devices to
  reach the proxy. When it is on, every device on the network shares the
  loopback trust model above.
- Updates are only offered to valid supporter keys, and every downloaded
  asset is verified against the sha256 digest published by the GitHub
  Releases API before installation.
