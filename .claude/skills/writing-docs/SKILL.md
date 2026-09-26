---
name: writing-docs
description: Writes and updates Teitunnel's documentation, both the user docs on the website (apps/web/content/docs, Fumadocs MDX) and the contributor docs in docs/, including page structure, navigation, components, generated reference pages, screenshots, sourcing third-party facts and the link check. Use when a change affects what users or contributors need to know, or when asked to write a guide, tutorial or reference page.
---

# Writing docs

Two audiences, two places:

| Audience | Where | Published |
|---|---|---|
| People using Teitunnel | `apps/web/content/docs/` (MDX) | teitunnel.teispace.com/docs |
| People changing Teitunnel | `docs/` (Markdown) | GitHub |

A pull request that changes behaviour updates the docs it affects in the same pull request.

## User docs

Sections, each a folder with a `meta.json` that orders its pages:

| Folder | For |
|---|---|
| `getting-started/` | Install, first share, first route, what's new |
| `tutorials/` | Use cases end to end ("Test webhooks on localhost") |
| `concepts/` | How things work (tunnels, changes, DNS ownership) |
| `guides/` | One feature each, complete: app, CLI, limits, troubleshooting |
| `compare/` | Teitunnel next to other tools |
| `reference/` | CLI, permissions, limits, troubleshooting, glossary |

A page:

```mdx
---
title: Stripe webhooks on localhost
description: One sentence that says what the reader gets, used for search and social cards.
---

Intro: what this page helps with, in two or three sentences.

## 1. A step or a topic
```

- Components: `<Callout>` (`type="warn"` for risks), `<Steps>`/`<Step>`, `<Tabs>`/`<Tab>`
  (per platform or per provider), `<Accordions>`/`<Accordion>`, `<Cards>`/`<Card>` for
  "Related".
- Show both ways: the app (**bold UI names**, `▸` between menu levels) and the terminal
  (`teitunnel …`). Examples use `teispace.com` hostnames.
- Link other pages with absolute paths (`/docs/guides/inspector`, with `#heading` anchors).
  The website test fails on any link or anchor that doesn't exist.
- Facts about Cloudflare or other products come from their own documentation: link it, and
  add a "Checked <date>" note where limits or prices may change. Never guess a number.
- Write for someone in the middle of a task: short sentences, the outcome first, exact error
  messages as they appear.

**Generated pages are never edited by hand**: `apps/web/content/docs/reference/cli.mdx` (from the CLI's clap
definitions) and `apps/web/content/docs/reference/permissions.mdx` (from `crates/core/src/accounts`). Regenerate:

```sh
UPDATE_DOCS=1 cargo test -p teitunnel-cli --bin teitunnel-cli docs
UPDATE_DOCS=1 cargo test -p teitunnel-core --test permissions_doc
```

**Screenshots** for the website come from the real UI on mocked data, in light and dark.
`apps/desktop/scripts/shoot-site.ts` renders the set in `apps/web/public/screens/` as WebP
(add a shot to its list; needs `cwebp`); run it from `apps/desktop` with
`node scripts/shoot-site.ts [name…]`. For a one-off, `pnpm --filter @teitunnel/desktop shoot`
with `?clean` on the route hides the developer section.
Show one with `<Screenshot name="routes" alt="…" />`; its `alt` says what's on screen (it's
what image search and screen readers read), and the sitemap and the page's structured data
list it automatically (`apps/web/lib/page-facts.ts`), as they date the page from git.

## Contributor docs

- `ARCHITECTURE.md`: update when a module boundary, data flow or invariant changes.
- `DESIGN.md`: update when a token, component, pattern or UI rule changes.
- `CONVENTIONS.md`: update when a coding or testing convention changes.
- `SECURITY_MODEL.md`: update when anything about secrets, processes, DNS ownership, network
  exposure or the supply chain changes.

Describe how things are, not their history; the reasoning belongs in the issue or pull
request that made the change.

## Check

```sh
pnpm --filter @teitunnel/web test      # links and anchors
pnpm --filter @teitunnel/web build     # every page compiles
pnpm --filter @teitunnel/web dev       # read it in the browser
```
