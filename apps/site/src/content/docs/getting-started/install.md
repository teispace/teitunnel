---
title: Install
description: Install Teitunnel and the cloudflared connector it runs.
---

Teitunnel runs on **macOS 14 (Sonoma) or later**, on Apple silicon and Intel Macs.
Windows and Linux versions are planned.

## Get the app

Download the latest release from [GitHub Releases](https://github.com/teispace/teitunnel/releases),
open the disk image and drag Teitunnel to Applications.

To build it yourself instead, see the [contributing guide](https://github.com/teispace/teitunnel/blob/main/CONTRIBUTING.md).

## Install cloudflared

Routes and Quick Shares run on [cloudflared](https://github.com/cloudflare/cloudflared),
Cloudflare's connector. On first launch Teitunnel looks for it (including a Homebrew
install). If it isn't there, choose **Install cloudflared** and Teitunnel downloads the
latest release from GitHub and checks it before use:

- the download must match the digest GitHub publishes,
- the binary must match the checksum in Cloudflare's release notes,
- and its code signature must be Cloudflare's.

The managed copy lives in Teitunnel's own folder and is updated from
**Settings ▸ cloudflared**. When it updates, running connectors move to the new version
without dropping your routes.

## Next

- [Share a local service](/teitunnel/getting-started/quick-share/) in a few seconds, no account needed.
- [Put a service on your own domain](/teitunnel/getting-started/first-route/).
