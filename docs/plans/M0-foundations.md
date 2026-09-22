# M0: Foundations

**Goal:** an empty app that already looks and behaves like a native macOS app, on a workspace, toolchain and CI that every later milestone builds on.
**Release:** none (internal).
**Exit criteria:**
- `pnpm dev` launches a native-looking window: vibrancy sidebar, overlay title bar, system font and accent, light/dark, native menu bar with a working Edit menu.
- The dev gallery shows every primitive and pattern in light and dark.
- A sample typed command and event round-trip through generated bindings.
- CI is green on macOS, Linux and Windows (build + lint + tests). Bindings drift check and `cargo deny` both pass.

---

## Tasks

### M0-01 · Workspace skeleton
- [ ] Root `Cargo.toml`: `[workspace]` with `resolver = "3"`, members `crates/*`, `apps/desktop/src-tauri`, `tools/*`. `[workspace.package]` (edition 2024, license MIT, repository). `[workspace.dependencies]` with pinned versions from `docs/research/platform.md`. `[workspace.lints]` as in CONVENTIONS.
- [ ] `rust-toolchain.toml` (stable channel pinned to the current stable, components `rustfmt`, `clippy`).
- [ ] `pnpm-workspace.yaml` (`apps/*`). Root `package.json` with `packageManager` pinned, `engines.node`, and scripts: `dev`, `build`, `check` (biome + tsc + clippy), `test`, `fmt`, `bindings`.
- [ ] `.editorconfig`, `.gitignore` (target, node_modules, dist, `.DS_Store`, `*.local`), `.node-version`.
- [ ] `biome.json` at root. `rustfmt.toml` (`imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`; nightly-only options excluded).
- **Accept:** `cargo metadata` and `pnpm install` succeed on a clean clone.

### M0-02 · Library crates (empty but real)
- [ ] `crates/cf-api`, `crates/cloudflared`, `crates/core`, each with `lib.rs`, crate docs, an `Error` enum, and one smoke test.
- [ ] `crates/core/src/{domain,engine,runtime,discovery,doctor,store,secrets,platform}.rs` as module roots with doc comments only (no stubs with fake logic).
- [ ] `crates/core/src/secret.rs`: `Secret<T>` with a redacting `Debug`/`Display`, plus unit tests.
- [ ] `tools/fake-cloudflared` binary crate (empty `main` for now; filled in M1).
- **Accept:** `cargo test --workspace` and `cargo clippy --workspace -- -D warnings` pass.

### M0-03 · Tauri app shell (`apps/desktop/src-tauri`)
- [ ] Tauri 2.11.x. `tauri.conf.json`: `identifier: com.teispace.teitunnel`, productName `Teitunnel`, `app.macOSPrivateApi: true`, and the main window per DESIGN §2 (`titleBarStyle: Overlay`, `hiddenTitle`, `trafficLightPosition {x:18,y:18}`, `transparent`, `windowEffects {effects:["sidebar"], state:"followsWindowActiveState"}`, size 1120×720, min 880×560, `visible: false` until the first frame is ready, to avoid a white flash).
- [ ] Strict CSP (SECURITY_MODEL). `capabilities/main.json`: least privilege.
- [ ] Plugins: `single-instance` (focus existing), `window-state`, `opener`, `os`.
- [ ] `lib.rs` wires state and plugins. `shell/window.rs` shows the window after the frontend signals `app_ready`.
- [ ] Tracing: `tracing-subscriber` with env filter plus a rolling file in `<app_data>/logs`, and the redaction layer (unit-tested with sample tokens).
- **Accept:** the window opens with no flash, vibrancy is visible in the sidebar region, and a second launch focuses the first.

### M0-04 · Frontend scaffold (`apps/desktop/src`)
- [ ] Vite 8 + React 19 + TypeScript 7 strict (tsconfig per CONVENTIONS). `@/` alias.
- [ ] Tailwind v4 via `@tailwindcss/vite`. The Tailwind theme maps to semantic tokens (`@theme` referencing CSS variables).
- [ ] TanStack Router (file-based plugin), TanStack Query provider, sonner Toaster (restyled), Zustand for the UI store.
- [ ] Routes: `/` (Overview placeholder with a designed empty state), `/settings`, `/__dev/gallery` (dev only, tree-shaken from release builds).
- [ ] Global CSS: disable text selection except `.selectable`, `cursor: default` everywhere, no rubber-band overscroll on the root, font smoothing, `-webkit-user-drag: none` on images/links.
- **Accept:** `pnpm build` produces initial JS under 150 KB gzip (the shell alone). Biome and tsc are clean.

### M0-05 · Typed IPC pipeline
- [ ] tauri-specta (pinned rc). `ipc/mod.rs` builds the specta `Builder` with commands and events. Debug builds export to `apps/desktop/src/lib/ipc/bindings.ts` with a header comment. `pnpm bindings` runs the exporter via a small `cargo run --bin export-bindings` (or a test) without launching the GUI.
- [ ] Sample command `app_info() -> AppInfo { version, platform, arch, dataDir }` and sample event `EntityChanged { kind, id? }`.
- [ ] `AppError` type (code, message, hint, field) with specta derive. Frontend `lib/ipc/client.ts` unwraps results into typed errors for Query.
- [ ] `lib/ipc/events.ts`: subscribe `EntityChanged`, invalidate via `query-keys.ts`.
- [ ] CI job: regenerate the bindings and `git diff --exit-code`.
- **Accept:** the Overview shows the version and platform from `app_info`. Changing a Rust type without regenerating fails CI.

