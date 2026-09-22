# Vision

## The problem

Cloudflare Tunnel is the best way to put a local service on the internet: no port forwarding, no static IP, TLS included. Using it is still hard work:

- Four separate concepts (tunnel, connector, ingress, DNS), spread across a CLI, YAML files, credential JSON files, `cert.pem` and the dashboard.
- Two tunnel types (locally-managed vs remotely-managed) that behave differently, and docs that mix them.
- DNS records that outlive the tunnels they point to.
- No easy way to see which process is running, why a hostname returns 502 or 1033, or what changed.
- Running several domains from one machine (`xyz.com → :3000`, `yx.com → :5000`) is possible, but only if you know how ingress rules and multi-zone DNS work.

## The product

**Teitunnel is a native desktop app that makes Cloudflare Tunnel effortless for beginners and fast for experts.**

You say what you want ("this local app, at this hostname"). Teitunnel works out the tunnel, configuration, DNS, process, health checks and cleanup. It shows exactly what it will change before it changes it, and it keeps watching afterwards.

## Who it's for

| Persona | Needs | Teitunnel gives them |
|---|---|---|
| **Sharer**: frontend dev, designer, student | Show localhost to someone right now | Quick Share: one click, URL + QR, no account |
| **Builder**: indie dev, freelancer | Stable hostnames on their own domains for several projects | Routes: `app.xyz.com → :3000`, `api.yx.com → :5000`, any number of domains, one tunnel |
| **Operator**: homelabber, self-hoster, small team | Always-on services, health, clean DNS, troubleshooting | Always-on mode, Doctor, cleanup, metrics, logs, import of existing setups |

## Principles

1. **Easy by default, powerful on demand.** The first screen never says "ingress". Every `cloudflared` option is still one disclosure away.
2. **Show before change.** Every mutation is a plan the user can read before applying. Nothing surprising happens to someone's domain.
3. **Never leave a mess.** Everything Teitunnel creates is tracked and removed when no longer needed. Anything it didn't create, it never deletes silently.
4. **Native, not a website in a box.** It should feel as if it shipped with macOS: system fonts, system accent, native menus, keyboard first, dense and calm.
5. **Transparent.** Every action can be copied as the equivalent `cloudflared` command or API call. Users learn instead of depending on a black box.
6. **Local-first and private.** There's no Teitunnel server, account or telemetry. Credentials stay in the OS keychain. The app only talks to Cloudflare and GitHub (for binary and app updates).
7. **Secure by construction.** Secrets never enter the webview, and processes never receive secrets on the command line. Least-privilege scopes.
8. **Fast.** Instant start, no spinners for local state, and smooth realtime views.

## Non-goals

- A general Cloudflare dashboard. We only touch what tunnels need: DNS for routed hostnames, Access for protecting routes, and zones for selection.
- A hosted service or an account system of our own.
- Mobile apps.
- Windows/Linux polish before the macOS v1.0. They are built and tested in CI from day one, then polished after v1.0.

## What success looks like

- A new user with a Cloudflare account and a domain goes from install to a working `https://app.theirdomain.com` in **under 60 seconds**.
- Deleting a route or tunnel leaves **zero** orphaned DNS records.
- Every Doctor issue has either a one-click fix or a clear explanation.
- Reviewers describe it as "feels like a real Mac app".
