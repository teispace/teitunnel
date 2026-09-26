# Contributor documentation

How Teitunnel is built, and the standards changes are held to. For using Teitunnel, see the
[user documentation](https://teitunnel.teispace.com/docs/).

| Document | Read it when |
|---|---|
| [ARCHITECTURE.md](ARCHITECTURE.md) | You change anything beyond one screen: the crates and their boundaries, the plan → apply engine, the runtime, persistence, IPC and the frontend. |
| [CONVENTIONS.md](CONVENTIONS.md) | Always: Rust and TypeScript style, naming, which tests a change needs, commits and docs. |
| [DESIGN.md](DESIGN.md) | You touch the UI: the native design system, tokens, components, motion, copy and the UI review checklist. |
| [SECURITY_MODEL.md](SECURITY_MODEL.md) | You touch credentials, processes, DNS, the network, the inspector, the MCP server or the release pipeline. |
| [RELEASING.md](RELEASING.md) | You maintain releases: how a version is cut and published, and the secrets it needs. |
| [release-notes/](release-notes) | Hand-written notes for each release, used in place of the generated ones. |

Start with the [contributing guide](../CONTRIBUTING.md) for setup, workflow and pull requests.
Proposals and design discussions happen in
[issues labelled `proposal`](https://github.com/teispace/teitunnel/labels/proposal) and in
[Discussions](https://github.com/teispace/teitunnel/discussions).
