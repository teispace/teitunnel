//! What a project file asks for on this machine (placeholders filled in), and how that
//! compares with what's there: each declared item is applied, differs or missing.

use localdomains::DomainTarget;
use serde::Serialize;

use super::{
    LocalDomainDecl, ProjectError, ProjectFile, RouteDecl, ShareDecl, SnapshotDecl,
    schema::template_error, template,
};
use crate::{
    domain::{Hostname, RouteOrigin},
    domain_shares::DomainShare,
    engine::{Change, DnsState, RouteInput, RouteView, SiteRow},
    local_domains::LocalDomainRow,
    text::{Text, UserText, msg::project as m},
};

/// A share with its hostname filled in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedShare {
    /// As declared.
    pub decl: ShareDecl,
    /// The hostname on this machine (placeholders filled in).
    pub hostname: Option<String>,
}

/// Everything the file declares, as it applies on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// Routes, as the engine takes them.
    pub routes: Vec<(RouteDecl, RouteInput)>,
    /// Shares.
    pub shares: Vec<ResolvedShare>,
    /// Snapshots with their hostname filled in.
    pub snapshots: Vec<(SnapshotDecl, Option<String>)>,
    /// Local domains.
    pub local_domains: Vec<LocalDomainDecl>,
}

fn fill(template: &str, vars: &template::Vars) -> Result<String, ProjectError> {
    let expanded = template::expand(template, vars)
        .map_err(|e| ProjectError::Unresolved(template_error(&e)))?;
    Hostname::parse(&expanded)
        .map(|h| h.to_string())
        .map_err(|e| ProjectError::Unresolved(e.text()))
}

/// Fills in placeholders.
///
/// # Errors
/// [`ProjectError::Unresolved`]: a placeholder has no value here (`{branch}` outside a
/// repository), or a hostname isn't valid once filled in.
pub fn resolve(file: &ProjectFile, vars: &template::Vars) -> Result<Resolved, ProjectError> {
    let routes = file
        .routes
        .iter()
        .map(|decl| {
            let hostname = fill(&decl.hostname, vars)?;
            Ok((
                decl.clone(),
                RouteInput {
                    hostname,
                    path: decl.path.clone(),
                    origin: decl.origin.clone(),
                    access: decl.login.clone(),
                    options: decl
                        .origin_request
                        .clone()
                        .filter(|o| !o.is_default())
                        .map(Box::new),
                },
            ))
        })
        .collect::<Result<_, ProjectError>>()?;
    let shares = file
        .shares
        .iter()
        .map(|decl| {
            Ok(ResolvedShare {
                decl: decl.clone(),
                hostname: decl
                    .hostname
                    .as_deref()
                    .map(|h| fill(h, vars))
                    .transpose()?,
            })
        })
        .collect::<Result<_, ProjectError>>()?;
    let snapshots = file
        .snapshots
        .iter()
        .map(|decl| {
            Ok((
                decl.clone(),
                decl.hostname
                    .as_deref()
                    .map(|h| fill(h, vars))
                    .transpose()?,
            ))
        })
        .collect::<Result<_, ProjectError>>()?;
    Ok(Resolved {
        routes,
        shares,
        snapshots,
        local_domains: file.local_domains.clone(),
    })
}

/// What kind of item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ItemKind {
    /// A route.
    Route,
    /// A share.
    Share,
    /// A Snapshot.
    Snapshot,
    /// A local domain.
    LocalDomain,
}

/// How a declared item compares with this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ItemState {
    /// As declared.
    Applied,
    /// There, but not as declared.
    Differs,
    /// Not there.
    Missing,
    /// This version can't apply it (see the note).
    Unsupported,
}

/// A declared item and its state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProjectItem {
    /// Route, share, Snapshot or local domain.
    pub kind: ItemKind,
    /// Its address or name.
    pub name: String,
    /// Where it goes (the service, the source).
    pub target: String,
    /// Applied, differs, missing.
    pub state: ItemState,
    /// The line it's declared on.
    pub line: u32,
    /// More to know.
    pub note: Option<Text>,
}

/// What's on this machine and account, for comparing.
#[derive(Debug, Clone, Copy)]
pub struct Observed<'a> {
    /// The account's routes on this machine.
    pub routes: &'a [RouteView],
    /// Shares on the account's domains.
    pub domain_shares: &'a [DomainShare],
    /// Services shared by running Quick Shares.
    pub quick_origins: &'a [String],
    /// The account's Snapshots.
    pub snapshots: &'a [SiteRow],
    /// This computer's local domains.
    pub local_domains: &'a [LocalDomainRow],
}

/// How a declared local domain compares with this computer's.
pub(crate) fn local_domain_state(decl: &LocalDomainDecl, rows: &[LocalDomainRow]) -> ItemState {
    match rows.iter().find(|r| r.name.as_str() == decl.name) {
        None => ItemState::Missing,
        Some(row)
            if row.target == (DomainTarget::Port { port: decl.port })
                && row.wildcard == decl.wildcard =>
        {
            ItemState::Applied
        }
        Some(_) => ItemState::Differs,
    }
}

