# Contributing to Teitunnel

Thanks for helping. Teitunnel aims to be the most pleasant way to use Cloudflare Tunnel, so both code quality and UI detail matter a lot here.

## Before you start
- Read [docs/README.md](docs/README.md). The key documents are ARCHITECTURE, DESIGN and CONVENTIONS.
- Check [docs/STATUS.md](docs/STATUS.md) and the [roadmap](docs/ROADMAP.md) for what's being worked on.
- For anything non-trivial, open an issue or discussion first so we can agree on the approach.

## Development setup
Prerequisites: macOS (primary platform), Rust stable (see `rust-toolchain.toml`), Node 24+, pnpm.

```bash
git clone https://github.com/teispace/teitunnel.git
cd teitunnel
pnpm install
pnpm dev
```

> The project is in its rewrite phase. Setup commands become available as milestone M0 lands.

## Making changes
- Branch: `<type>/<task-id>-<slug>`, e.g. `feat/m1-04-log-parser`.
- Follow [CONVENTIONS.md](docs/CONVENTIONS.md). Include tests. Update docs.
- UI changes: follow [DESIGN.md](docs/DESIGN.md), and attach light and dark screenshots (active and inactive window).
- Commits: [Conventional Commits](https://www.conventionalcommits.org/).
- Run `pnpm check && pnpm test` before pushing.

## Pull requests
- Keep them focused. Link the task ID or issue.
- Explain *why*, not only *what*.
- CI must be green. PRs are squash-merged.

## Code of conduct
This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
