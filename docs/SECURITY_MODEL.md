# Security Model

To report a vulnerability, see [SECURITY.md](../SECURITY.md). This document describes how Teitunnel protects users by design.

## Assets

1. Cloudflare credentials: OAuth refresh/access tokens, API tokens, cert.pem.
2. Tunnel run tokens. Anyone holding one can run a connector for that tunnel and receive its traffic.
3. The user's DNS zones. A wrong write can take a site down or enable subdomain takeover.
4. The user's local services, which must only be exposed deliberately.
5. Integrity of the `cloudflared` binary and of Teitunnel updates.

## Trust boundaries

```
[Webview UI]  ──IPC──▶  [Rust core]  ──▶ keychain, SQLite, processes, Cloudflare API, GitHub
   untrusted-ish            trusted
```

The webview is treated as the less-trusted side. It renders data and requests actions, but never holds secrets or performs privileged work.

## Controls

### Credentials
- Stored only in the OS keychain (`keyring`). Never in SQLite, config files, logs, or IPC responses.
- IPC commands take an `AccountId`, never a token. The token-add command accepts a token as input and never echoes it back.
- OAuth: Authorization Code + PKCE (S256), a random `state` checked on callback, a loopback listener bound to `127.0.0.1` only, a single use, and a 5-minute timeout. Refresh tokens are revoked on sign-out.
- The permissions are documented in the token template. It includes Access (apps and login methods) so logins work without a second trip (D-065); Teitunnel still changes only Access applications it created (ownership index, D-057), and a token without Access keeps working for plain routes. OAuth scopes for Access stay optional.
- In-memory token values are wrapped in a `Secret<T>` type whose `Debug`/`Display` impls redact them.

### Tunnel run tokens
- Session mode passes them through the `TUNNEL_TOKEN` environment variable of the child process. They never appear in argv, since argv is visible to other users via `ps`.
- Always-on mode uses `--token-file` in `<app_data>/tokens/` (directory 0700, file 0600). This is the one place a secret touches disk, because the OS service must start without the app. The file is removed when the service is uninstalled.

### Processes
- Only `tokio::process::Command` with discrete arguments. No shell, no `sh -c`, and no Tauri shell plugin.
- Every argument comes from typed builders (`crates/cloudflared/src/command.rs`). User input (hostnames, ports, paths) is validated before it reaches a builder.
- The metrics/diagnostics server is always bound to `127.0.0.1`.

### Webview & IPC
- Strict CSP: `default-src 'self'; connect-src ipc: http://ipc.localhost; img-src 'self' data:; style-src 'self' 'unsafe-inline'` (inline styles are needed by Radix positioning; revisit). No remote scripts. No remote content is ever loaded into the window.
- Tauri capabilities are least-privilege per window. The main window gets only the app's own commands plus required plugin permissions.
- External links open in the system browser via the opener plugin, restricted to `https://`.
- The dev gallery and devtools are excluded from release builds.

### DNS safety
- All DNS writes go through the planner and are shown before being applied.
- Only records carrying Teitunnel's ownership marker are deleted automatically. Foreign records need explicit, per-record confirmation.
- Creating a record over an existing one needs explicit confirmation, with the existing record shown.
- The orphan scanner flags CNAMEs to deleted tunnels (a subdomain-takeover risk).

### Supply chain
- **cloudflared downloads:** HTTPS from GitHub Releases. SHA256 is checked against the checksums published in the release notes. On macOS, `codesign --verify --strict` is also run and the Developer ID Team ID is checked against Cloudflare's. The install is atomic, and the previous version is kept for rollback.
- **App updates:** Tauri updater with signature verification (minisign key; the private key lives only in CI secrets).
- **Dependencies:** `cargo deny` (advisories, licenses, bans, sources) and `pnpm audit` in CI. Lockfiles are committed. Dependabot/Renovate is grouped weekly.
- **Releases** are built only in GitHub Actions from tagged commits. macOS builds are signed with a Developer ID and notarized.

### Logs & diagnostics
- A `tracing` redaction layer scrubs bearer tokens, `TUNNEL_TOKEN`, `apiToken`, and JWT-like strings.
- The diagnostics bundle is redacted, created locally, and shown to the user before they share it.

### Local database
- `teitunnel.db` has file mode 0600 and contains no secrets.

## Known residual risks

- A process running as the same OS user can read the always-on token file and the child process environment. This matches the OS threat model (same-user processes are trusted); the alternative would be a privileged helper, which adds more risk than it removes.
- Quick Share URLs are public and unauthenticated by design. The UI says so and offers an auto-stop timer.
