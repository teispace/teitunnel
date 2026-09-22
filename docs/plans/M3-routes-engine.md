# M3: Routes engine (the core promise)

**Goal:** `xyz.com → localhost:3000` and `yx.com → localhost:5000` from zero in under 60 seconds, previewed, verified, and fully reversible with zero leftovers.
**Release:** v0.3.0.
**Exit criteria:**
- Add, edit, reorder and remove routes across multiple zones on one auto-provisioned machine tunnel.
- Every mutation shows a plan preview, applies with progress, rolls back on failure, and verifies end to end.
- Removing all routes and deleting the tunnel leaves **zero** DNS records and no tunnel. This is asserted by the nightly real-account test.
- Drift from dashboard edits is detected and resolvable.
- The planner has snapshot tests for every intent and a property test for idempotency.

---

### M3-01 · cf-api: tunnels, configurations, DNS
- [x] Tunnels: list (filters, pagination), create (`config_src: cloudflare`), get, patch, delete, connections list/clean, token get.
- [x] Configurations get/put with `version`. Model the full ingress + `originRequest` schema as typed structs, preserving unknown fields (`#[serde(flatten)] extra`) so we never drop settings made in the dashboard.
- [x] DNS records: list (filters `name`, `type`, `content`, `comment.contains`), create, patch, delete.
- [ ] Fixtures recorded from a real account. *(Needs a test token from the maintainer.)*

### M3-02 · Domain types & validation
- [x] `Hostname` (IDNA/punycode, wildcard rules, max lengths), zone matching (the longest zone suffix wins), `PathRegex` (validated with the regex crate; Go RE2-compatible subset warning), `Origin` parsing from user input (`3000`, `:3000`, `localhost:3000`, `http://…`, `https://…`, `tcp://…`, `ssh://…`, `unix:/path`).
- [ ] `OriginOptions`: all `originRequest` fields cloudflared supports, with defaults omitted when serialising.
- [ ] Property tests for the parsers. `*_validate` commands return field-level errors.

### M3-03 · Observer
- [x] `observe(scope) -> Snapshot` where the scope is account/tunnel/hostnames. It fetches in parallel with bounded concurrency. A fingerprint is computed over the relevant parts.
- [x] A short-lived cache (5 s) shared by Doctor and UI queries; invalidated by our own writes.

### M3-04 · Planner
- [x] Intents: `AddRoute`, `UpdateRoute` (incl. rename), `RemoveRoute`, `RemoveTunnel` (cascade). The machine tunnel is ensured implicitly by any add. *(`ReorderRoutes` comes with drag reordering in M3-09; rules are specificity-sorted, so order only matters for equal specificity.)*
- [x] Rules from ARCHITECTURE §4.3: ordering, specificity sort + catch-all, conflict detection → confirmation, idempotency, human descriptions, command renderings.
- [ ] `insta` snapshots for ≥ 30 scenarios *(11 so far, covering every listed case; more with M3-11)*: first route on a fresh account, second zone, wildcard, path routes, conflicting A record, owned vs foreign CNAME, remove last route (offer tunnel stop/delete), rename hostname (create new → switch → delete old, zero-downtime order), and so on.
- [x] Property test: for random intents over random consistent states, `plan(apply(plan(s)))` is empty.

### M3-05 · Executor + activity log
- [x] Per-account async mutex. Staleness guard (re-observe + re-plan + compare). Config PUT with the expected version.
- [x] Step retry policy and compensation per step (ARCHITECTURE §4.4). Report `RolledBack` vs `PartiallyApplied { leftovers }`.
- [x] `activity` table and `dns_ownership` + `tunnels_local` migrations. *(`routes_meta` arrives with Disable in M3-09, where a route's definition must outlive its ingress rule.)*
- [x] Progress over a `Channel<Progress>` (`routes_apply`); the Routes query is invalidated after apply. `EntityChanged` is emitted at the end.
- [x] Tests with the fake `CloudApi`: failure injected at every step index → the state is restored (property test).

### M3-06 · Verifier
- [x] Staged probe (DNS via the API — not DoH, D-040 — edge, tunnel, origin) per ARCHITECTURE §4.5, with Cloudflare error-page detection (1033, 1016, 502, 530) mapped to stages.
- [x] Runs after apply and on demand ("Test route"). *(`Engine::verify`; wired to the UI in M3-09.)*

### M3-07 · Machine tunnel & connector integration
- [x] `EnsureMachineTunnel`: find our tunnel for this machine (by `tunnels_local`), else create one named after the Mac's name ("Krishna's MacBook Pro", sanitised). Fetch the run token into the keychain. Start a Session connector (the supervisor from M1) with a stable metrics port. *(Named after the host name; a taken name gets " 2", " 3"… Never adopts an existing tunnel by name: another Mac with the same name would then share it.)*
- [x] Connector status flows into routes (a route is healthy only if its connector is healthy and verify passes).

### M3-08 · Drift detection
- [x] Store `last_applied_version` per tunnel. On observe, a higher version means drift → compute a diff (our last applied config vs current) → `Drift` issue with **Keep theirs** (adopt, update metadata) / **Restore mine** (plan a PUT). *(`Engine::drift`, `keep_theirs`, `Intent::RestoreConfig`; the last applied ingress is stored for the diff. Edits that change no route are adopted silently. UI in M3-09.)*

### M3-09 · UI: Routes
- [x] Routes list grouped by domain (default) or by tunnel. The row shows hostname, origin, and a status dot with a label.
- [x] Inspector: URL (CopyField/Open), origin, health breakdown (connector / DNS / edge / origin), options, recent activity, and actions (Test, Edit, Disable, Remove). *(Disable and the origin options editor come later.)*
- [x] **Add route sheet (⌘N):** OriginPicker (detected services first) → HostnameInput (subdomain + zone combobox, live validation and conflict preview) → optional path → "Advanced" disclosure (all origin options) → **Review**, which shows the PlanPreview (steps, warnings, per-step "Copy as command") → **Apply** with a ProgressChecklist → success state with the verified URL.
- [x] Remove flow with a cascade preview. Reorder via drag (Advanced mode only). *(Reorder deferred: rules are specificity-sorted; Disable deferred with `routes_meta`.)*
- [x] Undo: after apply, a toast offers "Undo" for 10 s (plans the inverse intent).

### M3-10 · UI: Tunnels (Advanced)
- [ ] All tunnels in the account: name, status, connectors (colo, version, origin IP, machine = this Mac?), created, route count, "managed by Teitunnel" badge.
- [ ] Actions: start/stop (this machine), rename, delete with a cascade plan, clean stale connections.

### M3-11 · Tests
- [x] Vitest flows for add/remove/rename with mockIPC. *(add, confirm, field errors, remove, drift)*
- [ ] E2E (fake CloudApi via a test build feature + fake-cloudflared): full add → verify (stubbed) → remove.
- [ ] Nightly real-account job (dedicated test zone, API token in CI secrets): create 2 routes in 2 zones, verify HTTP 200 through the tunnel, delete everything, then assert zero records and no tunnel remain.
