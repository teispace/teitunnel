# Teitunnel Preview (GitHub Action)

Pull request previews on **your own domain**, served from **your own Cloudflare account**:
`https://pr-42.preview.example.com`, commented on the pull request, updated on every push,
and removed when the pull request closes.

Two kinds of preview:

- **Snapshot** (default): a static copy of your built site, hosted as a Worker with static
  assets on your account. It stays up after the job, so reviewers can open it any time.
  Each push publishes a new version; only changed files are uploaded.
- **Share**: a live tunnel to a server the job started (an API, a server-rendered app, a
  dev server), for as long as the job runs. Handy for end-to-end tests against a real
  hostname, webhooks, or a review while the job waits.

Every change goes through Teitunnel's plan → apply engine, the same one the app and the
`teitunnel` CLI use: nothing Teitunnel didn't create is ever changed, and a hostname a
teammate holds (their reservation, or their machine's route) is never taken.

## Quick start

```yaml
# .github/workflows/preview.yml
name: Preview
on:
  pull_request:
    types: [opened, synchronize, reopened, closed]

permissions:
  contents: read
  pull-requests: write # the comment

jobs:
  publish:
    if: github.event.action != 'closed'
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v5
      - run: npm ci
      - uses: teispace/teitunnel-action@v1
        with:
          cloudflare-api-token: ${{ secrets.CLOUDFLARE_API_TOKEN }}
          build: true
          zone: example.com

  cleanup:
    if: github.event.action == 'closed'
    runs-on: ubuntu-latest
    steps:
      - uses: teispace/teitunnel-action@v1
        with:
          cloudflare-api-token: ${{ secrets.CLOUDFLARE_API_TOKEN }}
          mode: cleanup
          zone: example.com
```

A live share of a server the job started:

```yaml
      - run: npm run start -- --port 3000 &
      - run: npx wait-on http://localhost:3000
      - id: preview
        uses: teispace/teitunnel-action@v1
        with:
          cloudflare-api-token: ${{ secrets.CLOUDFLARE_API_TOKEN }}
          mode: share
          port: 3000
          zone: example.com
      - run: npx playwright test
        env:
          BASE_URL: ${{ steps.preview.outputs.url }}
```

The share stays up until the job ends; the action's post step (which runs even when a step
failed) stops it, removes its DNS record and deletes the job's tunnel.

More in [`examples/`](examples): the workflow above and a
[reusable workflow](examples/reusable-preview.yml) other repositories can call.

## Inputs

| Input | Default | |
|---|---|---|
| `cloudflare-api-token` | (required) | A Cloudflare API token, from a secret. Account-owned tokens work (and are recommended for CI). |
| `account` | | The account's name or id, when the token reaches several. |
| `mode` | `snapshot` | `snapshot`, `share` or `cleanup`. |
| `port` / `url` | | Share mode: the server the job started (`3000`, or `http://localhost:8080`). |
| `path` | `.` | Snapshot mode: the folder to publish, or the project with `build`. |
| `build` | `false` | Snapshot mode: build the project first (its package manager runs its build script; Next.js needs `output: 'export'`). |
| `hostname` | `pr-{number}.preview.{zone}` | The hostname template (below). |
| `zone` | | One of the account's domains, for `{zone}`. |
| `name` | `{repo}-pr-{number}` | Snapshot mode: its name (the Worker is `teitunnel-<name>`). Keep it unique per repository and pull request. |
| `expires` | | End by itself: share `30m`, `2h`; snapshot `7d`. |
| `password` | | Snapshot mode: a password visitors type (pass a secret; only a salted hash reaches Cloudflare). |
| `allow` | | Require a Cloudflare Access login for these emails or `@domains` (comma or newline separated). |
| `comment` | `true` | Comment the URL on the pull request. |
| `github-token` | `${{ github.token }}` | For the comment and for downloading the CLI. |
| `version` | `latest` | The Teitunnel CLI version to install. |
| `cli-path` | | Use this `teitunnel` binary instead of installing one (self-hosted runners). |
| `wait` | `120` | Share mode: seconds to wait for the share to be live. |

