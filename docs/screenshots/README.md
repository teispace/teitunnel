# Screenshots

Visual evidence per task (`<task-id>/<name>-<light|dark>.png`, 1×).

- Captured with `pnpm --filter @teitunnel/desktop shoot <dir> [routes…]` (WebKit via Playwright), which approximates the native sidebar material with its measured colour. Native captures of the real window (vibrancy, traffic lights, accent) are taken with `screencapture -l <window-id>` when the screen is unlocked.
- Keep them small: 1× PNG, only the states that matter.
