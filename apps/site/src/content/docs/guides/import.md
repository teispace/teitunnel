---
title: Import an existing setup
description: Move routes from a cloudflared config.yml onto Teitunnel.
---

If you already run cloudflared with a `config.yml` (a *locally-managed* tunnel),
Teitunnel can bring its routes over.

1. In **Routes**, choose **Import** in the toolbar. Teitunnel scans `~/.cloudflared` and
   the usual system locations for config files and lists the routes it found.
2. Pick the routes to import and **Review**. They're added to this Mac's tunnel in one
   change, previewed like any other. DNS records that point at the old tunnel weren't
   created by Teitunnel, so repointing them needs your confirmation.
3. **Apply**, and check the routes work.

Your config files and the old tunnel are left untouched, so you can switch back. Once
you're happy, stop the old cloudflared yourself; Teitunnel lists it under
**Tunnels ▸ Also on this Mac** and can stop it for you.

:::note
Routes with their own `originRequest` settings in `config.yml` aren't imported yet.
They're listed with the reason, so you can add them by hand.
:::
