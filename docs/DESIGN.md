# Design System

Goal: Teitunnel should feel as if Apple shipped it. Quiet, dense, precise, fast. The reference bar is macOS System Settings, Xcode, Tower, Proxyman, TablePlus, Tailscale and the Linear desktop app.

Every UI PR is reviewed against the **checklist at the bottom**.

---

## 1. Hard rules

**Never:**
- Gradients, glows, neon, coloured shadows, fake-glass cards/panels (real system materials only; see §2a).
- Emoji in UI copy, "✨", decorative icons in every heading, oversized hero sections.
- Pointer (hand) cursor on buttons, rows or toggles. Native apps use the arrow. The hand is only for real hyperlinks.
- Custom-drawn imitations of native things: menu bar, context menus, file dialogs, notifications. Use the real ones.
- Raw hex or `rgb()` outside `styles/tokens.css`. Components use semantic tokens only.
- Layout shift on load. Reserve space; use skeletons only when content takes > 300 ms.
- Spinners for local state. Local reads are instant; spinners are for network only, and only after 150 ms.
- Modals for primary work. Modals are for destructive confirmation and plan review only.
- Exclamation marks, "Oops", "Awesome", or marketing tone in the product.

**Always:**
- System fonts, system accent color, system light/dark (with an override in Settings).
- Keyboard reachable: every action has a menu item, and frequent ones have shortcuts.
- Text not selectable by default. **Values** (URLs, IDs, hostnames, log lines) are selectable and have a copy affordance.
- Respect *Reduce motion*, *Reduce transparency*, *Increase contrast*.
- Status never by color alone: pair it with a shape or label.

---

## 2. Window & layout (macOS)

| Property | Value |
|---|---|
| Default size / min size | 1120 × 720 / 880 × 560 |
| Title bar | `titleBarStyle: Overlay`, `hiddenTitle: true`, `trafficLightPosition {x:19, y:28}` (buttons at the same pixels as System Settings, D-025) |
| Title bar height (drag region) | 52 px unified toolbar area; the content toolbar lives in it |
| Sidebar | 220 px (resizable 180–300), vibrancy `sidebar` material plus a dark-mode tint (D-024), collapsible (⌘⌥S) |
| Content | opaque `--surface-content` |
| Inspector | 320 px (resizable 240–480), toggle ⌘⌥I. List pane min 200, detail min 180, so three panes fit the minimum window |
| Split | Sidebar │ List │ Inspector (three-pane), like Mail/Finder |

The window uses `transparent: true` + `windowEffects: { effects: ["sidebar"], state: "followsWindowActiveState" }` + `macOSPrivateApi: true`. Only the sidebar region is transparent; content panes paint opaque backgrounds. When the window is inactive, the vibrancy dims and the accent colour turns grey, as in native apps.

**macOS 27 ("Golden Gate") alignment**, from WWDC 2026:
- Sidebars run **edge to edge** (full height, flush with the window edges). They are not the floating inset panels of macOS 26.
- **Unified toolbar** across the top: the title bar and toolbar are one 52 px band. Toolbar controls sit in grouped **capsules** (glass-like on 27).
- **Sidebar icons are tinted** again (accent colour when unselected in the active window, white on selection).
- **Window corner radius** is system-controlled. We never draw our own window corners.
- **Stronger active/inactive window distinction.** Our inactive state must visibly desaturate (selection grey, toolbar controls dimmed, vibrancy "inactive").

---

## 2a. Materials (Liquid Glass era)

| Surface | Implementation | Notes |
|---|---|---|
| Sidebar | Native `NSVisualEffectView` via `windowEffects: ["sidebar"]` | Real system material; follows the user's Liquid Glass/transparency settings |
| Content, inspector | Opaque tokens | Readability first |
| Toolbar capsules, floating status pill | CSS material: `backdrop-filter: blur(20px) saturate(180%)` + `--material-glass-tint` + 0.5 px inner highlight | Only on elements floating **over scrolling content** |
| Popovers, menus | Native menus where possible; HTML popovers use `--surface-raised` + blur material | |
| Sheets | `--surface-raised`, no blur | |

