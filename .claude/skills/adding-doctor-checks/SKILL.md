---
name: adding-doctor-checks
description: Adds a problem the Teitunnel Doctor detects (crates/core/src/doctor.rs), with the facts it gathers, a pure diagnosis, a translated title and detail, a one-click fix that goes through the plan engine, notification behaviour, tests and the Doctor reference page. Use when a recurring failure should be found and explained automatically instead of left to the person.
---

# Adding a Doctor check

The Doctor turns failures into a sentence and a fix. It runs at start-up, after every applied
change, when a connector changes state, and on demand; `doctor_monitor.rs` notifies about new
errors in the background.

Gathering and judging are separate: `gather` collects facts (I/O), `diagnose` is pure. That
keeps every check fast to test.

## Checklist

```
- [ ] 1. The fact: added to Facts / AccountFacts and filled in gather
- [ ] 2. The judgement in diagnose, with an id area.problem
- [ ] 3. Title, detail and evidence as catalog text
- [ ] 4. A fix, through the engine when it changes Cloudflare
- [ ] 5. Notification behaviour (QUIET_CHECKS)
- [ ] 6. Tests in doctor.rs
- [ ] 7. The Doctor reference page
- [ ] 8. pnpm verify
```

## 1. The fact

If the check needs information the Doctor doesn't collect yet, add it to `Facts` or
`AccountFacts` in `crates/core/src/doctor.rs` and fill it in `gather` (or `run`). Reuse the
observed engine snapshot; don't make an extra API call per check.

## 2–3. The judgement

In `diagnose`:

```rust
found.add(
    "dns.missing",                 // area.problem; stable, used to ignore it
    Severity::Error,
    hostname,                      // the subject
    msg::doctor::dns_missing::title(hostname),
    msg::doctor::dns_missing::detail(hostname),
    vec![/* evidence as Text */],
    vec![/* fixes */],
);
```

- The id is `area.problem`. The issue's identity adds the account and subject, so "Ignore"
  survives restarts. Never rename an id.
- The **title** says what's wrong in one sentence; the **detail** says what it means and
  what to do. Both come from `locales/en.json` under `core.doctor` (see
  `writing-user-facing-text`).
- Severity: `Error` when something people rely on is broken, `Warning` when it will break
  or is wasteful, `Info` for advice.
- Avoid false positives: a check that fires wrongly teaches people to ignore the Doctor.
  When a condition is transient (a connector reconnecting, DNS propagating), wait for it to
  persist.

## 4. The fix

- A fix that changes Cloudflare is `Fix::Change { label, change }`, so it goes through the
  planner, is shown as a plan and can be undone (see `changing-cloudflare-resources`).
- Local fixes (start a connector, clean stale connections) have their own `Fix` variants.
- `fix_safe` backs **Fix Safe Issues**: include a fix there only if its plan touches
  nothing but what Teitunnel owns and needs no confirmation.
- A problem only the person can solve (reconnect an account, install something) gets
  guidance instead of a fix.

## 5. Notifications

Errors notify once when they appear. If the problem is already announced another way, or
flaps with the network, add its id to `QUIET_CHECKS` in `crates/core/src/doctor_monitor.rs`.

## 6. Tests

In `doctor.rs`'s tests: build `Facts`, call `diagnose`, and assert the issue, its severity and
its fix. Add the healthy case that must not fire.

## 7. Docs

Add the check to `apps/web/content/docs/reference/doctor.mdx`: what it means and how the fix
works.

## 8. Verify

```sh
CARGO_INCREMENTAL=0 cargo nextest run -p teitunnel-core -E 'test(doctor)'
pnpm verify
```
