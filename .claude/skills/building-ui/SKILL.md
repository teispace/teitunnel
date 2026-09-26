---
name: building-ui
description: Builds or changes screens and components in Teitunnel's React desktop UI (apps/desktop/src) so they feel native on macOS, Windows and Linux, using the design system's tokens, primitives and patterns, TanStack Query data hooks, accessibility, light and dark screenshots and the component gallery. Use for any visual or interaction change in the desktop app.
---

# Building UI

Teitunnel should feel like it shipped with the operating system. The rules are in
[docs/DESIGN.md](../../../docs/DESIGN.md); read §1 (hard rules) before any UI change, and the
section for what you touch (§4 colour, §7 motion, §8 components, §10 copy, §11 accessibility).

## Where things are

```
src/components/ui/        primitives: Button, Sheet, Dialog, Switch, Select, SegmentedControl…
src/components/patterns/  patterns: ListPane, SplitView, GroupedList, EmptyState, ErrorState,
                          CopyField, KeyValueGrid, LogViewer, TimeSeriesChart, ConfirmDialog…
src/features/<area>/      a feature: components/, queries.ts, model.ts (pure), index.ts
src/routes/               TanStack Router file routes
src/styles/tokens.css     every colour, radius and shadow, light and dark
src/dev/                  the gallery (/dev/gallery) and the IPC mocks
```

## Rules

- **Reuse before you build.** Look in `components/ui` and `components/patterns` first. A new
  primitive or pattern gets a gallery entry in `src/dev/gallery.tsx`.
- **Semantic tokens only**: Tailwind classes mapped to tokens (`bg-surface-content`,
  `text-secondary`). No hex, `rgb()`, arbitrary colours or static `style={{}}`.
- **Never**: gradients, glows, fake glass, emoji, a hand cursor on buttons or rows, a modal
  for primary work, a spinner for local state, layout shift while loading.
- **Data only through hooks** in `features/<area>/queries.ts` (see `adding-ipc-commands`).
  Server state in TanStack Query, UI state in Zustand or the URL, form state in the
  component (`useState`), validated in the feature's `model.ts`.
  No `useEffect` for fetching or derived state.
- **Text** from `locales/en.json` through `t()` (see `writing-user-facing-text`).
- **Keyboard**: every action is reachable from the keyboard; frequent ones get a menu item
  and shortcut. Icon-only controls have an `aria-label`. Status is never shown by colour
  alone.
- **Pure logic in `model.ts`** with unit tests; components stay presentational. Split a file
  that grows past ~250 lines or a second responsibility.
- **Performance**: lists that can grow are virtualized with `@tanstack/react-virtual`, as the Inspector list is; memoize rows
  with stable callbacks; format numbers with `numberFormat()` from `lib/format.ts` (it caches
  `Intl` formatters).
- **Platforms**: macOS gets vibrancy and the native title bar; Windows and Linux use solid
  surfaces (`styles/platform-*.css`). Check both.

## Workflow

```
- [ ] 1. Find the pattern to reuse; sketch states: loading, empty, error, populated
- [ ] 2. Build with primitives and tokens; text from the catalog
- [ ] 3. Mock data in src/dev so the screen renders without a backend
- [ ] 4. Tests: model.ts units, component tests with mockIPC
- [ ] 5. Screenshots in light and dark (and Windows/Linux chrome if it differs)
- [ ] 6. Accessibility audit
- [ ] 7. pnpm verify
```

Screenshots render the real UI on the mocked backend in WebKit:

```sh
pnpm --filter @teitunnel/desktop shoot /tmp/shots /routes
pnpm --filter @teitunnel/desktop shoot /tmp/shots "/routes?platform=windows"
SHOOT_ACTIONS='role=button[name="Add Route"]' pnpm --filter @teitunnel/desktop shoot /tmp/shots /routes
```

`SHOOT_ACTIONS`, `SHOOT_FILL`, `SHOOT_THEN`, `SHOOT_KEYS`, `SHOOT_SIZE`,
`SHOOT_CONTRAST=more` and `SHOOT_REDUCED_MOTION=1` are documented at the top of
`apps/desktop/scripts/shoot.ts`. Look at every screenshot: alignment, truncation, contrast in both themes.

```sh
pnpm --filter @teitunnel/desktop a11y /routes     # axe, light and dark, both contrast modes
```

Native materials and window behaviour only show in a packaged build: `pnpm build`.

Attach the light and dark screenshots to the pull request, and go through DESIGN §12
(the UI review checklist).
