---
name: changing-cloudflare-resources
description: Implements a change Teitunnel makes on a Cloudflare account (tunnels, DNS records, Access, Workers, D1, rulesets, service tokens) through the observe → plan → apply → verify engine in crates/core/src/engine, with rollback, ownership tracking, the cf-api client, fakes and tests. Use for any feature or fix that creates, updates or deletes something on Cloudflare, or adds a Cloudflare API call.
---

# Changing Cloudflare resources

Teitunnel changes real accounts. Every change goes through one pipeline so the person sees it
first, it can fail halfway without leaving a mess, and it can be undone. **No IPC command, CLI
command or MCP tool calls the Cloudflare API to change something directly.**

Background: [docs/ARCHITECTURE.md §4](../../../docs/ARCHITECTURE.md) (the engine) and §4.7
(ownership). Read them before a non-trivial change.

```
Intent ─┐
        ├─▶ plan() (pure) ─▶ Plan ─▶ person reviews ─▶ Engine::apply ─▶ verify
Snapshot┘                                                   │
                                                  Activity + undo steps
```

## Checklist

```
- [ ] 1. API calls in crates/cf-api, with wiremock tests
- [ ] 2. CloudApi trait method, implemented for Client and FakeCloud
- [ ] 3. Observe what the plan depends on (Snapshot)
- [ ] 4. Intent variant (what the person asked for)
- [ ] 5. Step variant(s): describe(), command(), and an Undo
- [ ] 6. Planner: ordering, idempotency, conflicts, ownership
- [ ] 7. Ownership recorded (only what Teitunnel created is ever deleted)
- [ ] 8. Permissions: token template, OAuth scope, capability probe, docs
- [ ] 9. Tests: planner snapshots, executor rollback, fake-cloudflare
- [ ] 10. Callers use preview → apply (IPC, CLI, MCP)
- [ ] 11. pnpm verify
```

## 1. The API call (`crates/cf-api`)

One module per product (`dns.rs`, `tunnels.rs`, `workers.rs`, `rulesets.rs`, `d1.rs`…).
Requests are typed; responses go through the envelope in `envelope.rs`; errors are
`cf_api::Error`. Link the endpoint's Cloudflare documentation in the doc comment. Test each
endpoint with `wiremock`: a recorded success response and the error-envelope case.

## 2. `CloudApi` (`engine/cloud.rs`)

The engine depends on the `CloudApi` trait, not on `Client`. Add a method, implement it for
`Client` (in `cloud.rs`) and for `FakeCloud` (`engine/fake.rs`), whose in-memory state the
engine tests assert against. Keep the fake faithful: the same refusals and shapes as the
real API.

## 3–4. Observe and intend

- `Intent` (`engine/types.rs`) says what the person wants in product terms
  (`AddRoute`, `ProtectHostname`, …), never in API steps.
- The observed `Snapshot` holds everything the plan depends on, and its fingerprint changes
  when any of it changes, so a stale plan is re-planned before it's applied.

## 5. Steps and undo

A `Step` (`engine/types.rs`) is one Cloudflare or local operation. Each step:

- has `describe()` returning a translated `Text` (see `writing-user-facing-text`) and
  `command()` rendering it as a `cloudflared` or `curl` command where one exists;
- is executed in `Run::step` (`engine/executor.rs`), which returns an `Undo`;
- has a compensating `Undo` that restores the previous state (delete what was created,
  put back the previous config, restore a deleted Worker from its recorded config).

On failure, completed steps are undone in reverse order. A step that can't be undone must be
last, or be explicitly confirmed by the person.

Consecutive `Step::CreateRecord` steps aren't run one by one: `create_records` in
`engine/executor.rs` sends them as one batch per zone (`CloudApi::create_records`). A test
that counts `FakeCloud::mutations()` sees one mutation per zone for them.

## 6. The planner (`engine/planner.rs`, `engine/planner/*.rs`)

`plan(intent, snapshot)` is pure: no I/O, no clock, deterministic. It must:

- **order** steps safely: create → configure → DNS → start → verify; removal in reverse;
- be **idempotent**: planning against the converged state returns an empty plan;
- **never overwrite silently**: an existing record, rule, Worker route or Access app that
  Teitunnel doesn't own sets `requires_confirmation` and is shown to the person;
- scope edge rules and Workers to the hostname, never to a whole domain;
- add a cost or plan-limit warning when the change can cost money or needs a paid plan.

## 7. Ownership

Teitunnel deletes only what it created. Record everything it creates: DNS records carry the
comment `teitunnel:route=<id>`, and the store keeps ownership indexes (tunnels, records,
edge rules, Workers, Access apps, D1). New kinds of resource need their own index: append a
migration to `crates/core/src/store/migrations.rs` (never edit an existing one). Removing a
hostname's last route removes everything Teitunnel attached to it.

## 8. Permissions

A new API needs a permission:

- the token template (`accounts/template.rs`) and, for sign-in, the OAuth scope
  (`accounts/oauth.rs`): verify the exact key and scope on Cloudflare's dashboard first;
- a probe in `accounts/capabilities.rs` so the UI can disable the feature with the reason,
  and ask for the permission in place, instead of failing mid-plan;
- keep the permission optional unless every account needs it, so existing tokens keep working;
- regenerate the permissions page:
  `UPDATE_DOCS=1 cargo test -p teitunnel-core --test permissions_doc`.

## 9. Tests

- **Planner:** an `insta` snapshot per scenario in `engine/planner_tests.rs` (or the area's
  `*_tests.rs`): create, change, remove, the already-converged case (empty plan), and a
  resource someone else owns. Review snapshots with `cargo insta review`; never accept blindly.
- **Executor:** a step failing halfway rolls back the earlier ones (`engine/executor_tests.rs`).
- **End to end:** `tools/fake-cloudflare` serves the HTTP API for CLI and E2E tests; extend it
  when the CLI or app exercises the new endpoints.
- Never test against a real account.

## 10. Callers

The app (`src-tauri/src/ipc/routes.rs` and friends), the CLI and the MCP server all use
`Engine::preview` to get a plan and show it, then `Engine::apply` with the same intent and an
`Approval` carrying the reviewed plan's fingerprint (and `confirmed` when the person agreed to
replace something they own). If reality changed since the preview, the fingerprint no longer
matches and the person reviews again. CLI commands show the plan and ask unless `--yes`; MCP
tools return the plan and apply it after the person's approval.

## 11. Verify

`pnpm verify`, then check the change in the app against the E2E fake
(`pnpm --filter @teitunnel/desktop e2e:build && pnpm --filter @teitunnel/desktop e2e`).
