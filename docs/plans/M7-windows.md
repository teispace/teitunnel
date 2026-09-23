# M7: Windows

**Goal:** Teitunnel on Windows 11 with the same features as macOS, feeling like a Windows app.
**Release:** v1.1.
**Verification:** CI builds and runs every Rust test on `windows-latest`. Anything that needs a Windows desktop (look, feel, installer, real Task Scheduler) is marked *(needs a Windows machine)*.

### M7-01 · Processes and services
- [x] Helper programs start without a console window (`CREATE_NO_WINDOW`, `cloudflared::process::no_console`), for connectors, version checks and `schtasks`.
- [x] Always-on via Task Scheduler: per-user task at logon, restart on failure, no time limit, UTF-16 task XML, `CommandLineToArgvW` quoting (`cloudflared::task_scheduler`, `core::service::TaskScheduler`, D-051). Selected at runtime on Windows (D-054).
- [x] Logs for services the manager can't capture: the connector writes its own rotating log (`--log-directory`, 1 MB × 5, verified in cloudflared's `logger/`), which the app tails (D-054).
- [ ] Try it on a real Windows 11 machine: install, switch modes, reboot, uninstall. *(needs a Windows machine)*
- [ ] Graceful connector stop: `CTRL_BREAK` needs a shared console, which a GUI app doesn't have; evaluate a helper or keep `TerminateProcess` (the edge reroutes within seconds). *(needs a Windows machine)*
- [ ] Token file ACL: the file sits in the user's `AppData`, which only the user can read by default; decide whether to set an explicit ACL.

### M7-02 · Binary
- [x] Managed binary `cloudflared-windows-amd64.exe` (asset table), SHA-256 verified against the release checksums.
- [ ] Authenticode verification (`WinVerifyTrust`) with Cloudflare as the signer, like `codesign` on macOS. *(testable in CI against a signed system binary; the signer check needs research)*

### M7-03 · Credentials
- [x] Windows Credential Manager via `keyring` (`windows-native-keyring-store`, selected by default).

### M7-04 · Shell and design
- [ ] Mica backdrop (`windowEffects: ["mica"]`), Segoe UI Variable, caption controls with snap layouts. *(needs a Windows machine)*
- [ ] In-window menus (Windows has no global menu bar), Windows 11 conventions for title bar and menus. *(needs a Windows machine)*
- [ ] Tray icon and notifications. *(needs a Windows machine)*

### M7-05 · Distribution (deferred with the other release work)
- [ ] Installer (MSI/NSIS), signing (SignPath OSS or Azure Trusted Signing), winget and Scoop.
