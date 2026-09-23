---
title: Command line
description: Manage routes from the terminal with teitunnel-cli.
---

`teitunnel-cli` works with the accounts you connected in the app. Every change goes
through the same preview as the app: you see the plan before anything changes.

```sh
teitunnel-cli routes                         # this Mac's routes and their status
teitunnel-cli route add app.example.com 3000 # route a hostname to localhost:3000
teitunnel-cli route add api.example.com 8000 --path '^/v1/'
teitunnel-cli route remove app.example.com
teitunnel-cli export terraform > teitunnel.tf
teitunnel-cli accounts
```

| Option | Meaning |
|---|---|
| `-a, --account <name or id>` | Which account, when several are connected. |
| `-y, --yes` | Apply without asking (needed when there's no terminal to ask on, e.g. in scripts). |
| `--replace` | Also allow replacing or deleting DNS records Teitunnel didn't create. |
| `--json` | Machine-readable output for `accounts` and `routes`. |

The CLI doesn't run connectors. Routes are served by the app while it's open, or all the
time with [Always-on](/teitunnel/concepts/run-modes/). After adding a route, the CLI checks
it through Cloudflare and exits with status 1 if it doesn't work yet.

The first time it reads your Cloudflare token, macOS asks whether `teitunnel-cli` may use
the keychain item Teitunnel created. Choose **Always Allow** to not be asked again.
