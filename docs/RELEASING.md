# Releasing Teitunnel

How releases work (D-074) and the one-time setup they need. The workflow is
`.github/workflows/release.yml`; the website's download pages follow the latest release
(D-076).

## Day to day

1. Merge work into `main` with Conventional Commit messages (`feat:`, `fix:` …).
2. release-please keeps a pull request **"chore: release x.y.z"** up to date: the version in
   `apps/desktop/package.json`, `Cargo.toml` and `Cargo.lock`, and `CHANGELOG.md`.
3. To release, review and merge that pull request. The workflow then:
   - tags `vx.y.z` and creates a **draft** release;
   - builds macOS (universal), Windows (x64, arm64) and Linux (x64, arm64: `.deb`, `.rpm`,
     AppImage), plus the CLI for each;
   - signs and notarizes the Mac app and disk image, signs every update, writes
     `latest.json`, `SHA256SUMS.txt` and build provenance;
   - attaches everything and **publishes** the release (only then do the updater and the
     website see it), then rebuilds the website;
   - publishes the Docker image (`image.yml`) and submits the new version to winget.
   Within three hours the Homebrew tap picks it up by itself (see Channels).
4. A hand-written `docs/release-notes/vx.y.z.md` replaces the generated notes when present.

**Dry run:** Actions ▸ Release ▸ Run workflow (on `main`) builds and signs everything and
keeps it as a workflow artifact, without releasing. Pull requests that change the release
setup get an unsigned dry run automatically.

The first release, **0.1.0**, was published on 2026-09-24. From then on release-please picks
the next version from the commits since the last release: a `fix:` makes 0.1.1, a `feat:` makes
0.2.0, and a breaking change also makes 0.2.0 while the version is below 1.0
(`bump-minor-pre-major`). To force a version, set `release-as` for the package in
`release-please-config.json` and remove it again after that release.

Commits that only touch `apps/web/` or `docs/` never cause a release (`exclude-paths`): the
website deploys on its own when it changes, and an app release with no app changes would only
send users an update that does nothing.

## Channels

