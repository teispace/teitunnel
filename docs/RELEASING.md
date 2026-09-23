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
     website see it), then rebuilds the website.
4. A hand-written `docs/release-notes/vx.y.z.md` replaces the generated notes when present.

**Dry run:** Actions ▸ Release ▸ Run workflow (on `main`) builds and signs everything and
keeps it as a workflow artifact, without releasing. Pull requests that change the release
setup get an unsigned dry run automatically.

The first release is **0.1.0** (`release-as` in `release-please-config.json`; remove that line
after it's out).

## Setup status

| Step | State |
|---|---|
| `release` environment (deploys from `main` and `v*` tags only) | Done 2026-09-23 |
| Updater key secrets (`TAURI_SIGNING_PRIVATE_KEY`, `…_PASSWORD`) | Done 2026-09-23; key, password and public key backed up by the maintainer. |
| GitHub Pages (Actions, domain `teitunnel.teispace.com`), `DEPLOY_DOCS=true`, Enforce HTTPS | Done 2026-09-23 (the certificate came after removing and re-adding the custom domain; GitHub renews it). |
| Cloudflare DNS `teitunnel` CNAME → `teispace.github.io` (DNS only) | Done 2026-09-23 |
| Org-verified Pages domain `teispace.com` (TXT `_github-pages-challenge-teispace`) | Done 2026-09-23 (blocks other accounts' Pages from claiming it) |
| Apple Developer ID certificate + notarization key | Created 2026-09-23: Developer ID Application (G2), valid to 2031-09-17; API key F3NQ9BSCDM (Developer role). Backed up by the maintainer (password manager); no copies on disk. Secrets stored. |
| SignPath Foundation for Windows | After 0.1.0: step 6 |

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

### 5. Website

- Settings ▸ Pages ▸ Source: **GitHub Actions**; custom domain `teitunnel.teispace.com`,
  **Enforce HTTPS** once the certificate is issued.
- Settings ▸ Variables ▸ `DEPLOY_DOCS` = `true`.
- Cloudflare DNS for `teispace.com`: `CNAME teitunnel → teispace.github.io`, **DNS only**
  (grey cloud) so GitHub can issue the certificate.

### 6. Windows signing (after 0.1.0)

Apply to SignPath Foundation (https://signpath.org) once 0.1.0 is public. The website already
has the code signing policy and privacy pages it asks for. When accepted, add the SignPath
step where `release.yml` marks it (before the updater signatures), and update the code
signing policy page.
