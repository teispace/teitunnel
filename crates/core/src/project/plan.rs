//! Planning and applying a project file: one combined preview of everything it would
//! change (each route's plan from the engine, the shares it starts, the Snapshots it
//! publishes), then the routes applied one by one, each re-planned right before and
//! refused if it no longer matches what was shown.

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    HostHeaderDecl, Loaded, ProjectError, ProjectItem, SecretRef, SnapshotDecl, SnapshotSourceDecl,
    resolve,
    state::{self, Observed, find_route, route_change},
};
use crate::{
    Secret,
    engine::{
        AccessRule, Approval, Change, CloudApi, Connectors, Context, Engine, Outcome, PlanView,
        Progress, SiteRow, StepKind,
    },
    secrets::SecretStore,
    snapshot::{AddressInput, PasswordInput, SnapshotChange, SnapshotOptions},
    text::{Text, msg::project as m},
};

/// A route change of the project, with its reviewed plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteAction {
    /// The route's hostname.
    pub hostname: String,
    /// Its path rule.
    pub path: Option<String>,
    /// The change asked of the engine.
    pub change: Change,
    /// The tunnel (id) it's on; `None`: the default one.
    pub tunnel_id: Option<String>,
    /// The engine's plan, as shown.
    pub plan: PlanView,
}

/// A share the project starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ShareAction {
    /// The service.
    pub origin: String,
    /// On a hostname of the account's (a temporary route); `None`: a Quick Share.
    pub hostname: Option<String>,
    /// Ends by itself after this many seconds.
    pub expires_after: Option<u32>,
    /// Host header.
    pub host_header: HostHeaderDecl,
    /// Require a login.
    pub login: Option<AccessRule>,
    /// Inspect its traffic.
    pub inspect: bool,
}

/// A Snapshot the project publishes (its files are collected when applied).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotAction {
    /// Its name.
    pub name: String,
    /// The folder (absolute) its files come from, or the project to build.
    pub source: SnapshotSourceDecl,
    /// Its hostname; `None`: workers.dev.
    pub hostname: Option<String>,
    /// It exists already: a new version is published if its files or settings changed.
    pub exists: bool,
}

/// A local domain the project adds or changes on this computer (nothing in Cloudflare).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocalDomainAction {
    /// E.g. `shop.localhost`.
    pub name: String,
    /// The port it serves.
    pub port: u16,
    /// Subdomains too.
    pub wildcard: bool,
    /// It exists already (with another port or wildcard setting).
    pub exists: bool,
}

/// Everything applying the project would do, for review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProjectPlan {
    /// The project file.
    pub path: String,
    /// The project's name.
    pub name: String,
    /// The account it applies to.
    pub account_id: String,
    /// Each declared item and its state.
    pub items: Vec<ProjectItem>,
    /// Route changes, in order.
    pub routes: Vec<RouteAction>,
    /// Shares to start.
    pub shares: Vec<ShareAction>,
    /// Snapshots to publish.
    pub snapshots: Vec<SnapshotAction>,
    /// Local domains to add or change.
    pub local_domains: Vec<LocalDomainAction>,
    /// Some route change touches a DNS record Teitunnel didn't create.
    pub requires_confirmation: bool,
    /// Identifies this plan; applying checks it's still the same.
    pub fingerprint: String,
}

impl ProjectPlan {
    /// Nothing to do.
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
            && self.shares.is_empty()
            && self.snapshots.is_empty()
            && self.local_domains.is_empty()
    }
}

fn step_signature(plan: &PlanView) -> Vec<(StepKind, Text)> {
    plan.steps
        .iter()
        // An earlier route of the same apply may create the tunnel, and adds its rule to
        // the tunnel's routes (the count changes; the other rules are this project's).
        .filter(|s| s.kind != StepKind::CreateTunnel)
        .map(|s| {
            let mut description = s.description.clone();
            if s.kind == StepKind::PutConfig {
                description.args.remove("count");
            }
            (s.kind, description)
        })
        .collect()
}

