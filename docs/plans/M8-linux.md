# M8: Linux

**Goal:** Teitunnel on mainstream Linux desktops (GNOME, KDE) with the same features as macOS.
**Release:** v1.2.
**Verification:** CI builds and runs every Rust test on `ubuntu-latest`. Anything that needs a Linux desktop session is marked *(needs a Linux desktop)*.

### M8-01 · Services
- [x] The E2E suite (Quick Share; account → verified route → remove) runs on Linux in CI (`e2e-linux` job; cross-platform `e2e:build` script), green.
- [x] Always-on via `systemd --user`: a unit per tunnel (`Restart=always`, `append:` logs, quoted `ExecStart`), enabled with `enable --now` (`cloudflared::systemd`, `core::service::Systemd`, D-051). Selected at runtime when there's a user session (`XDG_RUNTIME_DIR`) (D-054).
- [x] Explain lingering: user units stop at logout unless `loginctl enable-linger`; the Always-on row says so on Linux rather than enabling it.
- [ ] Try it on real desktops: GNOME and KDE, install, switch modes, log out/in, reboot. *(needs a Linux desktop)*

### M8-02 · Credentials
- [x] Secret Service via `keyring` (`zbus-secret-service-keyring-store`).
- [x] A clear error when no Secret Service is available (`SecretError::Unavailable`: install and unlock GNOME Keyring or KWallet) (D-054).

### M8-03 · Shell and design
- [x] Native GTK decorations and an opaque window (`tauri.linux.conf.json`), solid Adwaita surfaces, Cantarell, no menu bar (Ctrl shortcuts, palette), Linux wording (this computer, keyring, system tray, package manager) (D-063).
- [ ] Check the look and the tray (StatusNotifierItem) on GNOME and KDE. *(needs a Linux desktop)*
- [ ] Design pass against GNOME HIG / KDE conventions. *(needs a Linux desktop)*

### M8-04 · Distribution (deferred with the other release work)
- [ ] AppImage, deb, rpm, Flatpak (Flathub), AUR.
