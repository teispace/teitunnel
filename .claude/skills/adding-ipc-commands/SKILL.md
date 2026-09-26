---
name: adding-ipc-commands
description: Adds or changes a Tauri IPC command in Teitunnel, from the Rust handler in apps/desktop/src-tauri through generated TypeScript bindings to a TanStack Query hook, the dev mock and tests. Use when the desktop UI needs new data or a new action from Rust, or when a command's arguments or return type change.
---

# Adding an IPC command

The UI talks to Rust only through generated commands. A request travels:
component → hook in `features/<area>/queries.ts` → `commands.x()` in
`src/lib/ipc/bindings.ts` (generated) → `#[tauri::command]` in `src-tauri/src/ipc/<area>.rs`
→ `teitunnel_core`.

Copy this checklist and tick it off:

```
- [ ] 1. Logic lives in crates/core (not in src-tauri)
- [ ] 2. Command in src-tauri/src/ipc/<area>.rs
- [ ] 3. Registered in collect_commands! in src-tauri/src/ipc.rs
- [ ] 4. pnpm bindings
- [ ] 5. Query key and hook in the feature's queries.ts
- [ ] 6. Dev mock answers it
- [ ] 7. Tests (Rust for the logic, Vitest for the hook or component)
- [ ] 8. pnpm verify
```

## 1. Put the logic in core

`src-tauri` has no business logic. Write the behaviour in `crates/core` with its own unit
tests; the command only validates input, calls core and maps errors to `AppError`.
Types that cross IPC derive `specta::Type` behind the `specta` feature:

```rust
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteStats { /* … */ }
```

Never put a secret in an IPC type. Return a boolean such as `hasToken` instead.

## 2. Write the command

Name it `<area>_<verb>` (`inspect_stats`, `routes_plan_add`). Document it: the doc comment
becomes the TypeScript doc.

```rust
/// A tap's traffic over `range` (null: a tap this app doesn't know).
#[tauri::command]
#[specta::specta]
pub fn inspect_stats(
    state: State<'_, AppState>,
    tap: TapId,
    range: AnalyticsRange,
) -> Option<RouteStats> {
    LensSource::new(state.inspector.clone()).tap_stats(&tap, range)
}
```

**Sync or async:** a synchronous command runs on the main thread. It may only read memory.
Anything that touches the disk, the keychain, another process, the network or builds a
window must be `async` and run blocking work through `ipc::off_main`:

```rust
/// Puts the command line tool on the PATH.
#[tauri::command]
#[specta::specta]
pub async fn cli_install() -> Result<CliState, AppError> {
    super::off_main(|| {
        let Some(layout) = layout() else {
            return Ok(CliState::Unavailable);
        };
        layout.install().map_err(|err| failed(&err))
    })
    .await?
}
```

On macOS a blocking sync command freezes every window; on Windows building a webview from
the main thread deadlocks WebView2.

Changes to Cloudflare never happen directly in a command: they go through the engine
(see the `changing-cloudflare-resources` skill).

## 3–4. Register and generate

Add the function to `collect_commands![…]` in `apps/desktop/src-tauri/src/ipc.rs`, then run
`pnpm bindings`. Never edit `bindings.ts` by hand; CI fails when it drifts.

## 5. The hook

Query keys come only from `src/lib/ipc/query-keys.ts`. Wrap the command in `call()` so
errors become typed `IpcError`s:

```ts
export function useTapStats(tap: TapId | null, range: AnalyticsRange) {
  return useQuery({
    queryKey: keys.stats(tap ?? "", range),
    queryFn: () => call(commands.inspectStats(tap ?? "", range)),
    enabled: tap !== null,
  });
}
```

Components never call `commands.*` or `invoke` directly. Mutations invalidate through the
`EntityChanged` events Rust emits; don't mirror server data into Zustand.

## 6. The dev mock

The browser-only dev server and the screenshot tool run on `src/dev/mock-ipc.ts` and the
per-area mocks (`mock-inspector.ts`, `mock-analytics.ts`, …). Add a `case "<command_name>"`
returning realistic data, or the screen stays empty in `pnpm --filter @teitunnel/desktop vite`
and in screenshots.

## 7. Tests

- Rust: unit tests in core for the behaviour.
- UI: Vitest with `mockIPC` from `@tauri-apps/api/mocks`; record calls and assert the
  arguments the hook sends (see `features/inspector/inspector-panels.test.tsx`).

## 8. Verify

Run `pnpm verify`. If bindings are stale, run `pnpm bindings` again; if a test fails, fix
the cause and re-run until it passes.
