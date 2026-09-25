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
- On macOS, items Teitunnel creates trust exactly its own signed programs (the app, the bundled and `PATH` `teitunnel` command; by designated requirement), so they share items without keychain prompts and no other program gains access (D-127). The only `unsafe` code in the workspace is this access-list call.
- IPC commands take an `AccountId`, never a token. The token-add command accepts a token as input and never echoes it back.
- OAuth: Authorization Code + PKCE (S256), a random `state` checked on callback, a loopback listener bound to `127.0.0.1` only, a single use, and a 5-minute timeout. Refresh tokens are revoked on sign-out. Cloudflare rotates the refresh token on every refresh, so the current access token is kept in the keychain too (`cf:<account>:oauth-access`, with its expiry and a hash of the refresh token it belongs to): the app, the CLI and the MCP server share one refresh instead of revoking each other's tokens. Removing the account deletes both items.
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
- **App updates:** Tauri updater with signature verification (minisign key; the public key is built into the app, the private key lives only in CI secrets, with an offline backup held by the maintainer). The download is verified before it's kept, and installed only at restart or quit. A check fetches `latest.json` from the latest GitHub release and sends nothing about the user (D-075).
- **Dependencies:** `cargo deny` (advisories, licenses, bans, sources) in CI. Lockfiles are committed. Dependabot opens grouped weekly updates (Cargo, npm, Actions) and security-fix PRs; alerts are fixed, or dismissed with the reason recorded here.
- **Repository:** secret scanning with push protection is on, so a commit containing a credential is refused. A ruleset on `main` (D-082) requires a pull request with passing CI (Rust on three systems, Web, IPC bindings, cargo-deny) and forbids force pushes and deleting the branch; only organization members can merge, so outside changes are always reviewed by one. Release signing keys live only in the protected `release` environment (D-074).
- **Releases** are built only in GitHub Actions from tagged commits. macOS builds are signed with a Developer ID and notarized.

### Snapshots
- Only regular files inside the chosen folder are uploaded; links that resolve outside it
  are skipped, and every file is re-checked against the folder when uploaded. Hidden files,
  `.env*`, private keys, `.git` and `node_modules` are never uploaded; the review lists them.
- Builds run the project's package manager with discrete arguments (`pnpm run build`),
  never a shell, and only after the user confirmed the command. Script names are checked
  to be plain words.
- The crawler captures only sites on this computer or its network, stays on the starting
  origin (redirects included), and writes only inside a fresh folder in the app data.
- A Snapshot password is hashed at once (PBKDF2-HMAC-SHA256, random salt) and sent only as
  a Worker secret; it's never stored locally or logged, and its `Debug` is redacted.
  Sessions are HMAC-signed `__Host-` cookies (Secure, HttpOnly, SameSite=Lax) keyed to the
  hash, so changing the password ends them.
### Edge protection and service tokens
- Edge rules are written one at a time to a phase's entry point and scoped to one
  hostname (`http.host`); Teitunnel never replaces a ruleset and never changes, moves or
  deletes a rule it didn't create (description marker or its ownership index). Nothing is
  ever applied to a whole domain: a Free zone's rate limit, which can't match a hostname,
  is refused.
- A service token's secret exists outside Cloudflare only right after it's created or
  rotated. The app keeps it in memory for 10 minutes and copies it to the clipboard from
  Rust, so it never crosses IPC and the window only shows a mask; the CLI prints it once;
  an agent gets it once from `service_token_create`, after the person approved it, marked
  sensitive. It's never stored, logged or recorded in Activity; only the token's id,
  client id, name and expiry are kept. Revoking takes the token out of the login before
  deleting it.
- A new token on a hostname without a login creates an Access application only tokens
  pass (people are refused), and the plan says so.

