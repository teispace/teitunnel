# Distribution: formats, signing, updates, releases

Verified 2026-09-23 unless noted. Decision: D-074.

## Download pages we modelled
- **claude.com/download**: separate buttons per platform ("Download for macOS", "Download for Windows", "Windows (arm64)"); macOS is one universal `.dmg`, Windows `.exe` setup per architecture, Linux (beta) for Ubuntu/Debian x64 and arm64. Links go through `…/desktop/<os>/<arch>/<format>/latest/redirect`. No version shown.
- **slack.com/downloads/mac**: one Download button plus the Mac App Store, current version and "What's new" shown, links to the Windows and Linux pages; the download link goes to an instructions page (`/downloads/instructions/mac`) that starts the download and explains installing.

## Tauri bundles and the updater (tauri 2.11, tauri-plugin-updater 2.12.0)
- Updater artifacts (`bundle.createUpdaterArtifacts: true`): macOS `.app.tar.gz` + `.sig`; Windows NSIS/MSI + `.sig`; Linux AppImage + `.sig`. Source: v2.tauri.app/plugin/updater.
- The plugin also installs `.deb` (`dpkg -i`) and `.rpm` updates, elevating with `pkexec`, then zenity/kdialog (source: `plugins/updater/src/updater.rs`, `install_deb`/`install_rpm`); the bundle type is baked into the binary and matched against installer-specific keys.
- `latest.json` platform keys are `OS-ARCH` (`darwin-aarch64`, `darwin-x86_64`, `windows-x86_64`, `windows-aarch64`, `linux-x86_64`, `linux-aarch64`), with installer-specific variants (e.g. `windows-x86_64-nsis`, `linux-x86_64-deb`). Each needs `url` and `signature`.
- Keys: `TAURI_SIGNING_PRIVATE_KEY` (+ `_PASSWORD`); losing the key means installed apps can never update again.

## Windows code signing
- Since 2024 EV certificates get no SmartScreen head start; reputation builds per certificate/file for OV and EV alike (v2.tauri.app/distribute/sign/windows).
- Tauri signs through `bundle.windows.signCommand` (any command, `%1` = file) or a local certificate thumbprint.
- **Microsoft Artifact Signing** (formerly Trusted Signing): Public Trust for organizations in the US, Canada, EU, UK, Australia, New Zealand, Japan, South Korea, Singapore, Switzerland, Norway, Israel; individuals in the US/Canada only; paid Azure subscription (learn.microsoft.com/azure/artifact-signing/quickstart, updated 2026-09-18). **Not available to Teispace (Nepal).**
- **SignPath Foundation** (signpath.org/terms): free for OSI-licensed projects without proprietary parts; the certificate names SignPath Foundation; MFA for all team members; roles (authors, reviewers, approvers); manual approval of every release; binaries built verifiably from source on CI; a public "Code signing policy" page with attribution, team roles and a privacy statement ("This program will not transfer any information to other networked systems unless specifically requested"); the project must already be released in the form to be signed.

## Tooling status (GitHub, 2026-09-23)
- `googleapis/release-please` v17.11.2 (action v5.0.0), `tauri-apps/tauri-action` action-v1.0.0, `axodotdev/cargo-dist` v0.33.0, `release-plz` 0.3.169: all maintained.
- A tag pushed with the workflow's `GITHUB_TOKEN` doesn't trigger other workflows, so builds run in the release-please workflow itself.
- GitHub `releases/latest` skips pre-releases.
