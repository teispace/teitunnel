//! Setup tools: accounts and what they can do, exports, existing cloudflared setups,
//! and the Activity log.

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::export::ExportFormat;

use super::{AccountRef, Hints, NoArgs, account, spec, tunnel_id};
use crate::{
    backend::SharedBackend,
    limits,
    registry::{ToolClass, ToolError, ToolOutput, ToolResult, ToolSpec, arguments},
};

/// Largest export returned in full (characters).
const MAX_EXPORT: usize = 200_000;

pub(super) fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<ExportArgs, ExportResult>(
            "export_config",
            "Export a tunnel's config",
            "Export one of this machine's tunnels with its routes as a file to run it elsewhere: a cloudflared `config.yml`, a Docker Compose service, or Terraform (Cloudflare provider v5, with `import` blocks so applying it changes nothing). Never contains a secret: the tunnel token is referenced, not included.\n\
             \n\
             Use it to move this machine's routes to a server, keep them in git, or hand them to infrastructure as code. With Compose or config.yml, the person sets the token on the server (e.g. `TUNNEL_TOKEN`); tell them.\n\
             \n\
             Example: {\"format\": \"dockerCompose\"}",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<NoArgs, ImportResult>(
            "import_scan",
            "Find existing cloudflared setups",
            "Find cloudflared setups already on this machine (config files in ~/.cloudflared, /etc/cloudflared and those of running cloudflared processes): their tunnel, account and routes, and which routes Teitunnel can take over.\n\
             \n\
             To bring routes under Teitunnel, plan an `importRoutes` change with the routes you want (plan_change), and show the person the plan.",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<AccountsArgs, AccountsResult>(
            "accounts",
            "List accounts",
            "List the Cloudflare accounts connected to Teitunnel (never their credentials), and optionally what each credential can do: read and edit tunnels, edit DNS per domain, manage logins (Access).\n\
             \n\
             Pass an account's name or id as `account` to other tools when several are connected. If a capability is missing, the person fixes it in the Teitunnel app (Settings → Accounts).",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<ActivityArgs, ActivityResult>(
            "recent_activity",
            "Recent changes",
            "List the most recent changes Teitunnel made in an account (newest first): what was asked, whether it applied, what changed (routes, DNS records, logins, networks), and who asked (an agent's name, or a person). Agents' changes are recorded here like everyone's.\n\
             \n\
             Use it to see what happened before a problem, or to pick an `entryId` for undo_last.",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
    ]
}

/// What to export.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExportArgs {
    /// `configYaml`, `dockerCompose` or `terraform`.
    format: FormatInput,
    /// Account name or id; needed only when several accounts are connected.
    #[serde(default)]
    account: Option<String>,
    /// One of this machine's tunnels (name or id). Default: the default tunnel.
    #[serde(default)]
    tunnel: Option<String>,
}

/// Export formats.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum FormatInput {
    /// A cloudflared `config.yml`.
    ConfigYaml,
    /// A Docker Compose service.
    DockerCompose,
    /// Terraform (Cloudflare provider v5).
    Terraform,
}

impl From<FormatInput> for ExportFormat {
    fn from(format: FormatInput) -> Self {
        match format {
            FormatInput::ConfigYaml => Self::ConfigYaml,
            FormatInput::DockerCompose => Self::DockerCompose,
            FormatInput::Terraform => Self::Terraform,
        }
    }
}

/// An export.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportResult {
    /// The account.
    account: AccountRef,
    /// Suggested file name.
    file_name: String,
    /// The file.
    contents: String,
    /// Cut short (it was very large).
    truncated: bool,
}

pub(super) async fn export_config(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: ExportArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let tunnel = tunnel_id(backend, &account.id, args.tunnel.as_deref()).await?;
    let file = backend
        .export(&account.id, tunnel.as_deref(), args.format.into())
        .await?
        .ok_or_else(|| {
            ToolError::new(format!(
                "This machine has no tunnel with routes in {} yet, so there's nothing to export.",
                account.name
            ))
        })?;
    let truncated = file.contents.chars().count() > MAX_EXPORT;
    let contents = if truncated {
        file.contents.chars().take(MAX_EXPORT).collect()
    } else {
        file.contents
    };
    Ok(ToolOutput::new(&ExportResult {
        account: (&account).into(),
        file_name: file.file_name,
        contents,
        truncated,
    }))
}

/// A route found in a setup.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FoundRouteOut {
    /// Hostname.
    hostname: String,
    /// Path regex.
    path: Option<String>,
    /// The service.
    service: String,
    /// Why it can't be imported, if it can't.
    unsupported: Option<String>,
}

/// A setup.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupOut {
    /// The config file.
    config_path: String,
    /// Its `tunnel:` (id or name).
    tunnel: Option<String>,
    /// The Cloudflare account of its credentials.
    account_id: Option<String>,
    /// The tunnel id of its credentials.
    tunnel_id: Option<String>,
    /// Routes, in order.
    routes: Vec<FoundRouteOut>,
    /// It has settings for every route that can't be carried over.
    has_global_options: bool,
    /// A problem reading it.
    problem: Option<String>,
}

