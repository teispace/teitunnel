---
title: Run modes
description: Keep routes running only while Teitunnel is open, or all the time.
---

This Mac's connector runs in one of two modes, set per account in **Tunnels ▸ Running**.

| | While Teitunnel runs | Always-on |
|---|---|---|
| Runs when | the app is open (including in the menu bar) | always, from login, even after you quit |
| Started by | Teitunnel | macOS (a launch agent) |
| Token | passed to the process in memory | a file only you can read (`0600`) |
| Restarts after a crash | yes, with backoff | yes, by macOS |

**Switching doesn't drop your routes.** The new connector starts and must connect before
the old one stops. If it doesn't connect within 30 seconds, the old one keeps running and
you're told why.

Closing the window keeps everything running in the menu bar. Quitting (⌘Q) with routes
running through the app asks first, and offers to switch them to Always-on.

## Updates

When cloudflared is updated, running connectors move to the new version one at a time.
An Always-on connector is covered by a temporary one while it restarts, so routes stay
up throughout.