fn same_origin(a: &str, b: &str) -> bool {
    match (RouteOrigin::parse(a), RouteOrigin::parse(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a.trim() == b.trim(),
    }
}

fn normalized(rule: Option<&crate::engine::AccessRule>) -> Option<crate::engine::AccessRule> {
    rule.and_then(|r| r.normalized().ok())
}

/// The change that makes the route as declared, if one is needed.
pub(crate) fn route_change(input: &RouteInput, existing: Option<&RouteView>) -> Option<Change> {
    let Some(route) = existing else {
        return Some(Change::AddRoute {
            route: input.clone(),
        });
    };
    let options = input.options.as_deref().cloned().unwrap_or_default();
    let config_same = same_origin(&route.origin, &input.origin) && route.options == options;
    let login_same = normalized(route.access.as_ref()) == normalized(input.access.as_ref());
    match (config_same, login_same, route.dns == DnsState::Ok) {
        (true, true, true) => None,
        // Adding an identical route only fills in what's missing (DNS, the login).
        (true, _, _) if input.access.is_some() || route.access.is_none() => {
            Some(Change::AddRoute {
                route: input.clone(),
            })
        }
        _ => Some(Change::UpdateRoute {
            hostname: route.hostname.clone(),
            path: route.path.clone(),
            route: RouteInput {
                // An edit with no options keeps the route's; declared defaults must win.
                options: Some(Box::new(options)),
                ..input.clone()
            },
        }),
    }
}

pub(crate) fn find_route<'a>(routes: &'a [RouteView], input: &RouteInput) -> Option<&'a RouteView> {
    let path = input
        .path
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty());
    routes
        .iter()
        .find(|r| r.hostname.eq_ignore_ascii_case(&input.hostname) && r.path.as_deref() == path)
}

fn state_of(change: Option<&Change>) -> ItemState {
    match change {
        None => ItemState::Applied,
        Some(Change::AddRoute { .. }) => ItemState::Missing,
        Some(_) => ItemState::Differs,
    }
}

/// Each declared item and how it compares with what's observed.
pub fn status(resolved: &Resolved, observed: Observed<'_>) -> Vec<ProjectItem> {
    let mut items = Vec::new();
    for (decl, input) in &resolved.routes {
        let existing = find_route(observed.routes, input);
        let change = route_change(input, existing);
        let mut state = state_of(change.as_ref());
        if existing.is_some() && state == ItemState::Missing {
            // There, only its DNS record or login is missing.
            state = ItemState::Differs;
        }
        items.push(ProjectItem {
            kind: ItemKind::Route,
            name: match &input.path {
                Some(path) => format!("{} {path}", input.hostname),
                None => input.hostname.clone(),
            },
            target: input.origin.clone(),
            state,
            line: decl.line,
            note: existing
                .filter(|r| r.temporary)
                .map(|_| m::note_temporary()),
        });
    }
    for share in &resolved.shares {
        let (state, name) = match &share.hostname {
            Some(hostname) => {
                let running = observed
                    .domain_shares
                    .iter()
                    .find(|s| s.hostname.eq_ignore_ascii_case(hostname));
                let state = match running {
                    Some(s) if same_origin(&s.origin, &share.decl.origin) => ItemState::Applied,
                    Some(_) => ItemState::Differs,
                    None => ItemState::Missing,
                };
                (state, hostname.clone())
            }
            None => {
                let running = observed
                    .quick_origins
                    .iter()
                    .any(|o| same_origin(o, &share.decl.origin));
                let state = if running {
                    ItemState::Applied
                } else {
                    ItemState::Missing
                };
                (state, "trycloudflare.com".to_owned())
            }
        };
        items.push(ProjectItem {
            kind: ItemKind::Share,
            name,
            target: share.decl.origin.clone(),
            state,
            line: share.decl.line,
            note: None,
        });
    }
    for (decl, hostname) in &resolved.snapshots {
        let existing = observed
            .snapshots
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&decl.name));
        let state = match existing {
            None => ItemState::Missing,
            Some(row) => {
                // No hostname declared: the account's workers.dev address.
                let at = row.hostname.as_deref();
                let same = match hostname {
                    Some(h) => at == Some(h.as_str()),
                    None => at.is_none_or(|a| a.ends_with(".workers.dev")),
                };
                if same {
                    ItemState::Applied
                } else {
                    ItemState::Differs
                }
            }
        };
        items.push(ProjectItem {
            kind: ItemKind::Snapshot,
            name: decl.name.clone(),
            target: match &decl.source {
                super::SnapshotSourceDecl::Folder(path)
                | super::SnapshotSourceDecl::Build(path) => path.clone(),
            },
            state,
            line: decl.line,
            note: (state == ItemState::Differs).then(m::note_snapshot_address),
        });
    }
    for domain in &resolved.local_domains {
        items.push(ProjectItem {
            kind: ItemKind::LocalDomain,
            name: if domain.wildcard {
                format!("{} (*.{})", domain.name, domain.name)
            } else {
                domain.name.clone()
            },
            target: format!("localhost:{}", domain.port),
            state: local_domain_state(domain, observed.local_domains),
            line: domain.line,
            note: None,
        });
    }
    items
}