/// Setups found.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportResult {
    /// Every setup found.
    setups: Vec<SetupOut>,
}

pub(super) async fn import_scan(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let NoArgs {} = arguments(args)?;
    let setups = backend
        .import_scan()
        .await?
        .into_iter()
        .map(|s| SetupOut {
            config_path: s.config_path,
            tunnel: s.tunnel,
            account_id: s.account_id,
            tunnel_id: s.tunnel_id,
            routes: s
                .routes
                .into_iter()
                .map(|r| FoundRouteOut {
                    hostname: r.hostname,
                    path: r.path,
                    service: r.service,
                    unsupported: r
                        .unsupported
                        .as_ref()
                        .map(teitunnel_core::text::Text::english),
                })
                .collect(),
            has_global_options: s.has_global_options,
            problem: s.problem.as_ref().map(teitunnel_core::text::Text::english),
        })
        .collect();
    Ok(ToolOutput::new(&ImportResult { setups }))
}

/// Options.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountsArgs {
    /// Also probe what each credential can do (a few read-only API calls per account).
    #[serde(default)]
    capabilities: bool,
}

/// An account.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountOut {
    /// Account id.
    id: String,
    /// Name.
    name: String,
    /// How it's connected: `oAuth`, `apiToken` or `certPem` (never the credential).
    credential: String,
    /// For cert.pem: the only domain it works for.
    limited_zone: Option<String>,
    /// What the credential can do, when asked.
    capabilities: Option<serde_json::Value>,
}

/// Accounts.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountsResult {
    /// Connected accounts.
    accounts: Vec<AccountOut>,
}

pub(super) async fn accounts(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: AccountsArgs = arguments(args)?;
    let mut out = Vec::new();
    for account in backend.accounts().await? {
        let capabilities = if args.capabilities {
            Some(
                backend
                    .capabilities(&account.id)
                    .await
                    .unwrap_or_else(|e| serde_json::json!({ "error": e.to_string() })),
            )
        } else {
            None
        };
        out.push(AccountOut {
            credential: serde_json::to_value(account.credential)
                .ok()
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
                .unwrap_or_default(),
            id: account.id,
            name: account.name,
            limited_zone: account.limited_zone,
            capabilities,
        });
    }
    Ok(ToolOutput::new(&AccountsResult { accounts: out }))
}

/// Which activity.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ActivityArgs {
    /// Account name or id; needed only when several accounts are connected.
    #[serde(default)]
    account: Option<String>,
    /// Only changes made by agents.
    #[serde(default)]
    agents_only: bool,
    /// At most this many (default 20, up to 200).
    #[serde(default)]
    limit: Option<usize>,
}

/// A change.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct EntryOut {
    /// Pass to undo_last as `entryId`.
    id: i64,
    /// When (milliseconds since the epoch).
    at: i64,
    /// What was asked.
    summary: String,
    /// `applied`, `rolledBack` or `partiallyApplied`.
    outcome: String,
    /// Who: an agent's name, or `a person`.
    by: String,
    /// Hostnames involved.
    hostnames: Vec<String>,
    /// What changed: `area hostname: before → after`.
    changes: Vec<String>,
    /// Why it failed, if it did.
    error: Option<String>,
}

/// Recent changes.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ActivityResult {
    /// The account.
    account: AccountRef,
    /// Newest first.
    entries: Vec<EntryOut>,
}

pub(super) async fn recent_activity(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: ActivityArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let limit = args.limit.unwrap_or(20).clamp(1, limits::MAX_PAGE);
    let fetch = if args.agents_only { 200 } else { limit };
    let entries = backend
        .activity(&account.id, u32::try_from(fetch).unwrap_or(200))
        .await?
        .into_iter()
        .filter(|e| !args.agents_only || e.record.as_ref().is_some_and(|r| r.actor.is_some()))
        .take(limit)
        .map(|e| {
            let record = e.record.as_ref();
            EntryOut {
                by: record
                    .and_then(|r| r.actor.as_ref())
                    .map_or_else(|| "a person".to_owned(), |a| a.client.clone()),
                hostnames: record.map(|r| r.hostnames.clone()).unwrap_or_default(),
                changes: record
                    .map(|r| {
                        r.changes
                            .iter()
                            .map(|d| {
                                let text = |t: Option<&teitunnel_core::text::Text>| {
                                    t.map_or_else(
                                        || "(none)".to_owned(),
                                        teitunnel_core::text::Text::english,
                                    )
                                };
                                format!(
                                    "{:?} {}{}: {} → {}",
                                    d.area,
                                    d.hostname,
                                    d.path
                                        .as_deref()
                                        .map(|p| format!(" {p}"))
                                        .unwrap_or_default(),
                                    text(d.before.as_ref()),
                                    text(d.after.as_ref())
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                error: record
                    .and_then(|r| r.error.as_ref())
                    .map(teitunnel_core::text::Text::english),
                id: e.id,
                at: e.at,
                summary: e.summary,
                outcome: e.outcome,
            }
        })
        .collect();
    Ok(ToolOutput::new(&ActivityResult {
        account: (&account).into(),
        entries,
    }))
}
