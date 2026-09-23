---
title: Traffic, logs and notifications
description: See what your routes are doing, and hear about problems.
---

## Traffic

**Tunnels ▸ Traffic** charts this Mac's connector: requests and failed requests per
second, and the round trip to Cloudflare, over the last **hour**, **day** or **week**.
While the chart is open it updates every second; history is kept per minute for seven
days. Below it: the current rate, responses by status class (2xx, 3xx, 4xx, 5xx), totals
and the edge locations in use. **Overview** shows a summary.

cloudflared reports traffic per tunnel, not per hostname, so these numbers cover all of
this Mac's routes together.

## Logs

- **Tunnels ▸ Logs** shows the connector's log.
- **Routes ▸ Logs** shows only the lines about that route's requests. cloudflared logs
  failed requests; successful ones are logged only at debug level.

Search, filter to warnings or errors, pause, copy, or save to a file (secrets are removed
from saved logs).

## Notifications

Teitunnel notifies you when:

- this Mac's connector loses its connection for more than 20 seconds, and when it's back,
- a connector keeps crashing,
- the Doctor finds a new problem (it checks every few minutes, also with the window closed),
- a Quick Share goes live or stops.

Brief blips stay quiet, each problem is announced once, and nothing is shown while a
Teitunnel window is in front. Each kind can be turned off in **Settings ▸ General**.
