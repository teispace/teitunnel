---
name: adding-mcp-tools
description: Adds or changes a tool, prompt or resource in Teitunnel's MCP server (crates/mcp), including its argument schema, safety class and hints, the Backend method in core_backend.rs, the fake backend, tool-count tests and the AI agents docs. Use when AI agents need a new capability or an existing MCP tool changes behaviour.
---

# Adding an MCP tool

Teitunnel's MCP server (`crates/mcp`) lets AI agents use Teitunnel. It's served over stdio by
`teitunnel mcp` and over Streamable HTTP by `teitunnel serve`. Agents are powerful and can be
misled by what they read, so every tool is designed for safety first.

## Rules

- **Secrets never reach the agent.** Tokens, keys and credentials are never arguments or
  results. Captured traffic is masked (`redaction.rs`) unless the server runs with
  `--allow-secrets`.
- **Changes wait for the person.** A tool that changes something is `ToolClass::Change` or
  `ToolClass::Destructive`; in `ask` mode the person approves it (in the app, or through the
  client). A Cloudflare change returns a plan and is applied with `apply_plan` after approval;
  it never calls the API directly (see `changing-cloudflare-resources`).
- **Bounded.** Every tool has a timeout; list results are paginated or capped.

## Checklist

```
- [ ] 1. Behaviour in crates/core, tested there
- [ ] 2. Backend method (backend.rs) with an Unsupported default
- [ ] 3. Implemented in core_backend.rs, and in FakeBackend (tools/tests.rs)
- [ ] 4. Args and result types with JsonSchema and doc comments
- [ ] 5. spec::<Args, Result>(…) in the area's tools/<area>.rs
- [ ] 6. Dispatched in tools.rs
- [ ] 7. Tests, including the tool counts
- [ ] 8. Docs: ai-agents.mdx and the product skill
- [ ] 9. pnpm verify
```

## 2–3. Backend

`Backend` (`crates/mcp/src/backend.rs`) is what tools call. Add a method with a default that returns
`Unsupported`, so other backends keep compiling. Implement it in `CoreBackend`
(`crates/mcp/src/core_backend.rs`) by calling `teitunnel_core`, and in `FakeBackend`
(`crates/mcp/src/tools/tests.rs`) with deterministic data for tests.

## 4. Arguments and results

```rust
/// Which route, over how long.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RouteTrafficArgs {
    /// The route's or share's hostname.
    hostname: String,
    /// Only requests under this path (`/api`), for a route with a path rule.
    #[serde(default)]
    path: Option<String>,
}
```

Doc comments become the JSON schema descriptions the agent reads: make each one precise,
with units and defaults. Use `deny_unknown_fields`, `camelCase`, and validated types
(hostnames, ports) rather than free strings where the core has them.

## 5. The spec

```rust
spec::<RouteTrafficArgs, RouteTrafficResult>(
    "route_traffic",
    "Route traffic",
    "What it returns and where the numbers come from.\n\
     \n\
     Use it for \"how much traffic does my site get?\". For single requests use traffic_list.\n\
     \n\
     Example: {\"hostname\": \"app.teispace.com\", \"range\": \"day\"}",
    ToolClass::Read,
    Hints::READ_CLOUD,
    Duration::from_secs(60),
),
```

The description is the agent's only manual. Write it as: what it does → when to use it (and
which neighbouring tool to use instead) → one example call. Pick the class honestly:
`Read`, `Wait` (blocks until something happens, bounded), `Change` (additive or reversible),
`Destructive` (removes or replaces).

## 6. Dispatch

Add the name to the `match` in `crates/mcp/src/tools.rs` that routes calls to the area module.

## 7. Tests

- A test calling the tool through the fake backend: arguments, result shape, and the error
  for invalid input (see `crates/mcp/src/tools/tests.rs` and `crates/mcp/src/protocol_tests.rs`).
- A change tool: a test that it asks for approval in `ask` mode and is refused in
  `read-only` mode.
- Update the tool counts: `assert_eq!(tools.len(), …)` in `crates/mcp/src/protocol_tests.rs`
  and `assert_eq!(names.len(), …)` in `apps/cli/tests/mcp.rs`.

## 8. Docs

- The tool table in `apps/web/content/docs/guides/ai-agents.mdx`.
- The Agent Skill users give their agents, `apps/web/public/skills/teitunnel/SKILL.md`, when
  the tool changes how an agent should work with Teitunnel.

## 9. Verify

Run `pnpm verify`. To try it by hand, run `teitunnel mcp --help` and connect an MCP client to
a development build.
