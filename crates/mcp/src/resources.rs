//! Resources: read-only views an agent (or the person, through the client) can attach
//! as context, updated when routes or shares change.

use rmcp::{
    ErrorData as McpError,
    model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate},
};
use serde_json::{Value, json};

use crate::{
    backend::{ChangeEvent, SharedBackend},
    redaction,
};

const ROUTES: &str = "teitunnel://routes";
const SHARES: &str = "teitunnel://shares";
const DOMAINS: &str = "teitunnel://domains";
const ISSUES: &str = "teitunnel://issues";
const ACTIVITY: &str = "teitunnel://activity";
const ROUTE: &str = "teitunnel://route/";
const LOGS: &str = "teitunnel://logs/";
/// Log lines a logs resource holds.
const LOG_LINES: usize = 200;

/// The fixed resources.
pub(crate) fn list() -> Vec<Resource> {
    let json = "application/json";
    vec![
        Resource::new(ROUTES, "routes")
            .with_title("Routes")
            .with_description(
                "This machine's routes in every connected account, with their status.",
            )
            .with_mime_type(json),
        Resource::new(SHARES, "shares")
            .with_title("Shares")
            .with_description("Running shares: Quick Shares and shares on your domains.")
            .with_mime_type(json),
        Resource::new(DOMAINS, "domains")
            .with_title("Domains")
            .with_description("The domains of every connected account.")
            .with_mime_type(json),
        Resource::new(ISSUES, "issues")
            .with_title("Issues")
            .with_description("Problems the Doctor finds now, with their fixes.")
            .with_mime_type(json),
        Resource::new(ACTIVITY, "activity")
            .with_title("Activity")
            .with_description(
                "The most recent changes in every connected account, and who made them.",
            )
            .with_mime_type(json),
    ]
}

/// The parameterised resources.
pub(crate) fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("teitunnel://route/{hostname}", "route")
            .with_title("A route")
            .with_description("One route by hostname: its service, status, login, tunnel and origin settings.")
            .with_mime_type("application/json"),
        ResourceTemplate::new("teitunnel://logs/{tunnel}", "logs")
            .with_title("Connector logs")
            .with_description("The newest 200 log lines of this machine's connector for a tunnel (name or id), redacted.")
            .with_mime_type("application/json"),
    ]
}

/// Whether `uri` is one of this server's resources.
pub(crate) fn is_ours(uri: &str) -> bool {
    [ROUTES, SHARES, DOMAINS, ISSUES, ACTIVITY].contains(&uri)
        || uri.strip_prefix(ROUTE).is_some_and(|h| !h.is_empty())
        || uri.strip_prefix(LOGS).is_some_and(|t| !t.is_empty())
}

/// Whether `event` changes what `uri` shows.
pub(crate) fn affected(uri: &str, event: ChangeEvent) -> bool {
    match event {
        ChangeEvent::Routes => {
            uri == ROUTES || uri == ISSUES || uri == ACTIVITY || uri.starts_with(ROUTE)
        }
        ChangeEvent::Shares => uri == SHARES || uri == ROUTES || uri.starts_with(ROUTE),
    }
}

fn contents(uri: &str, value: &Value, allow_secrets: bool) -> ReadResourceResult {
    let value = redaction::value(value.clone(), allow_secrets);
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    ReadResourceResult::new(vec![
        ResourceContents::text(crate::limits::truncate(text), uri)
            .with_mime_type("application/json"),
    ])
}

fn failed(err: impl std::fmt::Display) -> McpError {
    McpError::internal_error(err.to_string(), None)
}

async fn every_account<F, Fut>(backend: &SharedBackend, read: F) -> Result<Value, McpError>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Value, crate::backend::BackendError>>,
{
    let mut out = Vec::new();
    for account in backend.accounts().await.map_err(failed)? {
        let value = match read(account.id.clone()).await {
            Ok(value) => value,
            Err(err) => json!({ "error": err.to_string() }),
        };
        out.push(json!({ "account": { "id": account.id, "name": account.name }, "data": value }));
    }
    Ok(Value::Array(out))
}