### Hostname templates

| Placeholder | Becomes |
|---|---|
| `{number}` | The pull request number (runs for other events can't use it). |
| `{branch}` | The head branch, e.g. `feat/login` → `feat-login`. |
| `{sha}` | The first 7 characters of the head commit. |
| `{repo}`, `{owner}` | The repository and its owner. |
| `{zone}` | The `zone` input, as given. |

Every value is made a DNS label: lowercase letters, digits and dashes, at most 63
characters. Several repositories sharing a zone should include `{repo}`, e.g.
`pr-{number}.{repo}.{zone}`.

## Outputs

| Output | |
|---|---|
| `url` | The preview's URL (empty in cleanup mode). |
| `hostname` | Its hostname. |

## The comment

One comment per preview hostname, found again by a hidden marker
(`<!-- teitunnel-preview:<hostname> -->`) and edited in place on every push, so a pull
request never collects a stack of them. A share's comment says when the job ended; a
cleanup says the preview was removed.

## Permissions and security

- **The token.** Create one at *Cloudflare ▸ Manage account ▸ Account API tokens*
  (account-owned, not tied to a person) with:
  - Account · Cloudflare Tunnel · Edit, Zone · DNS · Edit, Zone · Zone · Read (share mode);
  - Account · Workers Scripts · Edit and Zone · Workers Routes · Edit (snapshot mode);
  - Account · Access: Apps and Policies · Edit (only with `allow`).

  Limit it to the zones previews use. Store it as a repository or environment secret; the
  action masks it, passes it to the CLI through the environment only (never in arguments),
  and it's never written to disk.
- **The workflow token** needs `pull-requests: write` for the comment. Without it the
  preview still works and the comment is skipped with a warning.
- **Pull requests from forks don't get secrets** on `pull_request`, so their previews are
  skipped. Don't switch to `pull_request_target` to get around it: that runs with your
  secrets and a write token, and building or serving the fork's code there hands both to
  whoever opened the pull request. If you must preview forks, have a maintainer apply a
  label after reviewing the code, run on `pull_request_target` only for that label, check
  out the reviewed commit by SHA, and keep the token scoped to the preview zone.
- **Names are shared fairly.** The action writes its owner (`github-actions/<repository>`)
  into the DNS records it makes. A hostname a teammate reserved (`teitunnel reserve`) or
  routes from their machine fails the step with exit code 3 and a message saying who
  holds it; the action never takes a name over.
- **Checksums.** The CLI is downloaded from Teitunnel's releases (teispace/teitunnel) and checked against
  the release's `SHA256SUMS.txt` before it runs; cloudflared (share mode) is Cloudflare's
  release, verified by the CLI.

## How it works

The action is a small JavaScript action (Node 24, no dependencies). It installs the
`teitunnel` CLI for the runner's OS and architecture and runs:

- snapshot: `teitunnel snapshot publish <path> --name <name> --on <hostname> --or-update --yes --json`
  (a Snapshot published by an earlier run is found on the account and updated);
- share: `teitunnel share <port> --on <hostname> --json` in the background (the CLI runs
  the tunnel's connector itself, on a tunnel named after the run), then, in the post step,
  stops it and `teitunnel tunnel delete <tunnel> --yes`;
- cleanup: `teitunnel snapshot rm <hostname> --missing-ok --yes`.

It's a JavaScript action rather than a composite one because only JavaScript (and Docker)
actions have a post step, which is what keeps a share alive exactly for the job and
cleans it up even when the job fails. GitLab users: see
[`integrations/gitlab-ci`](../gitlab-ci/teitunnel.gitlab-ci.yml).

## Development

```sh
node --test integrations/github-action/test/
```

`.github/workflows/action-test.yml` also runs the action itself against Teitunnel's
end-to-end fakes (a debug CLI pointed at `fake-cloudflare`, with `fake-cloudflared`) on
pull requests that touch it.
