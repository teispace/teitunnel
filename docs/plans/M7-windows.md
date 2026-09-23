# M7: Windows

**Goal:** Teitunnel on Windows 11 with the same features as macOS, feeling like a Windows app.
**Release:** v1.1.
**Verification:** CI builds and runs every Rust test on `windows-latest`. Anything that needs a Windows desktop (look, feel, installer, real Task Scheduler) is marked *(needs a Windows machine)*.

### M7-01 · Processes and services
- [x] The E2E suite (Quick Share; account → verified route → remove) runs on Windows in CI (`e2e-windows` job; cross-platform `e2e:build` script), green.
- [x] Helper programs start without a console window (`CREATE_NO_WINDOW`, `cloudflared::process::no_console`), for connectors, version checks and `schtasks`.
- [x] Always-on via Task Scheduler: per-user task at logon, restart on failure, no time limit, UTF-16 task XML, `CommandLineToArgvW` quoting (`cloudflared::task_scheduler`, `core::service::TaskScheduler`, D-051). Selected at runtime on Windows (D-054).
- [x] Logs for services the manager can't capture: the connector writes its own rotating log (`--log-directory`, 1 MB × 5, verified in cloudflared's `logger/`), which the app tails (D-054).
- [ ] Try it on a real Windows 11 machine: install, switch modes, reboot, uninstall. *(needs a Windows machine)*
- [ ] Graceful connector stop: `CTRL_BREAK` needs a shared console, which a GUI app doesn't have; evaluate a helper or keep `TerminateProcess` (the edge reroutes within seconds). *(needs a Windows machine)*
- [x] Token file ACL: the per-user profile ACL is kept (only the user, SYSTEM and Administrators read `AppData`); an explicit ACL would need `unsafe` Win32 FFI (D-063).

### M7-02 · Binary
- [x] Managed binary `cloudflared-windows-amd64.exe` (asset table), SHA-256 verified against the release checksums.
- [ ] Authenticode verification with Cloudflare as the signer, like `codesign` on macOS. *(The binary is signed by "Cloudflare, Inc." via DigiCert's G4 code-signing CA (research/cloudflare.md). Checking it needs `WinVerifyTrust` + the signer's certificate (Win32 FFI, so `unsafe`), which the workspace forbids; PowerShell's `Get-AuthenticodeSignature` would break the no-shell rule. **Maintainer decision:** allow a tiny isolated crate with audited `unsafe` for this, or rely on the release checksums alone on Windows. The SHA-256 check already applies.)*

### M7-03 · Credentials
- [x] Windows Credential Manager via `keyring` (`windows-native-keyring-store`, selected by default).

### M7-04 · Shell and design
- [x] Native title bar and caption controls (snap layouts), opaque window (`tauri.windows.conf.json`), Segoe UI Variable, Windows 11 neutral surfaces, accent, navigation selection with the accent pill (D-063).
- [ ] Real Mica backdrop: needs a transparent window and a Windows 11 check (Windows 10 would show the desktop through it). *(needs a Windows machine)*
- [x] No menu bar: Ctrl shortcuts in the window, Help links and Quit in the command palette; Windows wording (this PC, File Explorer, Credential Manager, notification area, winget) (D-063).
- [ ] Check it all on Windows 11. *(needs a Windows machine)*
- [x] Tray: left click opens the app, right click the menu; closing the window quits when the tray icon is hidden (D-063).
- [ ] Check the tray and notifications on Windows 11. *(needs a Windows machine)*

### M7-05 · Distribution (deferred with the other release work)
- [ ] Installer (MSI/NSIS), signing (SignPath OSS or Azure Trusted Signing), winget and Scoop.
