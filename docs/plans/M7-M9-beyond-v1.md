# After v1.0

These are outlines. Each will be expanded into a full plan file (same format as M0–M6) when it becomes the current milestone.

## M7: Windows
Full plan: [M7-windows.md](M7-windows.md).

- Mica backdrop (`windowEffects: ["mica"]`), Segoe UI Variable, Windows caption controls with snap layouts, tray, notifications.
- Always-on via Task Scheduler (per-user, at logon) or a Windows service (needs elevation; evaluate), plus `CTRL_BREAK` shutdown semantics.
- Tokens via Windows Credential Manager (keyring). The managed binary is `cloudflared-windows-amd64.exe` with Authenticode verification.
- Installer: MSI/NSIS, signed (SignPath OSS or Azure Trusted Signing). Channels: winget, Scoop.
- Design pass: Windows 11 conventions (title bar, menus in-window, since there's no global menu bar).

## M8: Linux
Full plan: [M8-linux.md](M8-linux.md).

- Always-on via `systemd --user` units, native GTK decorations, solid surfaces, tray via libappindicator/StatusNotifierItem.
- Secret Service (keyring) with a clear error when no secret service is available.
- Packages: AppImage, deb, rpm, Flatpak (Flathub), AUR.

## M9: Advanced features (v1.x)
Full plan: [M9-advanced.md](M9-advanced.md).

- **Protect with Access:** per-route toggle "Require login" (emails / email domain / one-time PIN) via Access applications + policies. Optional OAuth scopes.
- **Private networks:** CIDR routes and virtual networks for WARP clients; `cloudflared access` helpers for SSH/RDP/TCP on the client side.
- **Remote connectors:** manage tunnels running on other machines, with remote log streaming via the Management API (`POST …/management` token + websocket).
- **Replicas:** run the same tunnel on several machines; health across connectors.
- **Export:** config as `config.yml`, Terraform (`cloudflare_zero_trust_tunnel_cloudflared*`), or Docker Compose snippets.
- **CLI:** `apps/cli` over `teitunnel-core` (`teitunnel route add app.xyz.com :3000`).
- **i18n:** extract strings, community translations.
- **Load-balanced origins / multiple origins per route**, if cloudflared/Cloudflare support evolves.