### AI agents (MCP server, `crates/mcp`)
- Agents get the app's abilities through `teitunnel mcp` (stdio, started by the client) and `/mcp` on `teitunnel serve` (Streamable HTTP), never more: every Cloudflare change is a plan from the engine, applied by its fingerprint.
- **Modes** per server: `read-only` (tools that change anything aren't listed and are refused), `ask` (default: each change needs the person's approval, asked through the client with MCP elicitation when it can, else the tool answers `needsApproval` and only a second call with `confirmed: true` proceeds; an agent's `confirmed` never overrides a person who can be asked or said no), `full`. Records Teitunnel didn't create need an explicit confirmation in every mode.
- **Secrets never reach agents:** tools return core types that hold no credentials; every string in every answer passes through the redaction rules again; captured `Authorization`, `Cookie`, `Set-Cookie`, API-key and webhook-signature headers are masked unless the server was started with `--allow-secrets`. Account credentials are fixed only in the app.
- Every agent-initiated change is recorded in Activity with an `actor` (the client's name and version, and for HTTP the API key's name), so people can see and undo it.
- Rate limits per tool class (reads, waits, changes, destructive changes), bounded and paged answers, timeouts, and cancellation.
- **HTTP:** API keys only (`Authorization: Bearer`, hashes at rest), requests with an `Origin` header refused unless allowed (DNS rebinding), loopback `Host` names only unless `--allow-remote`, no CORS headers, repeated bad keys refused per address.
- Client setup (`teitunnel mcp install`) edits only the `teitunnel` entry of a client's configuration, keeps a backup, never touches a file it can't parse, and writes no secret (the entry is a command path and arguments).

### Local control connection and links (`crates/control`)
- Programs on this computer (the CLI, editor extensions, launchers) reach the running app
  only through a Unix socket (0600, in a 0700 folder, peers checked to be the same user) or
  a named pipe whose DACL grants only the current user, with a random name, refusing remote
  clients. Nothing listens on TCP.
- Each connection must present the per-install token (`<data>/control/token`, 0600,
  compared in constant time) in `hello` within 5 seconds; a wrong token or protocol version
  closes it. Messages are bounded (1 MiB), requests rate-limited and timed out, connections
  capped.
- Sharing, stopping a share and applying a plan need the person's approval in a native
  dialog naming the program, unless they chose "Always Allow" for that program name
  (revocable in Settings ▸ Integrations). Replacing or deleting DNS records Teitunnel didn't
  create is asked every time. Route changes are recorded in Activity with the program's
  name. The program's name is self-declared: the token and file permissions, not the name,
  keep other users out; a process running as the same user is trusted like the rest of this
  model.
- `teitunnel://` links are parsed strictly (known actions and parameters only, a port, a
  hostname or an id). Sharing from a link always asks, is never remembered, and only one
  question is shown at a time; links that only open a view don't ask. Links can be turned
  off. Nothing a link carries is passed to a process.
- Errors and texts sent over the connection are English sentences with no secrets; the
  protocol carries no credentials. `requestArrived` sends the method, the path with
  secrets masked (as the inspector's list shows it), status and duration, never headers
  or bodies; it's bounded (at most 10 events per 250 ms) and only produced while a client
  subscribes.
- The editor and launcher extensions (`integrations/`: VS Code, Raycast, JetBrains) read
  the token file themselves, hold it in memory only, and never log it or put it in an
  error message; their names (`vscode`, `raycast`, `jetbrains`) are self-declared like
  any client's, so **Always Allow** trusts a name, not a signed program. Any process of
  the same user could claim one; that's the same trust boundary as reading the token.
  They never share or stop anything without the app's approval, and a declined change is
  reported quietly.
- The menu bar's one-click share and the global shortcut (off by default) act on the
  person's own click or key press, so they don't ask; they run the exposure check first
  (D-108) and open the Quick Share sheet instead of sharing when it finds something. A
  shortcut another app holds is refused, and nothing is registered while it's off.

### Inspector (captured traffic, `core::inspect` over `crates/lens`)
- Taps listen on loopback only. Captures stay in the process's memory and, masked, in the
  local database for a day (credential headers keep only their scheme or cookie names;
  secret-named query, form and JSON values and token-like strings are replaced; text bodies
  are stored masked), so `teitunnel.db` still holds no secrets. Captures leave the process
  only by an explicit export or copy, the app's IPC, `serve`'s authenticated read-only
  `/api/traffic`, or an agent's traffic tools; all masked unless the person clicks to reveal
  (IPC `inspect_exchange` with `reveal`, never persisted) or unticks **Redact** on an
  export, or the MCP server runs with `--allow-secrets`.
- Webhook signing secrets and bearer tokens for exposed services live only in the
  keychain; they cross IPC only as input (never echoed back). A generated secret link key
  or bearer token is returned once, to show the person.
- Bearer tokens come from the OS's random generator (32 bytes). Agents get configurations
  with a `<TOKEN>` placeholder; the person reads the token with `teitunnel token`.
- Inspecting a route is a reviewed plan; it's reverted when inspection ends, when its
  process quits, at the next launch after a crash, and on the Doctor's `inspect.orphan` fix.

### Local HTTPS domains (`core::local_domains` over `crates/localdomains`)
- The local CA is name-constrained (critical NameConstraints: `localhost`, `test`, `local`, loopback and private IP ranges; pathlen 0), so a stolen key can't sign a certificate a browser or TLS library accepts for a real site. Its key (PEM bundle) lives only in the keychain (`localdomains:ca`), is loaded on a blocking thread, and never reaches the database, backups, logs or IPC; only the public certificate is written to disk (0644) or saved for a phone on request. `TEITUNNEL_LOCAL_CA_FILE` keeps it in a plain 0600 file instead, for keychain-less servers, with a warning. Leaf keys stay in memory.
- Trust is added at user level where the OS allows it (macOS asks for the password; Windows confirms). Nothing elevates silently: the Linux system store, the `.test` resolver entry and low-port fixes are shown as commands, or run through `pkexec` after the person chooses to.
- Handshakes are answered only for registered names. Listeners may be on the wildcard address (macOS allows 443 only there), so every connection's peer is checked before reading: this computer's addresses always; private-network peers only with LAN access on, and over TLS only for `.local` names. The plain listener only redirects to the same listed host (no open redirect). `CF-Connecting-IP` is ignored on these taps.
- The `.test` name server binds loopback only and answers only for registered `.test` names; `.test` is never delegated on the internet (RFC 6761).
- `localDomains.reload` on the control connection needs no approval: it only makes the app serve what its own database already says.

### Comments (`core::comments`, the overlay, the Snapshot Worker)
- Live shares answer the comments API from Lens under the reserved `/__teitunnel/comments/`
  path, after the tap's gates (password, secret link, bearer…) admitted the visitor;
  comments are kept in the local database. Snapshots answer it from their Worker, with a D1
  binding; the app reads and answers through the D1 query endpoint with the account's
  token, so no Worker exposes an owner API or holds an owner key.
- Writes must be `application/json` from the same site (`Sec-Fetch-Site: same-origin`, or a
  matching `Origin`), so a cross-site form or script can't post; bodies are capped (16 KB),
  and each visitor is rate-limited (10 writes a minute, 60 an hour; in the Worker by a
  hash of site and IP, never the IP itself).
- Text is validated (lengths, no control or bidirectional-override characters) and stored
  as typed; the overlay sets text only with `textContent`, the app renders it as React text.
  The overlay runs in a closed Shadow DOM, uses constructable stylesheets and DOM APIs only
  (works under a strict CSP that allows same-origin scripts), and loads nothing external.
- Reviewer identity is a typed name, or the `Cf-Access-Authenticated-User-Email` header only
  when the hostname has Teitunnel's own Access login (otherwise a visitor could send it).
  Email addresses are never returned to reviewers or to agents.
- Caps: 2,000 comments per subject, 200 per thread; agents' replies and resolutions need
  the person's approval like any change.

### Workers in front of routes (offline page, webhook inbox; `engine::front`, `core::inbox`)
- Created, changed and removed only through plan → apply with rollback and undo; Teitunnel
  touches only Worker scripts named `tt-…` and Worker routes recorded in its ownership index
  (migration 20); a route pattern someone else's Worker has is refused, never replaced.
  Routes are created to fail open, so the daily free-request limit never takes a site down.
- The offline Worker changes no working response; it replaces only 530 (and, if chosen,
  502/504) answers, with `no-store`, `noindex` and a restrictive CSP on its page.
- The inbox keeps at most 1,000 webhooks of at most 512 KB, drops cookies, `cf-*` and
  forwarding headers, deletes everything after its retention (1–30 days), and answers 503
  when full. A verifying inbox's signing secret comes from the keychain (never IPC outward,
  never argv: the CLI reads it from the environment or standard input), is sent only as a
  `secret_text` binding and kept on later versions with `keep_bindings`; it's never in a
  plan, Activity or the local database. Delivery goes only to the route's own local service
  (its ingress rule), without following redirects.

### Logs & diagnostics
- A `tracing` redaction layer scrubs bearer tokens, `TUNNEL_TOKEN`, `apiToken`, and JWT-like strings.
- The diagnostics bundle is redacted, created locally, and shown to the user before they share it.

### Local database
- `teitunnel.db` has file mode 0600 and contains no secrets.

## Known residual risks

- A process running as the same OS user can read the always-on token file and the child process environment. This matches the OS threat model (same-user processes are trusted); the alternative would be a privileged helper, which adds more risk than it removes.
- `glib` 0.18 (RUSTSEC-2024-0429, unsound `VariantStrIter`) comes only through Tauri's Linux GTK/tray stack, which pins gtk-rs 0.18. Teitunnel never uses `glib` directly, so the unsound iterator is unreachable; the Dependabot alert is dismissed as a tolerable risk (2026-09-23). Revisit when Tauri moves to gtk-rs 0.20 or later.
- A process of the same user can add a local domain (it writes the same database) and reach the services behind it over HTTPS; that's no more than it could reach on their ports directly.
- Quick Share URLs are public and unauthenticated by design. The UI says so and offers an auto-stop timer.