**Rules:**
- **Do not use `NSGlassEffectView`** (via `tauri-plugin-liquid-glass` or similar) in shipped builds for now. On macOS 27 with Tauri 2.11 and WKWebView it crashes packaged builds on resize and first paint ([tauri-apps/window-vibrancy#229](https://github.com/tauri-apps/window-vibrancy/issues/229), unfixed as of 2026-09-22). Revisit once it's fixed, and track it in `docs/research/platform.md`.
- CSS glass is for **controls floating over content only**. It's never used for cards, panels or large areas; that's where "AI-looking glassmorphism" comes from.
- Under *Reduce transparency*, every CSS material falls back to its opaque `--surface-raised` equivalent.
- Every material decision is verified in a **packaged** build (`tauri build`), not only `tauri dev`, since custom-protocol loading behaves differently.

---

## 3. Typography

System stack: `-apple-system, BlinkMacSystemFont, "SF Pro Text", "Segoe UI Variable", "Segoe UI", system-ui, sans-serif`.
Mono: `ui-monospace, "SF Mono", "Cascadia Mono", Menlo, monospace`.

| Token | Size / line | Weight | Use |
|---|---|---|---|
| `--text-large-title` | 26 / 32 | 700 | Onboarding only |
| `--text-title1` | 22 / 28 | 600 | Empty-state headline |
| `--text-title2` | 17 / 22 | 600 | Inspector title |
| `--text-title3` | 15 / 20 | 600 | Section titles |
| `--text-headline` | 13 / 16 | 600 | Row primary emphasis, group headers |
| `--text-body` | 13 / 16 | 400 | Default |
| `--text-callout` | 12 / 15 | 400 | Secondary rows, form help |
| `--text-footnote` | 11 / 13 | 400 | Captions, metadata, sidebar section labels |
| `--text-mono` | 12 / 16 | 400 | Values, logs |

Tabular numbers (`font-variant-numeric: tabular-nums`) for all metrics, ports and counts.

---

## 4. Color

Semantic tokens with light and dark values. The values below approximate AppKit semantic colours, and are tuned in M0 against native controls side by side.

| Token | Light | Dark |
|---|---|---|
| `--surface-window` | `#ECECEC` | `#1E1E1E` |
| `--surface-sidebar` | transparent (vibrancy) | `rgb(0 0 0 / 0.42)` over vibrancy (D-024) |
| `--surface-content` | `#FFFFFF` | `#212222` (measured, System Settings) |
| `--surface-raised` (popover, sheet) | `#FFFFFF` | `#2C2C2C` |
| `--surface-control` | `#FFFFFF` | `rgb(255 255 255 / 0.10)` |
| `--surface-hover` | `rgb(0 0 0 / 0.04)` | `rgb(255 255 255 / 0.05)` |
| `--surface-selected` | `var(--accent)` | `var(--accent)` |
| `--surface-selected-inactive` | `rgb(0 0 0 / 0.08)` | `rgb(255 255 255 / 0.10)` |
| `--text-primary` | `rgb(0 0 0 / 0.85)` | `rgb(255 255 255 / 0.85)` |
| `--text-secondary` | `rgb(0 0 0 / 0.50)` | `rgb(255 255 255 / 0.55)` |
| `--text-tertiary` | `rgb(0 0 0 / 0.26)` | `rgb(255 255 255 / 0.25)` |
| `--text-on-accent` | `#FFFFFF` | `#FFFFFF` |
| `--border-separator` | `rgb(0 0 0 / 0.10)` | `rgb(255 255 255 / 0.10)` |
| `--border-control` | `rgb(0 0 0 / 0.15)` | `rgb(255 255 255 / 0.12)` |
| `--accent` | system accent via AppKit, fallback `#007AFF` | same, fallback `#0A84FF` |
| `--focus-ring` | accent @ 50%, 3.5 px outer | same |
| `--status-healthy` | systemGreen `#34C759` | `#30D158` |
| `--status-warning` | systemOrange `#FF9500` | `#FF9F0A` |
| `--status-error` | systemRed `#FF3B30` | `#FF453A` |
| `--status-idle` | systemGray `#8E8E93` | `#98989D` |

- **Accent:** WKWebView's CSS `AccentColor` is a static blue (verified M0). The shell reads `NSColor.controlAccentColor` and the UI sets `--accent` on start and on every window focus (D-023). Tokens use `light-dark()`.
- **Brand orange** (Cloudflare-adjacent) appears only in the app icon and the About window.
- Increase contrast doubles separator/border alpha and raises `--text-secondary` to 0.7.

---

## 5. Spacing, radius, elevation

- 4 px base grid. Common steps: 4, 8, 12, 16, 20, 24, 32.
- Control heights: small 22, regular 28 (default), large 32 (onboarding only).
- List row: 28 px single-line, 44 px two-line. Sidebar row: 32 px, 18 px icons, 10 px inset (D-025).
- Radius: controls 6, list rows/selection 8, cards/sections 10, sheets/popovers 12, window (system).
- Borders: 1 px hairlines (`0.5px` on 2× displays via `@media (min-resolution: 2dppx)`).
- Shadows only on raised surfaces (popover, sheet, menu). Never on cards.

---

## 6. Iconography

- Lucide at **16 px, stroke 1.5** in toolbars and sidebar; 14 px inline.
- Sidebar icons are **accent-tinted** in the active window (as in macOS 27), white on the selected row, and `--text-secondary` when the window is inactive. Toolbar and inline icons use `--text-secondary`.
- Status is a `StatusDot`: an 8 px circle, plus a shape for colour-blind users (filled = healthy, ring = idle, triangle glyph = warning, cross = error).
- App icon (M6): a native macOS icon grid, rounded square, subtle depth. No generic "tunnel" clip-art.

---

## 7. Motion (Apple-style)

Apple UI motion is **spring-based, interruptible and physically consistent**. It isn't timed curves. We follow the SwiftUI model, where a spring is described by **duration + bounce**, which maps directly onto Motion's (`motion/react`) `{ type: "spring", visualDuration, bounce }`.

| Token | Spring (visualDuration / bounce) | SwiftUI analogue | Use |
|---|---|---|---|
| `spring.snappy` | 0.25 s / 0.10 | `.snappy` (fast) | Toggles, segmented control thumb, selection highlight, hover-to-press |
| `spring.smooth` | 0.35 s / 0.00 | `.smooth` | Disclosure expand/collapse, inspector open/close, list insert/remove, layout changes |
| `spring.sheet` | 0.45 s / 0.00 | sheet presentation | Sheets sliding from the toolbar, dialogs scale 0.97 → 1 + fade |
| `spring.bouncy` | 0.40 s / 0.20 | `.bouncy` | **Rare**: success confirmation (URL ready check mark), copy-confirmation tick |
| `spring.interactive` | 0.15 s / 0.00 | `.interactiveSpring` | Direct manipulation: split-view drag, reorder drag, slider |

**Implementation:**
- Simple state transitions (hover, press, colour) use CSS transitions with a **spring baked into `linear()` easing** (generated once from the tokens in `styles/motion.css`). There's no JS cost.
- Layout changes, enter/exit, reorder and shared-element moves use **Motion** (`motion/react`, `LazyMotion` + `domAnimation` to keep the bundle small) with `layout` / `AnimatePresence`. Springs are **interruptible**: a new target retargets from the current velocity, never snaps back.
- Section switches in the sidebar use **no slide**. Content cross-fades (≤ 120 ms), like System Settings. The View Transitions API (WebKit supports it) is used only for the list → inspector detail morph when it clearly helps.
- Press feedback: controls darken instantly on press (no delay) and release with `spring.snappy`. Buttons don't scale down (that's an iOS idiom, not a macOS one). Toolbar capsule items may use a subtle highlight.
- Status changes (connecting → healthy): the StatusDot cross-fades colour. Connecting is a slow 1.2 s opacity pulse, never a spinning ring.
- Numbers (request counters, metrics) roll with tabular digits. There are no count-up animations on first render.
- Animate only `transform`, `opacity` and `filter`; never `width/height/top/left`. Use Motion's `layout` for size changes (FLIP). Target 120 Hz on ProMotion displays: no layout thrash, and `will-change` only during animation.
- **Reduce motion:** springs become 80 ms opacity cross-fades, layout animations become instant, and the pulse stops (a static ring is shown).

**Never:** entrance animations for page content, staggered list cascades, parallax, bouncy navigation, looping decorative motion, skeleton shimmer longer than 1 s.

---

## 8. Components

`components/ui/` (primitives, shadcn/Radix-derived, restyled to these tokens):
Button (primary / secondary / plain / destructive; `sm`/`md`), IconButton, Input, TextArea, Field (label + help + error), Select, Combobox, Switch, Checkbox, Radio, SegmentedControl, Slider, Tooltip, Popover, Dialog, Sheet (slides from title bar, like macOS sheets), Toast (sonner, restyled), Badge, Kbd, StatusDot, Spinner, ProgressBar, Separator, ScrollArea, Disclosure, Skeleton.

`components/patterns/`:
AppShell, TitlebarToolbar, Sidebar + SidebarItem, SplitView (resizable, persisted), ListPane + ListRow, Inspector + InspectorSection, KeyValueGrid, CopyField, HostnameInput (subdomain + zone picker + live validation), OriginPicker (detected services + manual), PlanPreview (steps + warnings + command copy), ProgressChecklist, EmptyState (one sentence + one primary action), ErrorState (message + hint + fix button), MetricTile, Sparkline (uPlot), LogViewer (virtualized), CommandPalette (cmdk), QRCode (SVG from Rust).

Every primitive and pattern is shown in the **in-app dev gallery** (`/dev/gallery`, dev builds only) in light and dark. The gallery runs inside the real WKWebView with real vibrancy, which Storybook in a browser can't do.

---

## 9. Native integration checklist (macOS)

- Menu bar: App (About, Settings… ⌘,, Services, Hide, Quit), File (New Route ⌘N, New Quick Share ⇧⌘N, Close Window ⌘W), Edit (Undo/Redo/Cut/Copy/Paste/Select All: **required**, or text fields lose these shortcuts), View (Toggle Sidebar ⌘⌥S, Toggle Inspector ⌘⌥I, Refresh ⌘R), Window, Help (Documentation, Report an Issue, Export Diagnostics).
- Context menus on rows via Tauri's native `Menu` (not HTML).
- Menu bar extra (tray): monochrome **template** icon that adapts to light/dark, with a native menu listing routes and Quick Shares, their status, toggles, "Open Teitunnel", "Quit".
- Native notifications (only for events the user can't see: connector crashed while the window is hidden, Quick Share ready while in the background).
- Closing the last window keeps the app running in the menu bar when tunnels are active (configurable). Quit (⌘Q) with running Session connectors asks for confirmation, with a "Don't ask again" option.
- Single instance: a second launch focuses the existing window.
- Window size, position, sidebar width and inspector state are restored.
- Drag regions: the title bar/toolbar area only. Controls inside it are `no-drag`.
- Scrollbars follow the system (overlay, auto-hide).
- Double-clicking the title bar zooms/minimizes per the system preference.

---

## 10. Content & voice

- Sentence case everywhere ("Add route", not "Add Route").
- Buttons are verbs ("Add route", "Apply 4 changes", "Delete record"). Destructive buttons say exactly what they destroy.
- Explain outcomes, not internals: "app.xyz.com now points to localhost:3000", not "Ingress updated".
- Errors: what happened, then why, then what to do. For example: "Couldn't create app.xyz.com. A record with this name already exists (A 203.0.113.4). Replace it or choose another name."
- Numbers: "3 routes", "1 issue". Relative times ("2 min ago") with the absolute time in the tooltip.
- Terminology (fixed): Route, Domain, Tunnel, Connector, Quick Share, Local service, Doctor, Activity. Avoid "ingress" outside Advanced views.

---

## 11. Accessibility

- All controls are keyboard operable, with a visible focus ring (`--focus-ring`) on `:focus-visible` only.
- VoiceOver: every IconButton has a label, status is announced as text, live regions announce plan progress.
- Contrast ≥ 4.5:1 for body text and ≥ 3:1 for large text/icons in both themes.
- Hit targets ≥ 22 × 22 px.

---

## 12. PR checklist (UI)

- [ ] Uses only semantic tokens. No raw colours or ad-hoc spacing values.
- [ ] Screenshots in light and dark, active and inactive window.
- [ ] Keyboard path works. Menu item and shortcut exist where appropriate.
- [ ] No hand cursor, no layout shift, no spinner for local state.
- [ ] Copy follows §10. Empty, loading and error states are designed.
- [ ] Reduced motion/transparency checked.
- [ ] Added or updated in the dev gallery if it's a primitive/pattern.
