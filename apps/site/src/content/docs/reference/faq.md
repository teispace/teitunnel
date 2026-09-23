---
title: FAQ
description: Common questions.
---

### Is Teitunnel made by Cloudflare?

No. It's an independent open-source app (MIT) that uses Cloudflare's public API and
runs Cloudflare's `cloudflared` connector.

### Does it cost anything?

Teitunnel is free. Cloudflare Tunnel is available on Cloudflare's free plan; Quick Shares
need no account at all.

### What happens to my routes when I quit?

They stop, unless you've turned on [Always-on](/teitunnel/concepts/run-modes/). Quitting
with routes running asks first and offers to switch them.

### Can I still use cloudflared from the command line?

Yes. Teitunnel works with the Cloudflare API and its own connector, and lists other
cloudflared processes on the Mac under **Tunnels ▸ Also on this Mac** so they don't
conflict unnoticed. Routes you change elsewhere are detected and never overwritten
without asking.

### I already have tunnels. Will Teitunnel change them?

No. Teitunnel creates its own tunnel for this Mac and only changes that one. Other
tunnels are listed read-only. You can [import](/teitunnel/guides/import/) routes from a
`config.yml` setup when you're ready.

### Why does a route say "Nothing is listening"?

The route points at a port where no app is running. Start your app (or fix the port in
the route) and it goes live; nothing else needs to change.

### Windows and Linux?

Planned. The core already builds and is tested on both; the apps follow macOS.
