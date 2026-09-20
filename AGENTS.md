# AGENTS.md — Contributor & Agent Guidelines for Teitunnel

Welcome to the **Teitunnel** codebase! This document outlines engineering standards, architecture invariants, and operational guidelines for both human developers and autonomous AI agents contributing to this repository.

---

## 1. Project Mission & Identity

- **Name**: Teitunnel
- **Organization**: `teispace` (`github.com/teispace/teitunnel`)
- **Mission**: The ultimate open-source desktop control center for Cloudflare Tunnels (`cloudflared`), featuring 1-click ephemeral sharing, remotely-managed Zero Trust tunnels, visual ingress routing, automated DNS lifecycle management ("no mess"), live Prometheus telemetry, and an embedded terminal.
- **Design Standard**: Native, pixel-perfect, human-crafted desktop UI (no generic "AI generated" aesthetics). Smooth glass/vibrancy, crisp typography, clean dark/light mode, tactile interactions.

---

## 2. Technology Stack & Key Libraries

- **Desktop Framework**: Tauri v2 (`@tauri-apps/api`, `@tauri-apps/cli`)
- **Backend**: Rust 2021 Edition (Tokio, Reqwest with Rustls, Keyring, Serde, Serde_YAML, Sysinfo, Thiserror)
- **Frontend**: React 19, TypeScript 5+, Vite 8
- **Styling**: Tailwind CSS v4, Radix UI primitives, Lucide React icons
- **State Management**: Zustand
- **Visualizations**: Recharts
- **Terminal**: `@xterm/xterm`, `@xterm/addon-fit`

---

## 3. Architecture Rules & Invariants

1. **Secure Token Storage**:
   - Never write API tokens to disk or unencrypted config files.
   - Always route token read/write operations through the Rust `keyring` service.

2. **No Shell Injections**:
   - Always spawn child processes using `tokio::process::Command` with discrete arguments (`.arg()`), never through raw shell strings (`sh -c`).

3. **DNS Hygiene ("No Mess")**:
   - Any feature that provisions a tunnel route MUST support automated DNS CNAME record creation and cascade cleanup on deletion.
   - Always protect user domains from orphaned CNAME pointers.

4. **Process Supervision**:
   - The Rust backend manages `cloudflared` instances asynchronously using Tokio tasks.
   - Stdout/stderr must be parsed non-blockingly and piped to frontend event listeners (`app.emit("tunnel-log", ...)`).
   - Graceful shutdown (`SIGTERM`) with a 5-second deadline followed by `SIGKILL` must be respected.

5. **Type Safety Across the IPC Boundary**:
   - All Tauri commands must return a strongly typed `Result<T, AppError>`.
   - Frontend IPC calls must use typed wrappers defined in `src/lib/tauri.ts`.

---

## 4. Directory Layout

```
teitunnel/
├── src-tauri/
│   ├── src/
│   │   ├── commands/     # Tauri IPC command handlers
│   │   ├── services/     # Business logic: ProcessMgr, CloudflareApi, Keyring, Metrics
│   │   ├── models/       # Data transfer objects (Tunnel, Ingress, DNS, Metrics)
│   │   ├── error.rs      # Serializable error enums
│   │   ├── lib.rs        # Tauri app builder and plugin registration
│   │   └── main.rs       # App entry point
│   ├── tauri.conf.json   # Tauri configuration
│   └── Cargo.toml
├── src/
│   ├── components/       # Reusable UI & view components
│   │   ├── ui/           # Radix/Tailwind design system components
│   │   ├── layout/       # Sidebar, TopBar, WindowControls
│   │   ├── tunnels/      # Tunnel management views & cards
│   │   ├── quick-tunnel/ # 1-click ephemeral tunnel launcher + QR code
│   │   ├── ingress/      # Visual rule editor
│   │   ├── dns/          # DNS records list & hygiene cleaner
│   │   ├── metrics/      # Telemetry graphs & edge colos
│   │   └── terminal/     # Embedded xterm.js terminal
│   ├── hooks/            # Custom React hooks
│   ├── stores/           # Zustand stores (auth, tunnels, settings, logs)
│   ├── lib/              # Tauri IPC wrappers & utilities
│   ├── App.tsx           # Main application root
│   └── index.css         # Tailwind v4 configuration & theme tokens
├── docs/                 # Architecture, Roadmap, Security documentation
└── README.md
```

---

## 5. Development Commands

- **Install frontend dependencies**: `pnpm install`
- **Run dev mode (Vite frontend only)**: `pnpm dev`
- **Run desktop app dev mode**: `pnpm tauri dev`
- **Check Rust compilation**: `cd src-tauri && cargo check`
- **Run Rust tests**: `cd src-tauri && cargo test`
- **Build production desktop binary**: `pnpm tauri build`