| Channel | How it follows releases |
|---|---|
| [Download page](https://teitunnel.teispace.com/download/) and GitHub Releases | Rebuilt by the release workflow. |
| Docker `ghcr.io/teispace/teitunnel` (`linux/amd64`, `linux/arm64`) | `image.yml`, started after publishing: built from the release's own `teitunnel-cli` once its checksums and provenance check out, tagged `x.y.z`, `x.y` and `latest`, with an SBOM and its own provenance. Run it by hand (tag `vx.y.z`) to republish. |
| Homebrew [`teispace/homebrew-tap`](https://github.com/teispace/homebrew-tap): cask `teitunnel`, formula `teitunnel-cli` | The tap's `teitunnel.yml` checks every three hours, takes the checksums from the release's `SHA256SUMS.txt` after verifying its provenance, installs and tests on macOS and Linux, then pushes. No secret needed. GitHub pauses scheduled workflows in a repository with no commits for 60 days: if that happens, re-enable it under the tap's Actions tab (a release commit keeps it alive). The tap is shared by Teispace apps: each app has its own `scripts/<app>.mjs` and `.github/workflows/<app>.yml`. |
| winget `Teispace.Teitunnel` | The first version is submitted by hand (step 7). After it's accepted, the release workflow's `winget` job submits each new version with Komac, using `WINGET_TOKEN`. |

## Setup status

| Step | State |
|---|---|
| `release` environment (deploys from `main` and `v*` tags only) | Done 2026-09-23 |
| Updater key secrets (`TAURI_SIGNING_PRIVATE_KEY`, `…_PASSWORD`) | Done 2026-09-23; key, password and public key backed up by the maintainer. |
| GitHub Pages (Actions, domain `teitunnel.teispace.com`), `DEPLOY_DOCS=true`, Enforce HTTPS | Done 2026-09-23 (the certificate came after removing and re-adding the custom domain; GitHub renews it). |
| Cloudflare DNS `teitunnel` CNAME → `teispace.github.io` (DNS only) | Done 2026-09-23 |
| Org-verified Pages domain `teispace.com` (TXT `_github-pages-challenge-teispace`) | Done 2026-09-23 (blocks other accounts' Pages from claiming it) |
| Apple Developer ID certificate + notarization key | Created 2026-09-23: Developer ID Application (G2), valid to 2031-09-17; API key F3NQ9BSCDM (Developer role). Backed up by the maintainer (password manager); no copies on disk. Secrets stored. |
| Channels: ghcr.io image, Homebrew tap | Done 2026-09-24 (the image package's visibility must be **Public** in the organization's Packages settings once, after the first push) |
| winget | First submission: step 7; then `WINGET_TOKEN` |
| SignPath Foundation for Windows | Declined 2026-09-24: not enough reputation yet. Reapply once Teitunnel is better known (step 6). Windows builds are unsigned. |

## One-time setup (maintainer)

### 1. The `release` environment

Settings ▸ Environments ▸ **New environment** `release`:
- **Deployment branches and tags:** selected branches and tags: `main` and `v*`. This keeps
  the secrets away from other branches.
- Optionally **Required reviewers**: you, to approve every release run.

All secrets below go into this environment, not the repository.

### 2. Updater key (done on the maintainer's Mac)

The keypair was created 2026-09-23 and is kept in the maintainer's password manager (the public key is in
`apps/desktop/src-tauri/tauri.conf.json`).

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY --env release < ~/.tauri/teitunnel/updater.key
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --env release < ~/.tauri/teitunnel/updater.key.password
```

Then **back up both files** in your password manager. If the key is lost, installed copies
can never update again (a new key needs a manual reinstall).

### 3. Apple: Developer ID certificate

Only the **Account Holder** of the Teispace team can create it.

1. Keychain Access ▸ Certificate Assistant ▸ *Request a Certificate From a Certificate
   Authority…*, saved to disk.
2. developer.apple.com ▸ Certificates ▸ **+** ▸ **Developer ID Application** (G2 Sub-CA),
   upload the request, download the certificate and open it (it joins your keychain).
3. In Keychain Access, find *Developer ID Application: Teispace (TEAMID)*, expand it, select
   the certificate **and** its private key, *Export 2 items…* as `.p12` with a strong password.
4. Store it:

```sh
base64 -i DeveloperID.p12 | tr -d '\n' | gh secret set APPLE_CERTIFICATE --env release
gh secret set APPLE_CERTIFICATE_PASSWORD --env release        # the .p12 password
gh secret set APPLE_SIGNING_IDENTITY --env release --body "Developer ID Application: Teispace (TEAMID)"
```

Delete the `.p12` afterwards (keep it only in your password manager).

### 4. Apple: notarization key

App Store Connect ▸ Users and Access ▸ Integrations ▸ **App Store Connect API** ▸ Team Keys ▸
**+**, access **Developer**. Download the `.p8` (once only) and note the Key ID and Issuer ID.

```sh
gh secret set APPLE_API_KEY --env release --body "<Key ID>"
gh secret set APPLE_API_ISSUER --env release --body "<Issuer ID>"
gh secret set APPLE_API_KEY_P8 --env release < AuthKey_<KeyID>.p8
```

**Slow notarization.** A release submits three things to Apple: the app (by Tauri), the DMG
and the CLI. A new team's first submissions go through a deeper check that can take hours;
later ones take minutes. The macOS job waits up to 3 hours per file and fails with Apple's log
if one is rejected (`timeout-minutes: 300` on the job). To see Apple's side, with the key
from the password manager:

```sh
xcrun notarytool history --key AuthKey_<KeyID>.p8 --key-id <Key ID> --issuer <Issuer ID>
xcrun notarytool log <submission id> --key AuthKey_<KeyID>.p8 --key-id <Key ID> --issuer <Issuer ID>
```

### 5. Website

- Settings ▸ Pages ▸ Source: **GitHub Actions**; custom domain `teitunnel.teispace.com`,
  **Enforce HTTPS** once the certificate is issued.
- Settings ▸ Variables ▸ `DEPLOY_DOCS` = `true`.
- Cloudflare DNS for `teispace.com`: `CNAME teitunnel → teispace.github.io`, **DNS only**
  (grey cloud) so GitHub can issue the certificate.

### 6. Windows signing (reapply later)

The first application was declined on 2026-09-24 for lack of popularity, and the site's
code signing policy page and signing notes were removed (D-085). To reapply, restore
`apps/web/app/(home)/code-signing/` and its links from git history (commit `c513c0c`),
then follow the checklist below.

The application (https://signpath.org/apply) needs a person: it creates a SignPath account in
the applicant's name, has a CAPTCHA and asks to accept SignPath Foundation's code of conduct.
What SignPath checks (https://signpath.org/terms), and where Teitunnel meets it:

- OSI license, no proprietary code, released, documented: MIT, v0.1.0, the website and docs.
- "Code signing policy" on the home and download pages, with SignPath's attribution line,
  team roles with links, and the privacy statement: `/code-signing/` (footer and download page).
- Uninstall instructions: docs, Install ▸ Uninstall.
- MFA for every team member on GitHub and SignPath; turn on **Require two-factor
  authentication** in the organization's Authentication security settings.
- Reviews for outside changes: the `main` ruleset (D-082).
- Signed artifacts built on CI from the repository, each release approved by hand.

SignPath grants certificates only to projects with some verifiable reputation (downloads,
stars, coverage); a very new project may be asked to come back later. When accepted: connect
the repository in SignPath, add the signing step where `release.yml` marks it (before the
updater signatures, so updates carry the signed installer), set product name and version
restrictions in the artifact configuration, and change the Windows line on the code signing
policy page from pending to signed.

### 7. winget

The first version of `Teispace.Teitunnel` is a pull request to
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) with the three manifests
(version, installer, `en-US` locale; the installer is per-user NSIS, `ProductCode`
`Teitunnel`). Microsoft's bots validate it and a moderator merges it, usually within a few
days. Unsigned installers are accepted; SmartScreen still warns until SignPath signing.

After it's merged, for automatic updates: create a **classic** personal access token with
only the `public_repo` scope (Komac forks winget-pkgs into that account and opens the pull
request from there), then

```sh
gh secret set WINGET_TOKEN --env release
```

Without the secret the release workflow skips winget with a warning.