### M0-06 · Design tokens & platform styling
- [ ] `styles/tokens.css`: every token from DESIGN §3–§5, light + dark via `prefers-color-scheme`, plus a `data-theme` override.
- [ ] Verify the `AccentColor` system colour in WKWebView. If it's unreliable, implement a Rust `platform::accent_color()` (NSColor via objc2) and push it as `--accent` on change (`NSSystemColorsDidChangeNotification`). Record the outcome in DECISIONS.
- [ ] `styles/platform-macos.css`: traffic-light spacing, drag regions, vibrancy-aware sidebar text colours, inactive-window state (`:root[data-window-active="false"]` greys the selection).
- [ ] Emit window focus/blur to set `data-window-active`.
- [ ] Reduced motion / transparency / increased contrast media queries.
- [ ] `styles/motion.css`: spring tokens (DESIGN §7) as CSS `linear()` easings, generated by a small script from the same token source as `lib/motion.ts` (Motion spring configs). One source, two outputs.
- [ ] CSS material tokens for floating controls (DESIGN §2a) with a Reduce-transparency fallback.
- [ ] **Packaged-build soak test** on macOS 27: `tauri build`, then launch the `.app` and check resize storm, first paint, theme switch, sleep/wake, and active/inactive with the sidebar vibrancy. Record the result in `research/platform.md`.
- **Accept:** side-by-side screenshots against System Settings on macOS 27 (light/dark, active/inactive, transparency slider extremes) look consistent. Attach them to the PR.

### M0-07 · UI primitives (`components/ui`)
- [ ] Generate with the shadcn CLI where useful, then **restyle fully** to the tokens (remove shadcn default look): Button, IconButton, Input, TextArea, Field, Select, Combobox, Switch, Checkbox, Radio, SegmentedControl, Tooltip, Popover, Dialog, Sheet, Badge, Kbd, StatusDot, Spinner, ProgressBar, Separator, ScrollArea, Disclosure, Skeleton, Toast styling.
- [ ] Each has a gallery section with all variants and states (default, hover, pressed, focus, disabled, error), in light + dark.
- [ ] Vitest tests for logic-bearing primitives (SegmentedControl keyboard, CopyField).
- **Accept:** DESIGN §12 checklist passes for every primitive.

### M0-08 · Layout patterns (`components/patterns`)
- [ ] AppShell (sidebar + titlebar toolbar + content), Sidebar + SidebarItem (with sections, badges, collapse), SplitView (resizable with persisted sizes, min/max), ListPane + ListRow (keyboard navigation, selection, active/inactive colours), Inspector + InspectorSection, KeyValueGrid, CopyField, EmptyState, ErrorState, TitlebarToolbar.
- [ ] Sidebar IA placeholders: Overview, Routes, Quick Share, Domains, Tunnels, Activity, Doctor. Settings is reached via ⌘, and the gear icon.
- **Accept:** keyboard-only navigation works across the sidebar, list and inspector. Sizes persist across restarts.

### M0-09 · Native menus, tray stub, shortcuts
- [ ] `shell/menu.rs`: full macOS menu bar per DESIGN §9. Menu events are forwarded to the frontend as a typed `MenuAction` event. The frontend `app/shortcuts.ts` handles in-webview shortcuts that aren't menu-bound.
- [ ] `shell/tray.rs`: template icon, static menu (Open Teitunnel, Quit). "Keep running in menu bar" setting stub.
- [ ] Command palette shell (cmdk) on ⌘K, listing navigation commands.
- **Accept:** ⌘C/⌘V/⌘A work in inputs. ⌘, opens Settings. ⌘1–7 switch sections. The tray icon adapts to light/dark.

### M0-10 · Store foundation
- [ ] `core::store`: rusqlite (bundled), WAL, file perms 0600, migrations via `rusqlite_migration`, and a `Store` handle running on a dedicated blocking thread (message passing). The first migration has `settings` only. Later milestones add tables with their features.
- [ ] `settings_get` / `settings_set` commands (typed keys enum; theme override, keep-in-menu-bar).
- [ ] Tests: migration from empty and a migration-idempotency test.
- **Accept:** the theme override persists across restarts.

### M0-11 · CI
- [ ] `.github/workflows/ci.yml`: jobs `rust` (fmt, clippy -D warnings, nextest) on macos-latest, ubuntu-latest (with webkit2gtk deps), windows-latest; `web` (biome ci, tsc, vitest); `bindings` drift; `deny` (cargo-deny); `build-macos` (tauri build unsigned, upload artifact on PRs labelled `build`). Caching via `Swatinem/rust-cache` and pnpm store.
- [ ] `deny.toml` (licenses allowlist: MIT, Apache-2.0, BSD-2/3, ISC, Unicode-3.0, Zlib, MPL-2.0; advisories deny).
- [ ] Bundle size check (the `size-limit`-style script fails if initial JS goes over budget).
- [ ] Renovate/Dependabot config (weekly, grouped).
- **Accept:** a PR shows all jobs green on three OSes.

### M0-12 · Repo hygiene
- [ ] Lefthook pre-commit: biome check on staged files and `cargo fmt --check`. Fast, under 3 s.
- [ ] Issue forms (bug with diagnostics field, feature request), PR template with the checklist from CONVENTIONS/DESIGN.
- [ ] Update README "Development" with the real commands. Update AGENTS.md commands.
- **Accept:** a fresh clone → `pnpm i && pnpm dev` works by following the README alone.