/// Plans a project for one account: the routes' changes (each previewed by the engine),
/// the shares not running yet (`quick_origins`: services Quick Shares already share),
/// and its Snapshots.
///
/// # Errors
/// An invalid file, a placeholder without a value, an unknown tunnel, or the engine
/// refusing a route (e.g. a hostname outside the account's domains).
pub async fn plan<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    loaded: &Loaded,
    quick_origins: &[String],
) -> Result<ProjectPlan, ProjectError> {
    let file = loaded.file()?;
    let resolved = resolve(file, &loaded.vars)?;
    let base = Context {
        tunnel: None,
        ..ctx
    };
    let overview = engine.overview(api, connectors, base).await?;
    let local = engine.local();
    let domain_shares = local.shares(Some(ctx.account)).await?;
    let snapshots: Vec<SiteRow> = local.sites(Some(ctx.account)).await?;
    let tunnels = local.tunnels(ctx.account).await?;
    let local_rows = crate::local_domains::registry::list(local.store()).await?;

    let items = state::status(
        &resolved,
        Observed {
            routes: &overview.routes,
            domain_shares: &domain_shares,
            quick_origins,
            snapshots: &snapshots,
            local_domains: &local_rows,
        },
    );
    let local_domains = resolved
        .local_domains
        .iter()
        .filter_map(|decl| {
            let state = state::local_domain_state(decl, &local_rows);
            (state != state::ItemState::Applied).then(|| LocalDomainAction {
                name: decl.name.clone(),
                port: decl.port,
                wildcard: decl.wildcard,
                exists: state == state::ItemState::Differs,
            })
        })
        .collect();

    let mut routes = Vec::new();
    for (decl, input) in &resolved.routes {
        let existing = find_route(&overview.routes, input);
        let Some(change) = route_change(input, existing) else {
            continue;
        };
        let tunnel_id = match (&decl.tunnel, existing) {
            // An existing route stays on its tunnel.
            (_, Some(route)) => route.tunnel_id.clone(),
            (Some(name), None) => Some(
                tunnels
                    .iter()
                    .find(|t| t.name.eq_ignore_ascii_case(name) || t.tunnel_id == *name)
                    .map(|t| t.tunnel_id.clone())
                    .ok_or_else(|| ProjectError::Unresolved(m::no_tunnel(name)))?,
            ),
            (None, None) => None,
        };
        let route_ctx = Context {
            tunnel: tunnel_id.as_deref(),
            ..ctx
        };
        let intent = engine.intent_for(api, route_ctx, &change).await?;
        let preview = engine.preview(api, route_ctx, &intent).await?;
        if preview.is_empty() {
            continue;
        }
        routes.push(RouteAction {
            hostname: input.hostname.clone(),
            path: input.path.clone(),
            change,
            tunnel_id,
            plan: preview.view(ctx.account),
        });
    }

    let shares: Vec<ShareAction> = resolved
        .shares
        .iter()
        .zip(items.iter().filter(|i| i.kind == state::ItemKind::Share))
        .filter(|(_, item)| item.state != state::ItemState::Applied)
        .map(|(share, _)| ShareAction {
            origin: share.decl.origin.clone(),
            hostname: share.hostname.clone(),
            expires_after: share.decl.expires_after,
            host_header: share.decl.host_header.clone(),
            login: share.decl.login.clone(),
            inspect: share.decl.inspect,
        })
        .collect();

    let snapshot_actions = resolved
        .snapshots
        .iter()
        .map(|(decl, hostname)| SnapshotAction {
            name: decl.name.clone(),
            source: match &decl.source {
                SnapshotSourceDecl::Folder(p) => {
                    SnapshotSourceDecl::Folder(loaded.dir.join(p).display().to_string())
                }
                SnapshotSourceDecl::Build(p) => {
                    SnapshotSourceDecl::Build(loaded.dir.join(p).display().to_string())
                }
            },
            hostname: hostname.clone(),
            exists: snapshots
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&decl.name)),
        })
        .collect();

    let mut plan = ProjectPlan {
        path: loaded.path.display().to_string(),
        name: loaded.name.clone(),
        account_id: ctx.account.to_owned(),
        requires_confirmation: routes.iter().any(|r| r.plan.requires_confirmation),
        items,
        routes,
        shares,
        snapshots: snapshot_actions,
        local_domains,
        fingerprint: String::new(),
    };
    plan.fingerprint = fingerprint(&plan);
    Ok(plan)
}

