# Teitunnel

**A native desktop app for Cloudflare Tunnel.** Put any local app on your own domain in under a minute, and keep it clean, healthy and under control.

> **Status: early development.** Teitunnel is being rebuilt from the ground up. The first release (v0.1, Quick Share for macOS) is in progress. See the [roadmap](docs/ROADMAP.md) and [current status](docs/STATUS.md).

## What it will do

- **Quick Share:** expose `localhost:3000` at a public `trycloudflare.com` URL in one click, with a QR code and no account needed.
- **Routes across all your domains:** `app.xyz.com → localhost:3000`, `api.yx.com → localhost:5000`, served from one tunnel on your machine. Teitunnel creates the tunnel, configuration and DNS records for you.
- **See before you change:** every change is shown as a plan before it touches your Cloudflare account, and can be undone.
- **No mess:** DNS records Teitunnel creates are tracked and removed when you remove a route. Orphaned records from old tunnels are found and flagged.
- **Doctor:** detects closed origin ports, DNS conflicts, pending nameservers, blocked QUIC, crash loops, config drift and more, with one-click fixes.
- **Knows your machine:** detects running dev servers and Docker containers and suggests them as origins. Imports existing `cloudflared` setups.
- **Always-on:** keep tunnels running after quitting the app or rebooting, managed by macOS launchd.
- **Live insight:** request rates, errors, latency, edge locations and structured logs.
- **Native:** built to feel like it shipped with macOS. Keyboard-first, menu bar extra, light and dark.
- **Private and secure:** credentials live in your OS keychain. No telemetry and no Teitunnel servers.

## Platforms

macOS first (v1.0), then Windows and Linux.

## Built with

Tauri 2 · Rust · React · TypeScript · Tailwind CSS. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) and [docs/README.md](docs/README.md).

## Security

Please report vulnerabilities privately. See [SECURITY.md](SECURITY.md).

## License

[MIT](LICENSE) © teispace

Teitunnel is an independent open-source project and is not affiliated with or endorsed by Cloudflare, Inc. "Cloudflare" is a trademark of Cloudflare, Inc.
