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
teitunnel-cli route add admin.example.com 3000 --allow me@example.com --allow @example.com
teitunnel-cli route remove app.example.com
teitunnel-cli network add 192.168.1.0/24     # let WARP users reach a range
teitunnel-cli networks                       # the ranges this Mac shares
teitunnel-cli network remove 192.168.1.0/24
teitunnel-cli export terraform > teitunnel.tf
teitunnel-cli accounts
teitunnel-cli share 3000 --for 30m          # a temporary public URL, until Ctrl-C
teitunnel-cli doctor                        # check for problems; exits 1 on an error
teitunnel-cli doctor --fix                  # apply the safe fixes
```

| Option | Meaning |
|---|---|
| `-a, --account <name or id>` | Which account, when several are connected. |
| `-y, --yes` | Apply without asking (needed when there's no terminal to ask on, e.g. in scripts). |
| `--allow <email or @domain>` | `route add`: require a login; repeat for more people ([Require a login](/guides/require-login/)). |
| `--replace` | Also allow what needs a confirmation: replacing or deleting DNS records Teitunnel didn't create, or sharing a public range ([Private networks](/guides/private-networks/)). |
| `--json` | Machine-readable output for `accounts`, `routes` and `doctor`. |
| `--for <duration>` | `share`: stop by itself after `90s`, `30m` or `2h` (a bare number is minutes). |
| `--no-qr` | `share`: don't print a QR code. |
| `--fix` | `doctor`: apply the fixes that only touch what Teitunnel created, each through a plan. |

The CLI doesn't run your routes' connectors. Routes are served by the app while it's
open, or all the time with [Always-on](/teitunnel/concepts/run-modes/). After adding a
route, the CLI checks it through Cloudflare and exits with status 1 if it doesn't work yet.

`share` is the exception: it runs a Quick Share for exactly as long as the command. Ctrl-C,
closing the terminal or `--for` stops it. The URL is the only line on standard output, so
`URL=$(teitunnel-cli share 3000 | head -1)` works in scripts. `share` doesn't need an
account, only cloudflared (the app's copy, or one on your system).

`doctor` runs the app's checks and hides the issues you ignored there.

## Shell completions

```sh
teitunnel-cli completions zsh > ~/.zfunc/_teitunnel-cli   # zsh (with ~/.zfunc in fpath)
teitunnel-cli completions bash > ~/.local/share/bash-completion/completions/teitunnel-cli
teitunnel-cli completions fish > ~/.config/fish/completions/teitunnel-cli.fish
```

PowerShell and Elvish are supported too.

The first time it reads your Cloudflare token, macOS asks whether `teitunnel-cli` may use
the keychain item Teitunnel created. Choose **Always Allow** to not be asked again.