/// Reads a resource.
pub(crate) async fn read(
    backend: &SharedBackend,
    uri: &str,
    allow_secrets: bool,
) -> Result<ReadResourceResult, McpError> {
    let value = match uri {
        ROUTES => {
            every_account(backend, |account| async move {
                let overview = backend.overview(&account).await?;
                let statuses = overview.statuses();
                let routes: Vec<Value> = overview
                    .routes
                    .iter()
                    .zip(&statuses)
                    .map(|(route, (_, health))| {
                        let mut value = redaction::english(route);
                        value["status"] = json!(health.text().english());
                        value
                    })
                    .collect();
                Ok(json!({ "tunnels": redaction::english(&overview.tunnels), "routes": routes }))
            })
            .await?
        }
        SHARES => redaction::english(&backend.shares().await.map_err(failed)?),
        DOMAINS => {
            every_account(
                backend,
                |account| async move { backend.domains(&account).await },
            )
            .await?
        }
        ISSUES => redaction::english(&backend.doctor().await.map_err(failed)?),
        ACTIVITY => {
            every_account(backend, |account| async move {
                Ok(redaction::english(&backend.activity(&account, 20).await?))
            })
            .await?
        }
        _ if uri.starts_with(ROUTE) => {
            let hostname = uri[ROUTE.len()..].trim().to_ascii_lowercase();
            let mut found = None;
            for account in backend.accounts().await.map_err(failed)? {
                let Ok(overview) = backend.overview(&account.id).await else {
                    continue;
                };
                let statuses = overview.statuses();
                if let Some((route, (_, health))) = overview
                    .routes
                    .iter()
                    .zip(&statuses)
                    .find(|(r, _)| r.hostname == hostname)
                {
                    let mut value = redaction::english(route);
                    value["status"] = json!(health.text().english());
                    value["account"] = json!({ "id": account.id, "name": account.name });
                    found = Some(value);
                    break;
                }
            }
            found.ok_or_else(|| {
                McpError::resource_not_found(format!("No route {hostname} on this machine."), None)
            })?
        }
        _ if uri.starts_with(LOGS) => {
            let wanted = uri[LOGS.len()..].trim().to_owned();
            let mut found = None;
            for account in backend.accounts().await.map_err(failed)? {
                let Ok(overview) = backend.overview(&account.id).await else {
                    continue;
                };
                if let Some(tunnel) = overview
                    .tunnels
                    .iter()
                    .find(|t| t.id == wanted || t.name.eq_ignore_ascii_case(&wanted))
                {
                    let batch = backend
                        .logs(&account.id, Some(&tunnel.id), None, LOG_LINES)
                        .await
                        .map_err(failed)?;
                    found = Some(json!({
                        "tunnel": tunnel.name,
                        "source": batch.source,
                        "note": batch.note,
                        "lines": batch.lines,
                    }));
                    break;
                }
            }
            found.ok_or_else(|| {
                McpError::resource_not_found(format!("This machine has no tunnel {wanted}."), None)
            })?
        }
        _ => {
            return Err(McpError::resource_not_found(
                format!("No resource {uri}."),
                None,
            ));
        }
    };
    Ok(contents(uri, &value, allow_secrets))
}

/// Completions for template and prompt arguments: hostnames and tunnel names.
pub(crate) async fn complete(backend: &SharedBackend, argument: &str, prefix: &str) -> Vec<String> {
    let prefix = prefix.to_ascii_lowercase();
    let mut values = Vec::new();
    let Ok(accounts) = backend.accounts().await else {
        return values;
    };
    for account in accounts {
        let Ok(overview) = backend.overview(&account.id).await else {
            continue;
        };
        match argument {
            "hostname" => values.extend(overview.routes.iter().map(|r| r.hostname.clone())),
            "tunnel" => values.extend(overview.tunnels.iter().map(|t| t.name.clone())),
            _ => {}
        }
    }
    values.retain(|v| v.to_ascii_lowercase().starts_with(&prefix));
    values.sort();
    values.dedup();
    values.truncate(100);
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knows_its_uris() {
        assert!(is_ours("teitunnel://routes"));
        assert!(is_ours("teitunnel://route/app.xyz.com"));
        assert!(is_ours("teitunnel://logs/Mac"));
        assert!(!is_ours("teitunnel://route/"));
        assert!(!is_ours("file:///etc/passwd"));
        assert!(affected("teitunnel://route/a.xyz.com", ChangeEvent::Routes));
        assert!(affected("teitunnel://shares", ChangeEvent::Shares));
        assert!(!affected("teitunnel://domains", ChangeEvent::Shares));
        assert_eq!(list().len(), 5);
        assert_eq!(templates().len(), 2);
    }
}
