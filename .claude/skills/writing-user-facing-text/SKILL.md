---
name: writing-user-facing-text
description: Writes and wires any text people see in Teitunnel (UI labels, buttons, errors, Doctor findings, plan steps, notifications, menus) through the locales/en.json catalog, the generated text::msg functions in Rust and t() in React, with plurals, placeholders, per-platform wording and Teitunnel's voice. Use whenever a change adds or edits a user-visible string, in Rust or TypeScript.
---

# Writing user-facing text

Every sentence a person sees comes from `locales/en.json`, so the app can be translated and
the voice stays consistent. English is never assembled in code.

## Where text lives

- **UI text**: top-level keys (`"routes": { "empty": { "title": "No routes yet" } }`), read
  with `t("routes.empty.title")`.
- **Text produced in Rust** (errors, Doctor, plan steps, menus, notifications, CLI-facing
  messages from core): under `"core"`. `crates/core/build.rs` generates one function per
  message in `text::msg`, with exactly the message's placeholders as arguments, so a wrong
  key or missing argument fails to compile:

  ```json
  "core": { "doctor": { "dnsMissing": { "title": "{hostname} has no DNS record" } } }
  ```

  becomes `msg::doctor::dns_missing::title(hostname) -> Text`.

## In Rust

Return a `Text`, never a `String` meant for a person. Errors map each variant to its
message (`engine/planner.rs`):

```rust
impl UserText for PlanError {
    fn text(&self) -> Text {
        match self {
            Self::NoZone(hostname) => msg::error::plan::no_zone(hostname),
            Self::RouteExists(hostname) => msg::error::plan::route_exists(hostname),
            // …
        }
    }
}
```

- Errors implement `UserText`; their `Display` (English, for logs) comes from it.
- Technical values (hostnames, paths, codes) are arguments; `msg::raw` wraps text shown as
  it is.
- The UI translates a `Text` with `translate()`. The core renders it itself only for native
  menus, notifications and the CLI.

## In React

```tsx
import { t } from "@/lib/i18n";

<Button>{t("routes.empty.add")}</Button>
```

- Module-level maps hold message keys (`MessageKey`) and call `t()` at render, never at
  import.
- Write a whole sentence as one message with placeholders; never concatenate fragments.
- `aria-label`s, placeholders, tooltips and toasts are text too.

## Plurals and placeholders

- Placeholders are `{name}`. Keep their names in every language.
- Counts use CLDR plural suffixes with `{count}`:
  `"routes_one": "{count} route"`, `"routes_other": "{count} routes"`. In Rust the generated
  function takes `count: u64` first.

## Per-platform wording

A message that names something macOS-specific (this Mac, Finder, the keychain, System
Settings, the menu bar, ⌘, Homebrew) needs Windows and Linux wordings as sibling keys:
`"key@windows"` and `"key@linux"` ("this PC", "File Explorer", "Credential Manager",
"Settings", "Ctrl"). The catalog test fails when one is missing.

## Voice

Follow [DESIGN §10](../../../docs/DESIGN.md):

- Title case for buttons, menu items and window, sheet and dialog titles ("Add Route",
  "Check Again"); sentence case for everything else ("No routes yet").
- An action that asks for more before acting ends with "…" ("Delete…").
- Say what happened, why, and what to do next: "Couldn't create app.teispace.com. A record
  with this name already exists (A 203.0.113.4). Replace it or choose another name."
- Describe outcomes, not internals: "app.teispace.com now points to localhost:3000", not
  "Ingress updated". Use the product's terms: Route, Domain, Tunnel, Connector, Quick Share,
  Doctor, Activity.
- No exclamation marks, "Oops", emoji or marketing tone.
- Hostnames in examples use a real-looking domain such as `teispace.com`.

## Check

```sh
pnpm --filter @teitunnel/desktop test -- i18n         # catalog checks: keys, placeholders, platforms
pnpm --filter @teitunnel/desktop i18n:missing <lang>  # a translation's missing keys
cargo build -p teitunnel-core                       # regenerates text::msg
```

Then `pnpm verify`.
