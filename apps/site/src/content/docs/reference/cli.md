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
teitunnel-cli tunnels                        # this machine's tunnels
teitunnel-cli tunnel create staging          # another tunnel for this machine
teitunnel-cli route add beta.example.com 4000 --tunnel staging
teitunnel-cli tunnel delete staging
teitunnel-cli accounts
teitunnel-cli share 3000 --for 30m          # a temporary public URL, until Ctrl-C
teitunnel-cli share 3000 --on demo.example.com  # the same, on your own domain
teitunnel-cli shares                        # shares on your domains, from anywhere
teitunnel-cli shares --stop demo.example.com
teitunnel-cli doctor                        # check for problems; exits 1 on an error
teitunnel-cli doctor --fix                  # apply the safe fixes
teitunnel-cli setup                         # store an API token in the keychain
teitunnel-cli up                            # run this machine's tunnels (servers, Docker)
teitunnel-cli always-on on                  # as a service (a system unit as root)
teitunnel-cli routes --check                # exit 1 unless every route is live
```

| Option | Meaning |
|---|---|
| `-a, --account <name or id>` | Which account, when several are connected. |
| `-y, --yes` | Apply without asking (needed when there's no terminal to ask on, e.g. in scripts). |
| `--allow <email or @domain>` | `route add`: require a login; repeat for more people ([Require a login](/guides/require-login/)). |
| `--replace` | Also allow what needs a confirmation: replacing or deleting DNS records Teitunnel didn't create, or sharing a public range ([Private networks](/guides/private-networks/)). |
| `--tunnel <name>` | Which of this machine's tunnels a change or export is about ([Several tunnels](/guides/several-tunnels/)). Default: the tunnel carrying the route, or the default tunnel. |
| `--json` | Machine-readable output for `accounts`, `routes`, `tunnels` and `doctor`. |
| `--for <duration>` | `share`: stop by itself after `90s`, `30m` or `2h` (a bare number is minutes). |
| `--no-qr` | `share`: don't print a QR code. |
| `--fix` | `doctor`: apply the fixes that only touch what Teitunnel created, each through a plan. |

Routes are served by the app while it's open, all the time with
[Always-on](/teitunnel/concepts/run-modes/) (`always-on on` from the CLI), or by
`teitunnel-cli up` on a server or in a container. Without the app, give the API token in
`CLOUDFLARE_API_TOKEN` or `CLOUDFLARE_API_TOKEN_FILE`; it's never stored
([Servers and containers](/guides/servers/)). After adding a
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
