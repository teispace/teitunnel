//! What the inspector does for a share or route this server serves, for agents testing
//! how an app copes: mock responses, failures, a slow network, header rules and
//! CORS, watched paths, idle stop and recording. Changes apply at once to that share or
//! route only, on this computer (nothing at Cloudflare), and the person approves them
//! first in `ask` mode.

use std::time::Duration;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::inspect::{
    Inspector, NetworkPreset, TapPatch, TapView,
    lens::{FaultAction, FaultRule, HeaderOp, HeaderRules, PathPattern, StubMode, StubRule, TapId},
};

use crate::{
    backend::BoxFuture,
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolProvider,
        ToolResult, ToolSpec, arguments,
    },
    tools::{Hints, spec},
};

/// The inspection tools, over this server's inspector.
#[derive(Clone)]
pub struct InspectionTools {
    inspector: Inspector,
}

impl std::fmt::Debug for InspectionTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InspectionTools").finish_non_exhaustive()
    }
}

impl InspectionTools {
    /// Tools over `inspector`'s shares and routes.
    pub fn new(inspector: Inspector) -> Self {
        Self { inspector }
    }

    /// The tap for `scope`: its id, the share's URL, or a hostname.
    fn resolve(&self, scope: &str) -> Result<TapView, ToolError> {
        let scope = scope.trim();
        let host = scope
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(scope)
            .to_ascii_lowercase();
        let taps = self.inspector.taps();
        taps.iter()
            .find(|tap| tap.id.as_str() == scope)
            .or_else(|| {
                taps.iter().find(|tap| {
                    tap.name.eq_ignore_ascii_case(&host)
                        || tap.public_url.as_deref().is_some_and(|url| {
                            url.trim_start_matches("https://")
                                .split('/')
                                .next()
                                .is_some_and(|h| h.eq_ignore_ascii_case(&host))
                        })
                })
            })
            .cloned()
            .ok_or_else(|| {
                let known: Vec<String> = taps
                    .iter()
                    .map(|t| t.public_url.clone().unwrap_or_else(|| t.name.clone()))
                    .collect();
                ToolError::new(if known.is_empty() {
                    "Nothing this server shares is inspected. Share something first (share_port, share_folder or expose_mcp_server).".to_owned()
                } else {
                    format!(
                        "No share or route of this server matches \"{scope}\". It inspects: {}.",
                        known.join(", ")
                    )
                })
            })
    }
}

/// A canned response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Stub {
    /// Method to match (any when left out).
    #[serde(default)]
    method: Option<String>,
    /// Path: exact (`/health`), a glob (`/api/*`), or a regex prefixed with `re:`.
    path: String,
    /// `always` (never reaches the service) or `whenDown` (only while the service can't
    /// be reached; the default).
    #[serde(default)]
    when: Option<String>,
    /// Status (200–599).
    status: u16,
    /// `Content-Type` of the body (default `application/json`).
    #[serde(default)]
    content_type: Option<String>,
    /// The body.
    #[serde(default)]
    body: String,
}

/// A failure injected into a share of matching requests.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Fault {
    /// Method to match (any when left out).
    #[serde(default)]
    method: Option<String>,
    /// Path: exact, a glob (`/api/*`) or `re:` regex.
    path: String,
    /// Share of matching requests, 0–100.
    percent: f64,
    /// `status` (answer with `status`), `reset` (drop the connection), `delay` (answer
    /// late by `ms`) or `timeout` (hold `ms`, then 504).
    kind: String,
    /// For `status`: the code (400–599).
    #[serde(default)]
    status: Option<u16>,
    /// For `delay` and `timeout`: milliseconds.
    #[serde(default)]
    ms: Option<u64>,
}

