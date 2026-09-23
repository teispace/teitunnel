---
title: Share a local service
description: Get a temporary public URL for anything running on your Mac.
---

A **Quick Share** gives a service on your Mac a random public URL on
`trycloudflare.com`. It needs no Cloudflare account.

1. Open **Quick Share** (⌘3) or choose **Share a Local Port…** in the menu bar.
2. Pick one of the running services Teitunnel found (dev servers and Docker
   containers are listed first), or type a port or address such as `3000` or
   `http://localhost:8080`.
3. Choose how long it should keep running, then **Share**.

The URL appears as soon as Cloudflare assigns it. Copy it, open it, or show a QR code
for a phone. The card shows requests, errors and the connector's log.

**Stop Sharing** ends it, as does quitting Teitunnel. Anyone with the link can reach the
service while it runs, so share only what you mean to.

:::note
Quick Shares are meant for testing and demos. Cloudflare doesn't guarantee their
uptime, and the URL changes every time. For a stable address, use
[your own domain](/teitunnel/getting-started/first-route/).
:::
