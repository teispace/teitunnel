# Roadmap

macOS first (D-002). Linux and Windows compile and pass CI from M0, and get their UX polish in M7/M8.
Task-level detail is in [`plans/`](plans). Live progress is in [STATUS.md](STATUS.md).

| Milestone | Release | Theme | Status |
|---|---|---|---|
| [M0](plans/M0-foundations.md) | — | Workspace, native shell, design system, typed IPC, CI | ⏳ Next |
| [M1](plans/M1-binary-quick-share.md) | v0.1.0 | cloudflared manager, supervisor, Quick Share | Planned |
| [M2](plans/M2-accounts-domains.md) | v0.2.0 | OAuth / token / cert.pem, multi-account, domains | Planned |
| [M3](plans/M3-routes-engine.md) | v0.3.0 | Plan → apply engine, routes across domains, DNS ownership, drift | Planned |
| [M4](plans/M4-discovery-doctor.md) | v0.4.0 | Discovery, import/adopt, Doctor, cleanup | Planned |
| [M5](plans/M5-observability-always-on.md) | v0.5.0 | Metrics, logs, activity, always-on (launchd), menu bar | Planned |
| [M6](plans/M6-distribution.md) | **v1.0.0** | Signing, notarization, updater, Homebrew, docs site | Planned |
| [M7](plans/M7-M9-beyond-v1.md#m7-windows) | v1.1 | Windows | Later |
| [M8](plans/M7-M9-beyond-v1.md#m8-linux) | v1.2 | Linux | Later |
| [M9](plans/M7-M9-beyond-v1.md#m9-advanced-features-v1x) | v1.x | Access protection, private networks, remote connectors, export, CLI | Later |

## Milestone checklist (epic level)

### M0: Foundations
- [ ] M0-01 Workspace skeleton
- [ ] M0-02 Library crates
- [ ] M0-03 Tauri app shell
- [ ] M0-04 Frontend scaffold
- [ ] M0-05 Typed IPC pipeline
- [ ] M0-06 Design tokens, motion & platform styling
- [ ] M0-07 UI primitives
- [ ] M0-08 Layout patterns
- [ ] M0-09 Native menus, tray stub, shortcuts
- [ ] M0-10 Store foundation
- [ ] M0-11 CI
- [ ] M0-12 Repo hygiene

### M1: cloudflared + Quick Share (v0.1.0)
- [ ] M1-01 Locate + version · [ ] M1-02 Managed install · [ ] M1-03 Command builders · [ ] M1-04 Log parser
- [ ] M1-05 Local endpoints · [ ] M1-06 fake-cloudflared · [ ] M1-07 Supervisor · [ ] M1-08 Port discovery
- [ ] M1-09 Quick Share · [ ] M1-10 Onboarding (binary) · [ ] M1-11 Notifications · [ ] M1-12 Tests/E2E · [ ] M1-13 Release

### M2: Accounts & Domains (v0.2.0)
- [ ] M2-01 cf-api foundation · [ ] M2-02 Accounts/zones · [ ] M2-03 Capabilities · [ ] M2-04 OAuth
- [ ] M2-05 Token flow · [ ] M2-06 cert.pem import · [ ] M2-07 Account store · [ ] M2-08 Connect UI · [ ] M2-09 Domains view

### M3: Routes engine (v0.3.0)
- [ ] M3-01 cf-api tunnels/config/DNS · [ ] M3-02 Types & validation · [ ] M3-03 Observer · [ ] M3-04 Planner
- [ ] M3-05 Executor + activity · [ ] M3-06 Verifier · [ ] M3-07 Machine tunnel · [ ] M3-08 Drift
- [ ] M3-09 Routes UI · [ ] M3-10 Tunnels UI · [ ] M3-11 Tests

### M4: Discovery & Doctor (v0.4.0)
- [ ] M4-01 Processes/projects · [ ] M4-02 Docker · [ ] M4-03 Import setups · [ ] M4-04 Adoption
- [ ] M4-05 Doctor framework · [ ] M4-06 Checks · [ ] M4-07 Doctor UI · [ ] M4-08 Cleanup center · [ ] M4-09 Diagnostics export

### M5: Observability & Always-on (v0.5.0)
- [ ] M5-01 Metrics pipeline · [ ] M5-02 Charts · [ ] M5-03 Log viewer · [ ] M5-04 Activity view
- [ ] M5-05 launchd always-on · [ ] M5-06 Lifecycle & menu bar · [ ] M5-07 Notifications policy

### M6: Distribution (v1.0.0)
- [ ] M6-01 Signing/notarization · [ ] M6-02 Updater · [ ] M6-03 Release automation · [ ] M6-04 Homebrew
- [ ] M6-05 Polish pass · [ ] M6-06 Docs & community