/// A header change.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HeaderChange {
    /// `set` (replace), `append` or `remove`.
    op: String,
    /// Header name.
    name: String,
    /// Value (not for `remove`).
    #[serde(default)]
    value: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SettingsArgs {
    /// A share or route this server inspects: its URL, hostname or id. Left out: list
    /// them all.
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ConfigureArgs {
    /// The share or route: its URL, hostname or id.
    scope: String,
    /// Canned responses, replacing the list (`[]` removes them).
    #[serde(default)]
    stubs: Option<Vec<Stub>>,
    /// Injected failures, replacing the list (`[]` removes them).
    #[serde(default)]
    faults: Option<Vec<Fault>>,
    /// A simulated network: `off`, `3g`, `4g` or `satellite`.
    #[serde(default)]
    network: Option<String>,
    /// Request header changes, replacing the list.
    #[serde(default)]
    request_headers: Option<Vec<HeaderChange>>,
    /// Response header changes, replacing the list.
    #[serde(default)]
    response_headers: Option<Vec<HeaderChange>>,
    /// Allow calls from any site (CORS) for development.
    #[serde(default)]
    cors: Option<bool>,
    /// Paths that notify the person when requested (`/webhooks/*`), replacing the list.
    #[serde(default)]
    watched_paths: Option<Vec<String>>,
    /// Stop the share after this many minutes without a request (0: never).
    #[serde(default)]
    idle_stop_minutes: Option<u32>,
    /// Record requests (off passes them through without keeping them).
    #[serde(default)]
    capturing: Option<bool>,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// What the inspector does for one share or route.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Settings {
    /// Its id.
    id: String,
    /// Its name (hostname or URL).
    name: String,
    /// The public URL, when known.
    public_url: Option<String>,
    /// The local service.
    origin: String,
    /// Requests are recorded.
    capturing: bool,
    /// The paused page is served.
    paused: bool,
    stubs: Vec<Stub>,
    faults: Vec<Fault>,
    /// `off`, `3g`, `4g`, `satellite` or `custom`.
    network: String,
    request_headers: Vec<HeaderChange>,
    response_headers: Vec<HeaderChange>,
    cors: bool,
    watched_paths: Vec<String>,
    idle_stop_minutes: Option<u32>,
    /// Requests seen since it started.
    requests: u64,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SettingsOut {
    /// Each share or route asked for (all of them without a scope).
    inspected: Vec<Settings>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfigureOut {
    /// `configured`, `needsApproval` or `declined`.
    outcome: String,
    /// What happened.
    message: String,
    /// The settings now.
    settings: Option<Settings>,
}

fn network_name(tap: &TapView) -> String {
    let n = &tap.network;
    if n.latency.is_none() && n.up_bytes_per_sec.is_none() && n.down_bytes_per_sec.is_none() {
        return "off".into();
    }
    for (name, preset) in [
        ("3g", NetworkPreset::ThreeG),
        ("4g", NetworkPreset::FourG),
        ("satellite", NetworkPreset::Satellite),
    ] {
        if preset.config() == *n {
            return name.into();
        }
    }
    "custom".into()
}

fn header_changes(ops: &[HeaderOp]) -> Vec<HeaderChange> {
    ops.iter()
        .map(|op| match op {
            HeaderOp::Set { name, value } => HeaderChange {
                op: "set".into(),
                name: name.clone(),
                value: Some(value.clone()),
            },
            HeaderOp::Append { name, value } => HeaderChange {
                op: "append".into(),
                name: name.clone(),
                value: Some(value.clone()),
            },
            HeaderOp::Remove { name } => HeaderChange {
                op: "remove".into(),
                name: name.clone(),
                value: None,
            },
        })
        .collect()
}

fn settings(tap: &TapView) -> Settings {
    Settings {
        id: tap.id.to_string(),
        name: tap.name.clone(),
        public_url: tap.public_url.clone(),
        origin: tap.origin.clone(),
        capturing: tap.capturing,
        paused: tap.paused.is_some(),
        stubs: tap
            .stubs
            .iter()
            .map(|s| Stub {
                method: s.method.clone(),
                path: s.path.as_text(),
                when: Some(match s.mode {
                    StubMode::Always => "always".into(),
                    StubMode::WhenUnreachable => "whenDown".into(),
                }),
                status: s.status,
                content_type: s
                    .headers
                    .iter()
                    .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.clone()),
                body: s.body.clone(),
            })
            .collect(),
        faults: tap
            .faults
            .iter()
            .map(|f| {
                let (kind, status, ms) = match &f.action {
                    FaultAction::Status { status, .. } => ("status", Some(*status), None),
                    FaultAction::Reset => ("reset", None, None),
                    FaultAction::Delay { ms } => ("delay", None, Some(*ms)),
                    FaultAction::Timeout { after_ms } => ("timeout", None, Some(*after_ms)),
                };
                Fault {
                    method: f.method.clone(),
                    path: f.path.as_text(),
                    percent: f.percent,
                    kind: kind.into(),
                    status,
                    ms,
                }
            })
            .collect(),
        network: network_name(tap),
        request_headers: header_changes(&tap.header_rules.request),
        response_headers: header_changes(&tap.header_rules.response),
        cors: tap.header_rules.cors,
        watched_paths: tap.watched_paths.clone(),
        idle_stop_minutes: tap.idle_stop_minutes,
        requests: tap.requests,
    }
}

fn pattern(path: &str) -> Result<PathPattern, ToolError> {
    PathPattern::parse(path.trim()).map_err(|e| ToolError::new(format!("{path}: {e}")))
}

fn stub_rule(stub: &Stub) -> Result<StubRule, ToolError> {
    let mode = match stub.when.as_deref() {
        None | Some("whenDown") => StubMode::WhenUnreachable,
        Some("always") => StubMode::Always,
        Some(other) => {
            return Err(ToolError::new(format!(
                "Stub `when` is `always` or `whenDown`, not \"{other}\"."
            )));
        }
    };
    let rule = StubRule {
        method: stub.method.clone().filter(|m| !m.trim().is_empty()),
        path: pattern(&stub.path)?,
        mode,
        status: stub.status,
        headers: vec![(
            "Content-Type".into(),
            stub.content_type
                .clone()
                .unwrap_or_else(|| "application/json".into()),
        )],
        body: stub.body.clone(),
    };
    rule.validate().map_err(|e| ToolError::new(e.to_string()))?;
    Ok(rule)
}

fn fault_rule(fault: &Fault) -> Result<FaultRule, ToolError> {
    let ms = || {
        fault
            .ms
            .ok_or_else(|| ToolError::new(format!("A `{}` fault needs `ms`.", fault.kind)))
    };
    let action = match fault.kind.as_str() {
        "status" => FaultAction::Status {
            status: fault
                .status
                .ok_or_else(|| ToolError::new("A `status` fault needs `status`."))?,
            retry_after_secs: None,
        },
        "reset" => FaultAction::Reset,
        "delay" => FaultAction::Delay { ms: ms()? },
        "timeout" => FaultAction::Timeout { after_ms: ms()? },
        other => {
            return Err(ToolError::new(format!(
                "A fault's `kind` is status, reset, delay or timeout, not \"{other}\"."
            )));
        }
    };
    let rule = FaultRule {
        method: fault.method.clone().filter(|m| !m.trim().is_empty()),
        path: pattern(&fault.path)?,
        percent: fault.percent,
        action,
    };
    rule.validate().map_err(|e| ToolError::new(e.to_string()))?;
    Ok(rule)
}

fn header_ops(changes: &[HeaderChange]) -> Result<Vec<HeaderOp>, ToolError> {
    changes
        .iter()
        .map(|c| {
            let value = || {
                c.value
                    .clone()
                    .ok_or_else(|| ToolError::new(format!("`{}` needs a value.", c.op)))
            };
            Ok(match c.op.as_str() {
                "set" => HeaderOp::Set {
                    name: c.name.clone(),
                    value: value()?,
                },
                "append" => HeaderOp::Append {
                    name: c.name.clone(),
                    value: value()?,
                },
                "remove" => HeaderOp::Remove {
                    name: c.name.clone(),
                },
                other => {
                    return Err(ToolError::new(format!(
                        "A header `op` is set, append or remove, not \"{other}\"."
                    )));
                }
            })
        })
        .collect()
}

/// The change as a patch, and what it does in words (for the approval).
fn patch(args: &ConfigureArgs, current: &TapView) -> Result<(TapPatch, Vec<String>), ToolError> {
    let mut patch = TapPatch::default();
    let mut said = Vec::new();
    if let Some(stubs) = &args.stubs {
        patch.stubs = Some(stubs.iter().map(stub_rule).collect::<Result<_, _>>()?);
        said.push(match stubs.len() {
            0 => "remove the mock responses".to_owned(),
            n => format!(
                "answer {n} path{} with mock responses ({})",
                if n == 1 { "" } else { "s" },
                stubs
                    .iter()
                    .map(|s| format!("{} {}", s.status, s.path))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if let Some(faults) = &args.faults {
        patch.faults = Some(faults.iter().map(fault_rule).collect::<Result<_, _>>()?);
        said.push(match faults.len() {
            0 => "stop injecting failures".to_owned(),
            _ => format!(
                "inject failures ({})",
                faults
                    .iter()
                    .map(|f| format!("{}% of {} → {}", f.percent, f.path, f.kind))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    if let Some(network) = &args.network {
        let preset = match network.trim().to_ascii_lowercase().as_str() {
            "off" | "none" => NetworkPreset::Off,
            "3g" => NetworkPreset::ThreeG,
            "4g" => NetworkPreset::FourG,
            "satellite" => NetworkPreset::Satellite,
            other => {
                return Err(ToolError::new(format!(
                    "`network` is off, 3g, 4g or satellite, not \"{other}\"."
                )));
            }
        };
        patch.network_preset = Some(preset);
        said.push(format!("simulate a {network} network"));
    }
    if args.request_headers.is_some() || args.response_headers.is_some() || args.cors.is_some() {
        let rules = &current.header_rules;
        patch.header_rules = Some(HeaderRules {
            request: match &args.request_headers {
                Some(changes) => header_ops(changes)?,
                None => rules.request.clone(),
            },
            response: match &args.response_headers {
                Some(changes) => header_ops(changes)?,
                None => rules.response.clone(),
            },
            cors: args.cors.unwrap_or(rules.cors),
        });
        said.push("change header rules".to_owned());
        if args.cors == Some(true) {
            said.push("allow calls from any site (CORS)".to_owned());
        }
    }
    if let Some(paths) = &args.watched_paths {
        patch.watched_paths = Some(paths.clone());
        said.push(format!("notify on {}", paths.join(", ")));
    }
    if let Some(minutes) = args.idle_stop_minutes {
        patch.idle_stop_minutes = Some(minutes);
        said.push(if minutes == 0 {
            "never stop when idle".to_owned()
        } else {
            format!("stop after {minutes} idle minutes")
        });
    }
    if let Some(on) = args.capturing {
        patch.capturing = Some(on);
        said.push(
            if on {
                "record requests"
            } else {
                "stop recording requests"
            }
            .to_owned(),
        );
    }
    if said.is_empty() {
        return Err(ToolError::new(
            "Say what to change: stubs, faults, network, requestHeaders, responseHeaders, cors, watchedPaths, idleStopMinutes or capturing.",
        ));
    }
    Ok((patch, said))
}

impl InspectionTools {
    fn settings(&self, args: JsonObject) -> ToolResult {
        let args: SettingsArgs = arguments(args)?;
        let inspected = match args.scope.as_deref().filter(|s| !s.trim().is_empty()) {
            Some(scope) => vec![settings(&self.resolve(scope)?)],
            None => self.inspector.taps().iter().map(settings).collect(),
        };
        Ok(ToolOutput::new(&SettingsOut { inspected }))
    }

    async fn configure(&self, args: JsonObject, ctx: &ToolContext) -> ToolResult {
        let args: ConfigureArgs = arguments(args)?;
        let tap = self.resolve(&args.scope)?;
        let (patch, said) = patch(&args, &tap)?;
        let name = tap.public_url.clone().unwrap_or_else(|| tap.name.clone());
        let details = format!(
            "For {name} only, on this computer (nothing changes at Cloudflare): {}.",
            said.join("; ")
        );
        match ctx
            .approve(&ApprovalRequest {
                title: format!("Change how {name} answers"),
                details: details.clone(),
                confirmed: args.confirmed,
            })
            .await
        {
            Approval::Granted { .. } => {}
            Approval::NeedsConfirmation => {
                return Ok(ToolOutput::new(&ConfigureOut {
                    outcome: "needsApproval".into(),
                    message: format!(
                        "Nothing changed. Show the person this and call again with \"confirmed\": true if they agree:\n{details}"
                    ),
                    settings: None,
                }));
            }
            Approval::Declined(why) => {
                return Ok(ToolOutput::new(&ConfigureOut {
                    outcome: "declined".into(),
                    message: format!("{why} Nothing changed."),
                    settings: None,
                }));
            }
        }
        let id = TapId::new(&tap.id.to_string()).map_err(|e| ToolError::new(e.to_string()))?;
        let view = self
            .inspector
            .configure(&id, &patch)
            .map_err(|e| ToolError::new(e.to_string()))?;
        Ok(ToolOutput::new(&ConfigureOut {
            outcome: "configured".into(),
            message: format!(
                "{name} now: {}. It applies to the next request.",
                said.join("; ")
            ),
            settings: Some(settings(&view)),
        }))
    }
}

impl ToolProvider for InspectionTools {
    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            spec::<SettingsArgs, SettingsOut>(
                "inspection_settings",
                "See how a share answers",
                "What Teitunnel's inspector does for the shares and routes this server serves: mock responses, injected failures, simulated network, header rules and CORS, watched paths, idle stop, recording, and the request count. Leave out `scope` to list them all.\n\
                 \n\
                 Example: {\"scope\": \"https://quiet-river.trycloudflare.com\"}",
                ToolClass::Read,
                Hints::READ_LOCAL,
                crate::tools::DEFAULT_TIMEOUT,
            ),
            spec::<ConfigureArgs, ConfigureOut>(
                "configure_inspection",
                "Change how a share answers",
                "Test how an app copes, on one share or route this server serves (nothing changes at Cloudflare; it applies to the next request): mock responses (`stubs`, `always` or only `whenDown` so webhook senders keep getting answers while the service restarts), injected failures (`faults`: a percentage of matching requests get a status, a dropped connection, a delay or a timeout), a slow network (`network`: 3g, 4g, satellite, off), header rules and CORS, paths that notify the person, idle stop and recording. Lists replace what's there; fields left out stay. In `ask` mode the person approves first.\n\
                 \n\
                 Examples: {\"scope\": \"demo.teispace.com\", \"faults\": [{\"path\": \"/api/pay\", \"percent\": 20, \"kind\": \"status\", \"status\": 503}]} · {\"scope\": \"demo.teispace.com\", \"network\": \"3g\"} · {\"scope\": \"demo.teispace.com\", \"stubs\": [{\"path\": \"/webhooks/*\", \"status\": 200, \"body\": \"{\\\"ok\\\":true}\"}]}",
                ToolClass::Change,
                Hints {
                    read_only: false,
                    destructive: false,
                    idempotent: true,
                    open_world: false,
                },
                Duration::from_secs(30),
            ),
        ]
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: JsonObject,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            match name {
                "inspection_settings" => self.settings(arguments),
                "configure_inspection" => self.configure(arguments, ctx).await,
                other => Err(ToolError::new(format!("Unknown tool {other}."))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use teitunnel_core::inspect::{TapScope, TapSpec};

    use super::*;
    use crate::{
        config::Mode,
        tools::tests::{actor, settings as mode_settings},
    };

    fn args(value: &serde_json::Value) -> JsonObject {
        value.as_object().cloned().unwrap()
    }

    async fn inspected() -> (InspectionTools, String) {
        let inspector = Inspector::new(None, None, "app");
        let mut spec = TapSpec::new(
            TapScope::QuickShare {
                share_id: "qs-agent".into(),
            },
            "demo",
            "http://127.0.0.1:9",
        );
        spec.public_url = Some("https://quiet-river.trycloudflare.com".into());
        inspector.start(spec).await.unwrap();
        (
            InspectionTools::new(inspector),
            "https://quiet-river.trycloudflare.com".into(),
        )
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn configures_failures_mocks_and_a_slow_network() {
        let (tools, url) = inspected().await;
        let ctx = ToolContext::detached(mode_settings(Mode::Full), actor());
        let listed = tools
            .call("inspection_settings", args(&json!({})), &ctx)
            .await
            .unwrap()
            .structured;
        assert_eq!(listed["inspected"][0]["network"], "off");

        let out = tools
            .call(
                "configure_inspection",
                args(&json!({
                    "scope": url,
                    "faults": [{"path": "/api/pay", "percent": 20, "kind": "status", "status": 503}],
                    "stubs": [{"path": "/webhooks/*", "status": 200, "body": "{\"ok\":true}"}],
                    "network": "3g",
                    "cors": true,
                })),
                &ctx,
            )
            .await
            .unwrap()
            .structured;
        assert_eq!(out["outcome"], "configured", "{out}");
        let now = &out["settings"];
        assert_eq!(now["network"], "3g");
        assert_eq!(now["faults"][0]["status"], 503);
        assert_eq!(now["stubs"][0]["when"], "whenDown");
        assert_eq!(now["stubs"][0]["contentType"], "application/json");
        assert_eq!(now["cors"], true);

        // Found by hostname too; lists replace, fields left out stay.
        let out = tools
            .call(
                "configure_inspection",
                args(&json!({"scope": "quiet-river.trycloudflare.com", "faults": []})),
                &ctx,
            )
            .await
            .unwrap()
            .structured;
        assert!(out["settings"]["faults"].as_array().unwrap().is_empty());
        assert_eq!(out["settings"]["network"], "3g");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn says_what_is_wrong_and_asks_first_in_ask_mode() {
        let (tools, url) = inspected().await;
        let full = ToolContext::detached(mode_settings(Mode::Full), actor());
        for (bad, says) in [
            (
                json!({"scope": url, "network": "5g"}),
                "off, 3g, 4g or satellite",
            ),
            (
                json!({"scope": url, "faults": [{"path": "/x", "percent": 10, "kind": "delay"}]}),
                "needs `ms`",
            ),
            (
                json!({"scope": "nope.example.com", "network": "3g"}),
                "It inspects",
            ),
            (json!({"scope": url}), "Say what to change"),
        ] {
            let err = tools
                .call("configure_inspection", args(&bad), &full)
                .await
                .unwrap_err();
            assert!(err.to_string().contains(says), "{err}");
        }
        let ask = ToolContext::detached(mode_settings(Mode::Ask), actor());
        let out = tools
            .call(
                "configure_inspection",
                args(&json!({"scope": url, "network": "4g"})),
                &ask,
            )
            .await
            .unwrap()
            .structured;
        assert_eq!(out["outcome"], "needsApproval");
        assert!(
            out["message"]
                .as_str()
                .unwrap()
                .contains("simulate a 4g network")
        );
        let listed = tools
            .call("inspection_settings", args(&json!({"scope": url})), &full)
            .await
            .unwrap()
            .structured;
        assert_eq!(listed["inspected"][0]["network"], "off", "nothing changed");
    }
}