fn fingerprint(plan: &ProjectPlan) -> String {
    let mut hash = Sha256::new();
    for route in &plan.routes {
        hash.update(route.plan.fingerprint.as_bytes());
        hash.update([0]);
    }
    hash.update(
        serde_json::to_vec(&(&plan.shares, &plan.snapshots, &plan.local_domains))
            .unwrap_or_default(),
    );
    hash.finalize()[..12]
        .iter()
        .fold(String::with_capacity(24), |mut out, b| {
            use std::fmt::Write;
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Applies one reviewed route change: planned again now, refused unless its steps are
/// the ones shown (an earlier route of the same apply may have created the tunnel).
///
/// # Errors
/// [`ProjectError::Changed`] when the plan differs from the one shown; engine errors.
pub async fn apply_route<C, K, P>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    action: &RouteAction,
    confirmed: bool,
    progress: P,
) -> Result<Outcome, ProjectError>
where
    C: CloudApi,
    K: Connectors,
    P: FnMut(Progress) + Send,
{
    let ctx = Context {
        tunnel: action.tunnel_id.as_deref(),
        ..ctx
    };
    engine.invalidate(ctx.account);
    let intent = engine.intent_for(api, ctx, &action.change).await?;
    let fresh = engine.preview(api, ctx, &intent).await?;
    let view = fresh.view(ctx.account);
    if view.steps.is_empty() {
        // Already done (e.g. by an earlier step of this apply).
        return Ok(Outcome::Applied {
            tunnel_id: action.tunnel_id.clone(),
            verify: Vec::new(),
            connector_error: None,
        });
    }
    if step_signature(&view) != step_signature(&action.plan)
        || (view.requires_confirmation && !action.plan.requires_confirmation)
    {
        return Err(ProjectError::Changed);
    }
    let approval = Approval {
        fingerprint: &fresh.fingerprint,
        confirmed: confirmed && action.plan.requires_confirmation,
    };
    Ok(engine
        .apply(api, connectors, ctx, &intent, approval, progress)
        .await?)
}

/// Reads a secret the file refers to.
///
/// # Errors
/// [`ProjectError::Secret`] when the variable or keychain entry isn't there.
pub fn resolve_secret(
    reference: &SecretRef,
    keychain: &dyn SecretStore,
) -> Result<Secret<String>, ProjectError> {
    match reference {
        SecretRef::Env(name) => std::env::var(name)
            .ok()
            .filter(|v| !v.is_empty())
            .map(Secret::new)
            .ok_or_else(|| ProjectError::Secret(m::missing_env(name))),
        SecretRef::Keychain(name) => keychain
            .get(name)
            .ok()
            .flatten()
            .ok_or_else(|| ProjectError::Secret(m::missing_keychain(name))),
    }
}

/// The Snapshot change for a declared Snapshot whose files were prepared (`prepared`):
/// published when new, a new version otherwise. A password is only set on the first
/// publish (a Snapshot can't tell whether it changed); later applies keep it, and
/// removing `password` from the file removes it.
pub fn snapshot_change(
    decl: &SnapshotDecl,
    hostname: Option<&str>,
    prepared: String,
    existing: Option<&SiteRow>,
    password: Option<Secret<String>>,
) -> SnapshotChange {
    let options = |password: PasswordInput| SnapshotOptions {
        spa: decl.spa,
        password,
        access: decl.login.clone(),
        expires_in_days: decl.expires_in_days,
        comments: None,
    };
    match existing {
        None => SnapshotChange::Publish {
            prepared,
            name: decl.name.clone(),
            address: hostname.map_or(AddressInput::WorkersDev, |h| AddressInput::Domain {
                hostname: h.to_owned(),
            }),
            options: options(match password {
                Some(p) => PasswordInput::Set {
                    password: p.expose().clone(),
                },
                None => PasswordInput::Keep,
            }),
        },
        Some(row) => SnapshotChange::Update {
            snapshot: row.id.clone(),
            prepared: Some(prepared),
            options: options(match (decl.password.is_some(), row.password) {
                (true, false) => password.map_or(PasswordInput::Keep, |p| PasswordInput::Set {
                    password: p.expose().clone(),
                }),
                (false, true) => PasswordInput::Remove,
                (true, true) | (false, false) => PasswordInput::Keep,
            }),
        },
    }
}
