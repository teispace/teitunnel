# Releasing Teitunnel

How a release is made and published. This is for maintainers; contributors only need to
use [Conventional Commits](https://www.conventionalcommits.org/), which decide the next
version and the changelog. The workflow is `.github/workflows/release.yml`.

## Day to day

1. Merge work into `main` with Conventional Commit messages (`feat:`, `fix:` …).
2. release-please keeps a pull request **"chore: release x.y.z"** up to date: the version in
   `apps/desktop/package.json`, `Cargo.toml` and `Cargo.lock`, and `CHANGELOG.md`.
3. To release, review and merge that pull request. GitHub doesn't run workflows for
   pull requests that release-please opens or updates with the workflow's own token, so
   **close and reopen it** (or `gh pr close N && gh pr reopen N`) to run CI before merging.
   The workflow then:
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

release-please picks the next version from the commits since the last release: before 1.0,
a `fix:` bumps the patch version, and a `feat:` or a breaking change bumps the minor version
(`bump-minor-pre-major`). The workspace crates' versions in `Cargo.lock` are the packages
without a `source` (`$.package[?(!@.source)].version`). To force a version, set `release-as`
for the package in `release-please-config.json` and remove it again after that release.

Commits that only touch `apps/web/` or `docs/` never cause a release (`exclude-paths`): the
website deploys on its own when it changes, and an app release with no app changes would only
send users an update that does nothing.

## Channels

| Channel | How it follows releases |
|---|---|
| [Download page](https://teitunnel.teispace.com/download/) and GitHub Releases | Rebuilt by the release workflow. |
| Docker `teispace/teitunnel` on Docker Hub and `ghcr.io/teispace/teitunnel` (`linux/amd64`, `linux/arm64`) | `image.yml`, started after publishing (Docker Hub gets a copy of the ghcr index, same digest, with `DOCKERHUB_USERNAME` and `DOCKERHUB_TOKEN` from the `release` environment; the overview is `docker/README.md`, pasted into Docker Hub by hand when it changes): built from the release's own `teitunnel` once its checksums and provenance check out, tagged `x.y.z`, `x.y` and `latest`, with an SBOM and its own provenance. Run it by hand (tag `vx.y.z`) to republish. |
| Homebrew [`teispace/homebrew-tap`](https://github.com/teispace/homebrew-tap): cask `teitunnel`, formula `teitunnel` | The tap's `teitunnel.yml` checks every three hours, takes the checksums from the release's `SHA256SUMS.txt` after verifying its provenance, installs and tests on macOS and Linux, then pushes. No secret needed. GitHub pauses scheduled workflows in a repository with no commits for 60 days: if that happens, re-enable it under the tap's Actions tab (a release commit keeps it alive). The tap is shared by Teispace apps: each app has its own `scripts/<app>.mjs` and `.github/workflows/<app>.yml`. |
| apt and dnf repositories at `teitunnel.teispace.com/linux/` | Rebuilt with the website (`docs.yml`, which the release workflow runs after publishing): the two latest releases' `.deb` and `.rpm`, checked against checksums and provenance, repository metadata and RPMs signed with the key in `LINUX_REPO_GPG_KEY` (`scripts/release/linux-repo.sh`). The public key is `apps/web/public/linux/teitunnel.asc`. |
| winget `Teispace.Teitunnel` | The first version is submitted by hand (see [winget](#winget)). After it's accepted, the release workflow's `winget` job submits each new version with Komac, using `WINGET_TOKEN`. |

## Secrets and variables

Release secrets live in the `release` environment, which deploys only from `main` and `v*`
tags, never in the repository's own secrets.

| Name | Kind | Used for |
|---|---|---|
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` | secret | Signing the macOS app with a Developer ID Application certificate |
| `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_P8` | secret | Notarizing with an App Store Connect API key |
| `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | secret | Signing updates; the public key is built into the app |
| `LINUX_REPO_GPG_KEY` | secret | Signing the apt and dnf repositories |
| `DOCKERHUB_TOKEN` | secret | Copying the image to Docker Hub |
| `WINGET_TOKEN` | secret | Submitting new versions to winget (optional) |
| `DOCKERHUB_USERNAME` | variable | The Docker Hub account |
| `DEPLOY_DOCS` | variable | `true` deploys the website from `main` |

Without a secret, the step that needs it is skipped with a warning, or the build stays
unsigned in a dry run.

Windows builds aren't code-signed yet, so SmartScreen warns on first launch.

## winget


The first version of `Teispace.Teitunnel` is a pull request to
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs) with the three manifests
(version, installer, `en-US` locale; the installer is per-user NSIS, `ProductCode`
`Teitunnel`). Microsoft's bots validate it and a moderator merges it, usually within a few
days. Unsigned installers are accepted; SmartScreen still warns until the installers are signed.

After it's merged, for automatic updates: create a **classic** personal access token with
only the `public_repo` scope (Komac forks winget-pkgs into that account and opens the pull
request from there), then

```sh
gh secret set WINGET_TOKEN --env release
```

Without the secret the release workflow skips winget with a warning.


## Linux repository key


Once, from the repository root (needs `gpg` and `gh`):

```sh
scripts/release/linux-repo-key.sh
```

It creates an RSA 4096 signing key without expiry (every apt and rpm version can check it,
and installed machines never need a new key), stores the private key as `LINUX_REPO_GPG_KEY`
in the `release` environment, writes the public key to `apps/web/public/linux/teitunnel.asc`
(commit it), and leaves `teitunnel-linux-repo.key.asc` (git-ignored): move it to your
password manager and delete it. If the key is lost, publish a new public key and every user
has to fetch it again; if it leaks, do the same at once.


## Browser extension stores


`integrations/browser` builds with `pnpm --filter @teitunnel/browser-extension build` into
`dist/chrome` and `dist/firefox`; zip each folder's contents.

- **Chrome Web Store** (also serves Brave, Arc, Vivaldi): upload `dist/chrome`. The store keeps
  its own key, so the published extension gets a new id: add it to
  `CHROMIUM_EXTENSION_IDS` in `crates/core/src/browser_host.rs` (next to the unpacked one, from
  the manifest's `key`), ship an app release, and users choose **Set Up** again (or it's
  rewritten at the next **Set Up**). Optionally put the store's public key in
  `static/manifest.json` `key` so unpacked builds share the store id.
- **Edge Add-ons**: upload the same package; its id is different again: add it too.
- **Firefox Add-ons**: upload `dist/firefox`; the id is fixed (`browser@teitunnel.teispace.com`).

The extension only asks for `nativeMessaging` and `activeTab`; the listing explains that it
works with the Teitunnel app, which must be installed and set up (Settings ▸ Integrations).
