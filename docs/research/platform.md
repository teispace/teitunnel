# Research: platform & tooling

Checked 2026-09-22. Re-check versions at the start of each milestone.

## Toolchain on the maintainer machine
macOS 27.0 · rustc 1.98.0 · Node 26.9 · pnpm 12.4.1

## Current versions (npm / crates.io, 2026-09-22)

| Package | Version | Note |
|---|---|---|
| tauri (crate) | 2.11.6 stable (3.0.0-alpha.2 exists) | Stay on 2.x (D-015) |
| @tauri-apps/cli / api | 2.11.5 / 2.11.1 | |
| tauri-plugin-updater / single-instance / deep-link / autostart | 2.12.0 / 2.4.5 / 2.4.10 / 2.5.1 | Use 2.x lines |
| tauri-specta / specta | 2.0.0-rc.25 | RC; pin exactly (D-014) |
| window-vibrancy | 0.8.1 | Only if `windowEffects` config is insufficient |
| reqwest | 0.13.5 | rustls |
| keyring | 4.2.0 | v4 API differs from v3 |
| rusqlite / rusqlite_migration | 0.40.2 / 2.6.0 | `bundled` feature |
| sysinfo / listeners / bollard | 0.39.6 / 0.6.1 / 0.21.1 | |
| oauth2 | 5.0.0 | PKCE helpers |
| tracing | 0.1.44 | |
| insta / wiremock / proptest | 1.48 / 0.6.5 / 1.11 | |
| react / vite / typescript | 19.3.0 / 8.3.0 / 7.0.2 | TS 7 = native (Go) compiler |
| tailwindcss | 4.3.3 | |
| @tanstack/react-router / react-query / react-virtual | 1.170 / 5.103 / 3.14 | |
| zod / react-hook-form | 4.6.5 / 7.88 | |
| uplot / cmdk / sonner / lucide-react | 1.6.32 / 1.1.1 / 2.0.8 / 1.47 | |
| radix-ui / shadcn CLI | 1.6.7 / 4.21 | |
| @biomejs/biome | 2.5.14 | |
| vitest / @playwright/test / @wdio/tauri-service | 5.0.1 / 1.63 / 1.4.0 | |

## Tauri v2 macOS window config
Source: https://v2.tauri.app/reference/config/
- `titleBarStyle: "Overlay"`, `hiddenTitle: true`, `trafficLightPosition: {x, y}`.
- `transparent: true` + `windowEffects: { effects: ["sidebar"], state, radius }`. Requires `app.macOSPrivateApi: true`. That's fine because we don't ship to the Mac App Store.
- Overlay mode needs explicit drag regions (`data-tauri-drag-region`).

## E2E testing
Sources: https://v2.tauri.app/develop/tests/webdriver/ · https://webdriver.io/docs/wdio-tauri-service/
- There's no Apple WebDriver for WKWebView, so plain `tauri-driver` works only on Linux/Windows.
- `@wdio/tauri-service` embeds a WebDriver server in the app and **supports macOS**. We use it (D-017).

## macOS 27 "Golden Gate" design (WWDC 2026)
Sources: https://9to5mac.com/2026/06/09/macos-27-golden-gate-includes-these-changes-that-tahoe-critics-will-appreciate/ · https://appleinsider.com/articles/26/06/09/dont-get-excited-for-big-liquid-glass-changes-in-macos-27-because-they-arent-there · https://www.macrumors.com/2026/06/10/how-liquid-glass-is-changing-in-ios-27/
- Liquid Glass stays, with a system-wide slider from "ultraclear" to "fully tinted", better diffusion and contrast.
- Sidebars run edge to edge (no longer floating/inset as in 26). Uniform toolbars. Consistent system window corner radius.
- Sidebar icons regain colour. Fewer icons in menus. Stronger active/inactive window distinction.

## Liquid Glass from Tauri
- `tauri-plugin-liquid-glass` (0.1.6) wraps the **private** `NSGlassEffectView` (macOS 26+).
- **Blocker:** on macOS 27 with Tauri 2.11.x, `NSGlassEffectView` behind the WKWebView crashes **packaged** builds (resize → `objc_initWeak`; custom-protocol first paint → `objc_storeWeak`). Dev builds over HTTP are unaffected. Unfixed. https://github.com/tauri-apps/window-vibrancy/issues/229
- Decision D-021: use `NSVisualEffectView` (`windowEffects: ["sidebar"]`) plus CSS materials for floating controls. Re-check the issue each milestone.

## Motion
- `motion` (motion.dev) 13.4.1. The spring API `{type: "spring", visualDuration, bounce}` mirrors SwiftUI's duration/bounce model. Use `LazyMotion` + `domAnimation` for bundle size.
- CSS `linear()` easing (supported in WebKit) can encode spring curves for zero-JS transitions.

## To verify in M0
- `windowEffects: ["sidebar"]` (NSVisualEffectView) is stable in a **packaged** build on macOS 27 (resize, first paint, sleep/wake, theme switch).
- Whether the user's Liquid Glass slider/transparency settings are reflected in `prefers-reduced-transparency` or need a native query.
- `AccentColor` CSS system colour inside WKWebView, including live updates on accent change.
- Vibrancy `sidebar` material with an opaque content pane: check the compositing looks right in both themes and when the window is inactive.
- The Edit menu is required for ⌘C/⌘V in inputs on macOS.
