# Roadmap

macOS first (D-002). Linux and Windows compile and pass CI from M0, and get their UX polish in M7/M8.
Task-level detail is in [`plans/`](plans). Live progress is in [STATUS.md](STATUS.md).

| Milestone | Release | Theme | Status |
|---|---|---|---|
| [M0](plans/M0-foundations.md) | — | Workspace, native shell, design system, typed IPC, CI | ✅ Done (PR #1, awaiting merge) |
| [M1](plans/M1-binary-quick-share.md) | v0.1.0 | cloudflared manager, supervisor, Quick Share | ⏳ Release prep (PR #2) |
| [M2](plans/M2-accounts-domains.md) | v0.2.0 | OAuth / token / cert.pem, multi-account, domains | ✅ Done except maintainer items (PR #3) |
| [M3](plans/M3-routes-engine.md) | v0.3.0 | Plan → apply engine, routes across domains, DNS ownership, drift | ✅ Done (PR #4; nightly waits for a test token) |
| [M4](plans/M4-discovery-doctor.md) | v0.4.0 | Discovery, import/adopt, Doctor, cleanup | ✅ Done (PR #5) |
| [M5](plans/M5-observability-always-on.md) | v0.5.0 | Metrics, logs, activity, always-on (launchd), menu bar | ✅ Done (PR #6) |
| [M6](plans/M6-distribution.md) | **v1.0.0** | Signing, notarization, updater, releases, downloads (v0.1.0 public beta first, D-074) | Planned |
| [M7](plans/M7-windows.md) | v1.1 | Windows | 🚧 Core done; needs Windows hardware |
| [M8](plans/M8-linux.md) | v1.2 | Linux | 🚧 Core done; needs a Linux desktop |
| [M9](plans/M9-advanced.md) | v1.x | Export, CLI, Access protection, replicas, private networks | 🚧 Export, CLI, Access, remote connectors, private networks and UI i18n done |
| [M10](plans/M10-parity.md) | v1.x | Parity and beyond: fix in place, several tunnels, share on your domain, headless server, Docker, browser UI + API, load balancing | ✅ Done |
| [M11](plans/M10-parity.md#m11-landing-page-and-docs-site) | — | Landing page and Fumadocs docs site | ✅ Built; deploys with the release work |

**Order (maintainer, 2026-09-23):** M10 → M11 → release work (M6 signing with the existing Developer ID, updater, release automation, channels, deploys) last.

## Milestone checklist (epic level)

### M0: Foundations
- [x] M0-01 Workspace skeleton
- [x] M0-02 Library crates
- [x] M0-03 Tauri app shell
- [x] M0-04 Frontend scaffold
- [x] M0-05 Typed IPC pipeline
- [x] M0-06 Design tokens, motion & platform styling (native captures of the packaged app pending an unlocked screen)
- [x] M0-07 UI primitives
- [x] M0-08 Layout patterns
- [x] M0-09 Native menus, tray stub, shortcuts
- [x] M0-10 Store foundation
- [x] M0-11 CI
- [x] M0-12 Repo hygiene

### M1: cloudflared + Quick Share (v0.1.0)
- [x] M1-01 Locate + version · [x] M1-02 Managed install · [x] M1-03 Command builders · [x] M1-04 Log parser
- [x] M1-05 Local endpoints · [x] M1-06 fake-cloudflared · [x] M1-07 Supervisor · [x] M1-08 Port discovery
- [x] M1-09 Quick Share · [x] M1-10 Onboarding (binary) · [x] M1-11 Notifications · [x] M1-12 Tests/E2E · [x] M1-13 Release (folded into M6: v0.1.0 public beta, D-074)

### M2: Accounts & Domains (v0.2.0)
- [x] M2-01 cf-api foundation · [x] M2-02 Accounts/zones (real fixtures pending) · [x] M2-03 Capabilities · [x] M2-04 OAuth (hidden until the client is registered)
- [x] M2-05 Token flow · [x] M2-06 cert.pem import · [x] M2-07 Account store · [x] M2-08 Connect UI · [x] M2-09 Domains view

### M3: Routes engine (v0.3.0)
- [x] M3-01 cf-api tunnels/config/DNS · [x] M3-02 Types & validation · [x] M3-03 Observer · [x] M3-04 Planner
- [x] M3-05 Executor + activity · [x] M3-06 Verifier · [x] M3-07 Machine tunnel · [x] M3-08 Drift
- [x] M3-09 Routes UI · [x] M3-10 Tunnels UI · [x] M3-11 Tests (nightly real-account job waits for a test token)

### M4: Discovery & Doctor (v0.4.0)
- [x] M4-01 Processes/projects · [x] M4-02 Docker · [x] M4-03 Import setups · [x] M4-04 Adoption
- [x] M4-05 Doctor framework · [x] M4-06 Checks · [x] M4-07 Doctor UI · [x] M4-08 Cleanup center (in the Doctor) · [x] M4-09 Diagnostics export

### M5: Observability & Always-on (v0.5.0)
- [x] M5-01 Metrics pipeline · [x] M5-02 Charts · [x] M5-03 Log viewer · [x] M5-04 Activity view
- [x] M5-05 launchd always-on · [x] M5-06 Lifecycle & menu bar · [x] M5-07 Notifications policy

### M6: Distribution (v1.0.0)
- [ ] M6-01 Signing/notarization · [x] M6-02 Updater · [x] M6-03 Release automation · [ ] M6-04 Channels (image and Homebrew done; winget pending) · [x] M6-07 Download experience (D-074, D-076)
- [ ] M6-05 Polish pass · [ ] M6-06 Docs & community

### M10: Parity and beyond
- [x] M10-01 Fix in place · [x] M10-02 Several tunnels per machine · [x] M10-03 Share on your domain · [x] M10-04 Headless server
- [x] M10-05 Container · [x] M10-06 Browser UI + API · [x] M10-07 Load balancing · [x] M10-08 Docs

### M11: Landing page and docs site
- [x] Landing page · [x] Fumadocs docs · [ ] Deploy (with release work)

## M12 · Inspector, agents, analytics
Plan: [plans/M12-platform.md](plans/M12-platform.md). Research: [research/competitors-2026.md](research/competitors-2026.md). Maintainer answered the open questions 2026-09-24.
- [x] M12-01 dev servers fixed in place (D-096)
- [x] M12-02 inspector: Lens core (D-100), integration (D-110) and screens (D-111); left: breakpoints, a measured perf budget
- [x] M12-03 MCP server (D-097), approvals in the app (D-121)
- [ ] M12-04 protection: edge rules (D-105) and service tokens (D-106) done; Lens gates in the UI, login presets, webhook bypass left
- [ ] M12-05 analytics: edge analytics, uptime and alerts (D-098) and the Lens source (D-110) done; live Overview left
- [x] M12-06 sharing: Snapshots (D-099), pause/resume (D-116), schedules (D-117), stable names (D-118), folder share (D-119), comments (D-122), offline page (D-123)
- [x] M12-11 CI/CD and teams: PR previews (D-104), reservations (D-103)
- [x] M12-12 more: project files (D-107), exposure check (D-108), backups (D-109), idle stop, OpenAPI from traffic (D-120), webhook inbox (D-123, D-124)
- [ ] M12-07 everywhere: control connection, links, completion, `teitunnel top` (D-102), VS Code / Raycast / JetBrains, tray and global shortcut (D-112–D-114) done; local HTTPS domains (D-101, D-115) done; browser extension later
