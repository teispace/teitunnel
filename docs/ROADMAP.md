# Teitunnel Project Roadmap & Milestones

This document tracks the phased development plan for Teitunnel.

---

## Phase 1: Foundation & Project Scaffolding ✅
- [x] Create project repository under `teispace/teitunnel`
- [x] Configure Tauri v2 with React 19, TypeScript, Vite, Tailwind CSS v4, and shadcn/ui
- [x] Establish Rust backend architecture with Tokio, Reqwest, Keyring, and Serde
- [x] Design system setup: Dark/Light mode, macOS native window styling (frameless overlay), typography
- [x] Set up documentation suite (`ARCHITECTURE.md`, `ROADMAP.md`, `SECURITY.md`, `AGENTS.md`, `README.md`)

---

## Phase 2: Binary Management & Quick Ephemeral Tunnel ⚡
- [ ] **Binary Manager**:
  - [ ] Auto-detect existing system `cloudflared` (`/opt/homebrew/bin/cloudflared`, etc.)
  - [ ] Version query and compatibility validation
  - [ ] Automatic download & self-managed binary fallback (`~/.teitunnel/bin/cloudflared`) for macOS, Linux, and Windows
- [ ] **Quick Ephemeral Tunnel ("Try Instantly")**:
  - [ ] 1-click port forward (`3000`, `8080`, custom ports, or local directory)
  - [ ] Real-time stdout parsing for `https://*.trycloudflare.com` URL
  - [ ] Shareable link with Copy button, Open in Browser, and QR Code modal
  - [ ] Live connection status badge and quick Stop button

---

## Phase 3: Cloudflare API Integration & Token Management 🔑
- [ ] **Keychain Storage**:
  - [ ] Integration with macOS Keychain / Windows Credential Manager / Linux Secret Service via Rust `keyring`
  - [ ] Scoped token validation against `https://api.cloudflare.com/client/v4/user/tokens/verify`
- [ ] **Account & Zone Explorer**:
  - [ ] Fetch accessible accounts and zones/domains
  - [ ] Multi-account selector in UI header

---

## Phase 4: Full Remotely-Managed Tunnels & Ingress Builder 🌐
- [ ] **Tunnel Lifecycle**:
  - [ ] List existing tunnels in Cloudflare account (status, connections, creation date)
  - [ ] Create new named tunnel (auto-provisions tunnel + secure token)
  - [ ] Start / Stop / Restart tunnel child process supervised by Tokio
  - [ ] Delete tunnel with confirmation
- [ ] **Visual Ingress Rule Builder**:
  - [ ] Add / Edit / Remove ingress rules
  - [ ] Protocol selectors: HTTP, HTTPS, TCP, SSH, RDP, Unix Socket
  - [ ] Hostname and path regex routing
  - [ ] Fallback catch-all (`http_status:404`)
  - [ ] Advanced TLS flags (`noTLSVerify`, SNI override, origin CA)

---

## Phase 5: "No-Mess" DNS Engine & Hygiene Scanner 🧹
- [ ] **Automated DNS Sync**:
  - [ ] Auto-provision proxied CNAME records on Cloudflare DNS when linking hostnames
  - [ ] Cascade deletion prompt: Purge CNAME records when routes/tunnels are deleted
- [ ] **DNS Hygiene Scanner**:
  - [ ] Scan zone DNS records for orphaned `*.cfargotunnel.com` CNAME pointers
  - [ ] 1-click batch cleanup for dead tunnel records

---

## Phase 6: Telemetry, Charts & Embedded Terminal 📊
- [ ] **Prometheus Metrics Scraper**:
  - [ ] Local metrics polling (`127.0.0.1:<port>`)
  - [ ] Real-time latency chart (sparkline/area graph)
  - [ ] Active Cloudflare edge colos badges (e.g. `SJC`, `FRA`, `NRT`)
  - [ ] Request volume and HTTP status code breakdown
- [ ] **Embedded Terminal & Log Viewer**:
  - [ ] xterm.js integration for colorized live streaming logs
  - [ ] Search, filter by log level (`INFO`, `WARN`, `ERROR`), pause/resume stream
  - [ ] Embedded CLI runner for ad-hoc `cloudflared` commands

---

## Phase 7: Native Desktop Polish & Distribution 🚀
- [ ] System Tray / Menu Bar companion with 1-click tunnel toggles
- [ ] Native OS desktop notifications for tunnel events
- [ ] GitHub Actions CI/CD workflow for automated multi-platform builds (macOS dmg, Windows msi/exe, Linux AppImage/deb)
