//! Doctor: checks that detect problems and propose fixes. A fix is either an engine
//! change (previewed and applied like any other, so it's rollback-safe) or a simple
//! local action. Gathering reads Cloudflare; diagnosing is a pure function of the
//! gathered facts, so every check is unit-tested.

use std::{
    collections::{HashMap, HashSet},
    net::{Ipv4Addr, SocketAddr},
    time::Duration,
};

use cf_api::DnsRecord;
use futures_util::{StreamExt, stream};
use ipnet::IpNet;
use serde::Serialize;
use tokio::net::TcpStream;

use crate::{
    accounts::{Domain, DomainStatus},
    domain::{Hostname, PathRule, RouteOrigin},
    engine::{
        Change, CloudApi, Connectors, Context, Drift, Engine, EngineError, ObserveError,
        ObserveNeed, RouteInput, Snapshot, TunnelSummary, Want, access_domain, observe,
        tunnel_target,
    },
    runtime::ConnectorState,
    text::{Text, UserText, msg, msg::doctor as m},
};

/// How bad an issue is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// Something doesn't work.
    Error,
    /// Something may stop working, or needs attention.
    Warning,
    /// Worth knowing; nothing is broken.
    Info,
}

/// A way to fix an issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Fix {
    /// A change in Cloudflare, previewed as a plan before it's applied.
    Change {
        /// Button title, e.g. "Fix the DNS Record".
        label: Text,
        /// The change.
        change: Change,
    },
    /// Install (or update) the managed cloudflared.
    InstallBinary,
    /// Start this Mac's connector.
    StartConnector {
        /// Account.
        account_id: String,
    },
    /// Accept an outside edit of this Mac's routes.
    KeepTheirs {
        /// Account.
        account_id: String,
    },
    /// Create a token with the right permissions.
    Reconnect,
    /// Remove a tunnel's stale connections.
    CleanConnections {
        /// Account.
        account_id: String,
        /// Tunnel.
        tunnel_id: String,
    },
    /// Local domains, fixed on this computer.
    LocalDomains {
        /// What to do.
        action: crate::local_domains::LocalDomainFix,
    },
}

/// A detected problem.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// Stable id (check, account, subject), so "Ignore" survives restarts.
    pub id: String,
    /// Which check found it, e.g. `dns.missing`.
    pub check: String,
    /// How bad it is.
    pub severity: Severity,
    /// The account it's in, if any.
    pub account_id: Option<String>,
    /// What it's about, e.g. a hostname or a tunnel name (stable: part of `id`).
    pub subject: String,
    /// `subject` as shown to the user.
    pub label: Text,
    /// One line.
    pub title: Text,
    /// What it means and what to do.
    pub detail: Text,
    /// Supporting facts (records, states).
    pub evidence: Vec<Text>,
    /// Fixes, the recommended one first.
    pub fixes: Vec<Fix>,
    /// The tunnel of this Mac's it's about, when not the default one (a fix applies there).
    pub tunnel_id: Option<String>,
}

/// The cloudflared binary, as the Doctor sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryFact {
    /// Installed and new enough.
    Ok,
    /// Not installed.
    Missing,
    /// Installed but older than Teitunnel needs.
    Unsupported {
        /// Its version, if known.
        version: Option<String>,
    },
}

/// Everything the checks look at for one account.
#[derive(Debug, Clone)]
pub struct AccountFacts {
    /// Account id.
    pub account_id: String,
    /// Which of this Mac's tunnels these facts are about (`None`: the default one).
    pub tunnel: Option<String>,
    /// This Mac's tunnel and every routed hostname's records.
    pub snapshot: Snapshot,
    /// All tunnels in the account.
    pub tunnels: Vec<TunnelSummary>,
    /// An outside edit of this Mac's routes.
    pub drift: Option<Drift>,
    /// Domains.
    pub domains: Vec<Domain>,
    /// CNAMEs pointing at any tunnel (`*.cfargotunnel.com`), by zone id.
    pub tunnel_cnames: Vec<(String, DnsRecord)>,
    /// Ids of records Teitunnel created.
    pub owned: HashSet<String>,
    /// Whether the credential can manage routes (None: unknown).
    pub can_manage_routes: Option<bool>,
    /// This Mac's connector.
    pub connector: Option<ConnectorState>,
    /// Local origin ports and whether something listens on them.
    pub listening: HashMap<u16, bool>,
    /// This Mac's connector's recent log lines.
    pub logs: Vec<String>,
    /// Logins Teitunnel added that still exist but whose route is gone (Access domains).
    pub orphan_logins: Vec<String>,
    /// Routes left pointing at an inspector that nobody runs (`inspect.orphan`).
    pub lens_orphans: Vec<crate::inspect::routes::InspectedRoute>,
    /// How WARP clients are set up; read only when this Mac shares private networks.
    pub warp: WarpFacts,
}

/// The account's WARP client settings that decide whether clients reach a private
/// network. Each is `None` when it couldn't be read (the token needs Zero Trust read).
#[derive(Debug, Clone, Default)]
pub struct WarpFacts {
    /// Gateway proxy settings.
    pub settings: Option<cf_api::DeviceSettings>,
    /// The default device profile's Split Tunnels.
    pub profile: Option<cf_api::DefaultDeviceProfile>,
}

/// Everything the checks look at.
#[derive(Debug, Clone)]
pub struct Facts {
    /// The binary.
    pub binary: BinaryFact,
    /// Each connected account.
    pub accounts: Vec<AccountFacts>,
    /// cloudflared processes Teitunnel didn't start.
    pub foreign: Vec<crate::discovery::cloudflared::ForeignConnector>,
}

const TUNNEL_SUFFIX: &str = ".cfargotunnel.com";

async fn is_listening(port: u16) -> bool {
    let addrs = [
        SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
    ];
    for addr in addrs {
        if matches!(
            tokio::time::timeout(Duration::from_millis(300), TcpStream::connect(addr)).await,
            Ok(Ok(_))
        ) {
            return true;
        }
    }
    false
}

/// Reads what the checks need for one account (bounded concurrency, read-only).
///
/// # Errors
/// API or database errors.
pub async fn gather<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    domains: Vec<Domain>,
    can_manage_routes: Option<bool>,
) -> Result<AccountFacts, EngineError> {
    let snapshot = observe(
        api,
        engine.local(),
        ctx.account,
        ctx.tunnel,
        ctx.machine_name,
        None,
        &ObserveNeed {
            networks: Want::IfAllowed,
            ..ObserveNeed::none()
        },
        engine.who(),
    )
    .await?;
    let tunnels = engine.tunnels(api, connectors, ctx.account).await?;
    let drift = engine.drift(api, ctx.account, ctx.tunnel).await?;
    let owned = engine
        .local()
        .owned_records(ctx.account)
        .await
        .map_err(ObserveError::from)?;
    let tunnel_cnames: Vec<(String, DnsRecord)> = stream::iter(snapshot.zones.clone())
        .map(|zone| async move {
            let records = api.cname_records(&zone.id).await.unwrap_or_default();
            records
                .into_iter()
                .filter(|r| r.content.to_ascii_lowercase().ends_with(TUNNEL_SUFFIX))
                .map(|r| (zone.id.clone(), r))
                .collect::<Vec<_>>()
        })
        .buffered(4)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .flatten()
        .collect();
    let mut listening = HashMap::new();
    for rule in snapshot.routes() {
        if let Ok(origin) = RouteOrigin::parse(&rule.service)
            && origin.is_local()
            && let Some(port) = origin.port()
            && !listening.contains_key(&port)
        {
            listening.insert(port, is_listening(port).await);
        }
    }
    let connector = snapshot
        .tunnel
        .as_ref()
        .and_then(|t| connectors.state(&t.id));
    let logs = snapshot
        .tunnel
        .as_ref()
        .map(|t| connectors.recent_logs(&t.id, 500))
        .unwrap_or_default();
    let orphan_logins = orphan_logins(engine, api, ctx.account, &snapshot).await;
    let lens_orphans = {
        let remembered = crate::inspect::routes::list(engine.local().store(), Some(ctx.account))
            .await
            .unwrap_or_default();
        let rules: Vec<(String, Option<String>, String)> = snapshot
            .routes()
            .into_iter()
            .filter_map(|r| Some((r.hostname.clone()?, r.path.clone(), r.service.clone())))
            .collect();
        crate::inspect::routes::orphans(&remembered, &rules, &listening)
    };
    let warp = if shared_networks(&snapshot).is_empty() {
        WarpFacts::default()
    } else {
        let (settings, profile) = tokio::join!(
            api.device_settings(ctx.account),
            api.default_device_profile(ctx.account)
        );
        WarpFacts {
            settings: settings.ok(),
            profile: profile.ok(),
        }
    };
    Ok(AccountFacts {
        account_id: ctx.account.to_owned(),
        tunnel: ctx.tunnel.map(str::to_owned),
        snapshot,
        tunnels,
        drift,
        domains,
        tunnel_cnames,
        owned,
        can_manage_routes,
        connector,
        listening,
        logs,
        orphan_logins,
        lens_orphans,
        warp,
    })
}

/// The ranges routed to this Mac's tunnel in the default virtual network.
fn shared_networks(snapshot: &Snapshot) -> Vec<IpNet> {
    let (Some(tunnel), Some(networks)) = (&snapshot.tunnel, &snapshot.networks) else {
        return Vec::new();
    };
    let mut ranges: Vec<IpNet> = networks
        .in_default_vnet()
        .filter(|r| r.tunnel_id == tunnel.id)
        .filter_map(crate::engine::ObservedNetworkRoute::range)
        .map(|r| r.net())
        .collect();
    ranges.sort_unstable();
    ranges.dedup();
    ranges
}

/// A Split Tunnels address entry: a CIDR range or a bare address.
fn split_entry(entry: &cf_api::SplitTunnelEntry) -> Option<IpNet> {
    let address = entry.address.as_deref()?.trim();
    address
        .parse::<IpNet>()
        .ok()
        .or_else(|| address.parse::<std::net::IpAddr>().ok().map(IpNet::from))
}

/// Why WARP clients won't send `range` to the tunnel, per the default profile's Split
/// Tunnels: `Some((check, entries))`, or `None` when they will (or it's unknown).
fn split_tunnel_problem(
    profile: &cf_api::DefaultDeviceProfile,
    range: &IpNet,
) -> Option<(&'static str, Vec<Text>)> {
    if let Some(include) = &profile.include {
        let covered = include
            .iter()
            .filter_map(split_entry)
            .any(|entry| entry.contains(range));
        return (!covered).then(|| ("network.not_included", Vec::new()));
    }
    let excluded: Vec<Text> = profile
        .exclude
        .iter()
        .flatten()
        .filter_map(|e| split_entry(e).map(|net| (net, e)))
        .filter(|(net, _)| net.contains(range) || range.contains(net))
        .map(
            |(net, e)| match e.description.as_deref().filter(|d| !d.is_empty()) {
                Some(description) => m::network_excluded::excluded_named(net, description),
                None => m::network_excluded::excluded(net),
            },
        )
        .collect();
    (!excluded.is_empty()).then_some(("network.excluded", excluded))
}

/// Logins Teitunnel added for routes that no longer exist, confirmed to still be in
/// Cloudflare. Best effort: a token that can't read Access finds none.
async fn orphan_logins<C: CloudApi>(
    engine: &Engine,
    api: &C,
    account: &str,
    snapshot: &Snapshot,
) -> Vec<String> {
    let Ok(owned) = engine.local().owned_access_apps(account).await else {
        return Vec::new();
    };
    // Routes on this Mac's other tunnels keep their logins too.
    let routed: HashSet<String> = snapshot
        .routes()
        .into_iter()
        .map(|rule| (rule.hostname.as_deref(), rule.path.as_deref()))
        .chain(
            snapshot
                .elsewhere
                .iter()
                .map(|r| (Some(r.hostname.as_str()), r.path.as_deref())),
        )
        .filter_map(|(hostname, path)| {
            let host = Hostname::parse(hostname?).ok()?;
            let path = path.and_then(|p| PathRule::parse(p).ok());
            access_domain(&host, path.as_ref()).ok()
        })
        .map(|d| d.to_ascii_lowercase())
        .collect();
    let candidates: Vec<(String, String)> = owned
        .into_iter()
        .filter(|(_, domain)| !routed.contains(&domain.to_ascii_lowercase()))
        .collect();
    let mut found: Vec<String> = stream::iter(candidates)
        .map(|(id, domain)| async move {
            let apps = api.access_apps_for(account, &domain).await.ok()?;
            apps.iter().any(|a| a.id == id).then_some(domain)
        })
        .buffered(4)
        .filter_map(std::future::ready)
        .collect()
        .await;
    found.sort_unstable();
    found.dedup();
    found
}

/// Checks everything: the binary and every connected account. An account that can't
/// be read becomes an issue itself rather than failing the run.
pub async fn run<K: Connectors>(
    accounts: &crate::accounts::Accounts,
    engine: &Engine,
    machine: &K,
    binary: &crate::binary::BinaryManager,
    machine_name: &str,
) -> Vec<Issue> {
    let binary = match binary.refresh().await {
        Ok(status) if status.is_supported() => BinaryFact::Ok,
        Ok(status) => BinaryFact::Unsupported {
            version: status.version.map(|v| v.to_string()),
        },
        Err(_) => BinaryFact::Missing,
    };
    let mut facts = Facts {
        binary,
        accounts: Vec::new(),
        foreign: crate::discovery::cloudflared::foreign().await,
    };
    let mut unreachable = Vec::new();
    for account in accounts.list().await.unwrap_or_default() {
        // The default tunnel first, then this Mac's others: each is checked on its own.
        let others: Vec<String> = engine
            .local()
            .tunnels(&account.id)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|t| !t.is_default)
            .map(|t| t.tunnel_id)
            .collect();
        let gathered = async {
            let api = accounts.client(&account.id).await.map_err(|e| e.text())?;
            let domains = accounts.domains(&account.id).await.map_err(|e| e.text())?;
            let can_manage = accounts
                .capabilities(&account.id)
                .await
                .ok()
                .map(|c| c.can_manage_routes());
            let mut gathered = Vec::new();
            for tunnel in std::iter::once(None).chain(others.iter().map(|id| Some(id.as_str()))) {
                let ctx = Context {
                    account: &account.id,
                    machine_name,
                    tunnel,
                };
                gathered.push(
                    gather(engine, &api, machine, ctx, domains.clone(), can_manage)
                        .await
                        .map_err(|e| e.text())?,
                );
            }
            Ok::<_, Text>(gathered)
        }
        .await;
        match gathered {
            Ok(account_facts) => facts.accounts.extend(account_facts),
            Err(message) => unreachable.push(Issue {
                id: format!("account.unreachable:{}:{}", account.id, account.name),
                check: "account.unreachable".into(),
                severity: Severity::Error,
                account_id: Some(account.id.clone()),
                subject: account.name.clone(),
                label: msg::raw(&account.name),
                title: m::unreadable::title(&account.name),
                detail: message,
                evidence: Vec::new(),
                fixes: vec![Fix::Reconnect],
                tunnel_id: None,
            }),
        }
    }
    let mut issues = diagnose(&facts);
    // Account-wide findings repeat for each tunnel checked; keep the first.
    let mut seen = HashSet::new();
    issues.retain(|issue| seen.insert(issue.id.clone()));
    issues.extend(unreachable);
    issues.sort_by(|a, b| {
        (a.severity, &a.subject, &a.check).cmp(&(b.severity, &b.subject, &b.check))
    });
    issues
}

/// Runs the checks, then applies every safe fix in every account.
pub async fn fix_all_safe<K: Connectors>(
    accounts: &crate::accounts::Accounts,
    engine: &Engine,
    machine: &K,
    binary: &crate::binary::BinaryManager,
    machine_name: &str,
) -> FixReport {
    let issues = run(accounts, engine, machine, binary, machine_name).await;
    let mut report = FixReport::default();
    for account in accounts.list().await.unwrap_or_default() {
        let Ok(api) = accounts.client(&account.id).await else {
            continue;
        };
        let ctx = Context {
            account: &account.id,
            machine_name,
            tunnel: None,
        };
        let part = fix_safe(engine, &api, machine, ctx, &issues).await;
        report.fixed += part.fixed;
        report.skipped += part.skipped;
        report.failed.extend(part.failed);
    }
    report
}

/// What "Fix all safe issues" did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FixReport {
    /// Issues fixed.
    pub fixed: u32,
    /// Issues left for the user (their fix needs a confirmation or a decision).
    pub skipped: u32,
    /// Fixes that failed (and were rolled back), with why.
    pub failed: Vec<String>,
}

/// Whether an issue's fix may run without review: DNS repairs and orphan deletions
/// (records and logins) only.
/// Whether it really is safe is decided by its plan (no confirmation needed = nothing
/// Teitunnel doesn't own is touched).
/// The change "Fix Safe Issues" may apply for `issue` without review, if any (its plan
/// still has to need no confirmation).
pub fn safe_change(issue: &Issue) -> Option<&Change> {
    match issue.fixes.first()? {
        Fix::Change { change, .. }
            if matches!(
                change,
                Change::AddRoute { .. } | Change::DeleteRecord { .. } | Change::RemoveLogin { .. }
            ) =>
        {
            Some(change)
        }
        _ => None,
    }
}

/// Applies every safe fix among `issues` for one account, each through a fresh plan.
pub async fn fix_safe<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    issues: &[Issue],
) -> FixReport {
    let mut report = FixReport::default();
    for issue in issues
        .iter()
        .filter(|i| i.account_id.as_deref() == Some(ctx.account))
    {
        let Some(change) = safe_change(issue) else {
            report.skipped += 1;
            continue;
        };
        // The fix applies to the tunnel the issue is about.
        let ctx = Context {
            tunnel: issue.tunnel_id.as_deref().or(ctx.tunnel),
            ..ctx
        };
        let planned = async {
            let intent = engine.intent_for(api, ctx, change).await?;
            let plan = engine.preview(api, ctx, &intent).await?;
            Ok::<_, EngineError>((intent, plan))
        }
        .await;
        let (intent, plan) = match planned {
            Ok((intent, plan)) if !plan.requires_confirmation && !plan.is_empty() => (intent, plan),
            Ok(_) => {
                report.skipped += 1;
                continue;
            }
            Err(err) => {
                report.failed.push(format!("{}: {err}", issue.subject));
                continue;
            }
        };
        let approval = crate::engine::Approval {
            fingerprint: &plan.fingerprint,
            confirmed: false,
        };
        match engine
            .apply(api, connectors, ctx, &intent, approval, |_| {})
            .await
        {
            Ok(crate::engine::Outcome::Applied { .. }) => report.fixed += 1,
            Ok(
                crate::engine::Outcome::RolledBack { error, .. }
                | crate::engine::Outcome::PartiallyApplied { error, .. },
            ) => {
                report.failed.push(format!("{}: {error}", issue.subject));
            }
            // Something changed or now needs a yes: leave it for the user.
            Err(_) => report.skipped += 1,
        }
    }
    report
}

struct Found<'a> {
    account: Option<&'a str>,
    tunnel: Option<&'a str>,
    issues: Vec<Issue>,
}

/// What an issue is about: a stable id part, and how it's shown.
struct Subject {
    id: String,
    label: Text,
}

impl Subject {
    /// A subject named in words (translated), with a fixed id.
    fn named(id: &str, label: Text) -> Self {
        Self {
            id: id.to_owned(),
            label,
        }
    }
}

/// A hostname, tunnel name or range: shown as it is.
impl From<&str> for Subject {
    fn from(name: &str) -> Self {
        Self {
            id: name.to_owned(),
            label: msg::raw(name),
        }
    }
}

impl From<&String> for Subject {
    fn from(name: &String) -> Self {
        name.as_str().into()
    }
}

impl Found<'_> {
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        check: &str,
        severity: Severity,
        subject: impl Into<Subject>,
        title: Text,
        detail: Text,
        evidence: Vec<Text>,
        fixes: Vec<Fix>,
    ) {
        let subject = subject.into();
        self.issues.push(Issue {
            id: format!("{check}:{}:{}", self.account.unwrap_or("-"), subject.id),
            check: check.to_owned(),
            severity,
            account_id: self.account.map(str::to_owned),
            subject: subject.id,
            label: subject.label,
            title,
            detail,
            evidence,
            fixes,
            tunnel_id: self.tunnel.map(str::to_owned),
        });
    }
}

fn describe(record: &DnsRecord) -> Text {
    if record.proxied {
        m::record_proxied(&record.name, &record.kind, &record.content)
    } else {
        m::record(&record.name, &record.kind, &record.content)
    }
}

/// Checks on the connector's recent logs: patterns cloudflared prints for problems the
/// user can fix. The newest matching line is the evidence.
fn log_checks(found: &mut Found<'_>, tunnel: &str, logs: &[String]) {
    let last = |needles: &[&str]| {
        logs.iter()
            .rev()
            .find(|line| {
                let lower = line.to_ascii_lowercase();
                needles.iter().all(|n| lower.contains(n))
            })
            .cloned()
    };
    let quic = last(&["quic", "timeout"])
        .or_else(|| last(&["failed to dial to edge with quic"]))
        .or_else(|| last(&["quic", "no recent network activity"]));
    if let Some(line) = quic {
        found.add(
            "net.udp_blocked",
            Severity::Warning,
            tunnel,
            m::udp_blocked::title(),
            m::udp_blocked::detail(),
            vec![msg::raw(line)],
            Vec::new(),
        );
    }
    if let Some(line) = last(&["x509"]).or_else(|| last(&["tls", "origin"])) {
        found.add(
            "origin.tls",
            Severity::Warning,
            tunnel,
            m::origin_tls::title(),
            m::origin_tls::detail(),
            vec![msg::raw(line)],
            Vec::new(),
        );
    }
    if let Some(line) = last(&["certificate", "not yet valid"]).or_else(|| last(&["clock skew"])) {
        found.add(
            "net.clock_skew",
            Severity::Warning,
            tunnel,
            m::clock_skew::title(),
            m::clock_skew::detail(),
            vec![msg::raw(line)],
            Vec::new(),
        );
    }
}

/// Runs every check.
pub fn diagnose(facts: &Facts) -> Vec<Issue> {
    let mut found = Found {
        account: None,
        tunnel: None,
        issues: Vec::new(),
    };
    match &facts.binary {
        BinaryFact::Ok => {}
        BinaryFact::Missing => found.add(
            "binary.missing",
            Severity::Error,
            "cloudflared",
            m::binary_missing::title(),
            m::binary_missing::detail(),
            Vec::new(),
            vec![Fix::InstallBinary],
        ),
        BinaryFact::Unsupported { version } => found.add(
            "binary.unsupported",
            Severity::Warning,
            "cloudflared",
            m::binary_unsupported::title(),
            m::binary_unsupported::detail(),
            version.iter().map(m::binary_unsupported::version).collect(),
            vec![Fix::InstallBinary],
        ),
    }
    for connector in &facts.foreign {
        use crate::discovery::cloudflared::ForeignMode;
        use m::foreign_running as f;
        let title = match &connector.mode {
            ForeignMode::QuickTunnel { origin } => f::quick_tunnel(origin),
            ForeignMode::Named {
                tunnel: Some(name), ..
            } => f::named(name),
            ForeignMode::Named { .. } => f::unnamed(),
            ForeignMode::Other => f::other(),
        };
        found.add(
            "tunnel.foreign_running",
            Severity::Info,
            Subject::named(
                &format!("cloudflared (pid {})", connector.pid),
                f::subject(connector.pid),
            ),
            title,
            if connector.service {
                f::detail_service()
            } else {
                f::detail()
            },
            vec![msg::raw(&connector.command)],
            Vec::new(),
        );
    }
    let mut issues = found.issues;
    for account in &facts.accounts {
        issues.extend(diagnose_account(account, &facts.foreign));
    }
    issues.sort_by(|a, b| {
        (a.severity, &a.subject, &a.check).cmp(&(b.severity, &b.subject, &b.check))
    });
    issues
}

#[allow(clippy::too_many_lines)]
fn diagnose_account(
    facts: &AccountFacts,
    foreign: &[crate::discovery::cloudflared::ForeignConnector],
) -> Vec<Issue> {
    let account = facts.account_id.as_str();
    let mut found = Found {
        account: Some(account),
        tunnel: facts.tunnel.as_deref(),
        issues: Vec::new(),
    };

    if facts.can_manage_routes == Some(false) {
        found.add(
            "auth.missing_scope",
            Severity::Warning,
            Subject::named("Permissions", m::missing_scope::subject()),
            m::missing_scope::title(),
            m::missing_scope::detail(),
            Vec::new(),
            vec![Fix::Reconnect],
        );
    }

    for domain in &facts.domains {
        if domain.status == DomainStatus::Pending {
            found.add(
                "zone.pending",
                Severity::Warning,
                &domain.name,
                m::zone_pending::title(&domain.name),
                m::zone_pending::detail(),
                domain
                    .name_servers
                    .iter()
                    .map(m::zone_pending::nameserver)
                    .collect(),
                Vec::new(),
            );
        }
    }

    if let Some(drift) = &facts.drift {
        found.add(
            "config.drift",
            Severity::Warning,
            Subject::named("Routes", m::config_drift::subject()),
            m::config_drift::title(),
            m::config_drift::detail(),
            drift
                .changes
                .iter()
                .map(|c| msg::raw(&c.hostname))
                .collect(),
            vec![
                Fix::KeepTheirs {
                    account_id: account.to_owned(),
                },
                Fix::Change {
                    label: m::fix::restore_mine(),
                    change: Change::RestoreConfig,
                },
            ],
        );
    }

    let tunnel = facts.snapshot.tunnel.as_ref();
    let target = tunnel.map(|t| tunnel_target(&t.id));
    let routes = facts.snapshot.routes();
    let routed: HashSet<&str> = routes
        .iter()
        .filter_map(|r| r.hostname.as_deref())
        .collect();

    for rule in &routes {
        let Some(hostname) = rule.hostname.as_deref() else {
            continue;
        };
        let records: Vec<_> = facts
            .snapshot
            .records_named(hostname)
            .filter(|r| matches!(r.record.kind.as_str(), "A" | "AAAA" | "CNAME"))
            .collect();
        let fix = || Fix::Change {
            label: m::fix::fix_dns(),
            change: Change::AddRoute {
                route: RouteInput {
                    hostname: hostname.to_owned(),
                    path: rule.path.clone(),
                    origin: rule.service.clone(),
                    access: None,
                    options: Some(Box::new(crate::domain::OriginOptions::from_map(
                        &rule.origin_request,
                    ))),
                },
            },
        };
        let points_here = |r: &&&crate::engine::ObservedRecord| {
            target
                .as_deref()
                .is_some_and(|t| r.record.content.eq_ignore_ascii_case(t))
        };
        if records.is_empty() {
            found.add(
                "dns.missing",
                Severity::Error,
                hostname,
                m::dns_missing::title(hostname),
                m::dns_missing::detail(),
                Vec::new(),
                vec![fix()],
            );
        } else if let Some(record) = records.iter().find(points_here) {
            if !record.record.proxied {
                found.add(
                    "dns.not_proxied",
                    Severity::Error,
                    hostname,
                    m::dns_not_proxied::title(hostname),
                    m::dns_not_proxied::detail(),
                    vec![describe(&record.record)],
                    vec![fix()],
                );
            }
        } else {
            let tunnel_record = records.iter().any(|r| {
                r.record
                    .content
                    .to_ascii_lowercase()
                    .ends_with(TUNNEL_SUFFIX)
            });
            let (check, title) = if tunnel_record {
                ("dns.wrong_target", m::dns_wrong_target::title(hostname))
            } else {
                ("dns.conflict", m::dns_conflict::title(hostname))
            };
            found.add(
                check,
                Severity::Error,
                hostname,
                title,
                m::dns_conflict::detail(),
                records.iter().map(|r| describe(&r.record)).collect(),
                vec![fix()],
            );
        }

        if let Ok(origin) = RouteOrigin::parse(&rule.service)
            && let Some(port) = origin.port()
            && facts.listening.get(&port) == Some(&false)
            // A stopped inspector has its own issue (`inspect.orphan`) and fix.
            && !facts.lens_orphans.iter().any(|o| o.lens_url == rule.service)
        {
            found.add(
                "origin.not_listening",
                Severity::Warning,
                hostname,
                m::origin_not_listening::title(port),
                m::origin_not_listening::detail(),
                vec![msg::raw(format!("{hostname} → {}", rule.service))],
                Vec::new(),
            );
        }
    }

    if let Some(tunnel) = tunnel {
        let has_routes = !routes.is_empty() || !shared_networks(&facts.snapshot).is_empty();
        match &facts.connector {
            _ if !has_routes => {}
            None | Some(ConnectorState::Stopped) => found.add(
                "tunnel.no_connections",
                Severity::Error,
                &tunnel.name,
                m::no_connections::title(),
                m::no_connections::detail(),
                Vec::new(),
                vec![Fix::StartConnector {
                    account_id: account.to_owned(),
                }],
            ),
            Some(ConnectorState::CrashLoop { exit_code }) => found.add(
                "tunnel.crash_loop",
                Severity::Error,
                &tunnel.name,
                m::crash_loop::title(),
                m::crash_loop::detail(),
                exit_code.iter().map(m::crash_loop::exit_code).collect(),
                vec![Fix::StartConnector {
                    account_id: account.to_owned(),
                }],
            ),
            Some(ConnectorState::Degraded) => found.add(
                "tunnel.degraded",
                Severity::Warning,
                &tunnel.name,
                m::degraded::title(),
                m::degraded::detail(),
                Vec::new(),
                Vec::new(),
            ),
            Some(_) => {}
        }
        let running = matches!(
            facts.connector,
            Some(ref state) if !matches!(state, ConnectorState::Stopped)
        );
        let connectors = facts
            .tunnels
            .iter()
            .find(|t| t.id == tunnel.id)
            .map_or(&[][..], |t| t.connectors.as_slice());
        let listed: usize = connectors.iter().map(|c| c.connections.len()).sum();
        if !running && listed > 0 {
            found.add(
                "tunnel.stale_connections",
                Severity::Warning,
                &tunnel.name,
                m::stale_connections::title(),
                m::stale_connections::detail(),
                vec![m::stale_connections::connections(listed as u64)],
                vec![Fix::CleanConnections {
                    account_id: account.to_owned(),
                    tunnel_id: tunnel.id.clone(),
                }],
            );
        }
        let twins: Vec<String> = foreign
            .iter()
            .filter(|f| {
                matches!(&f.mode, crate::discovery::cloudflared::ForeignMode::Named { tunnel: Some(t), .. }
                    if *t == tunnel.id || *t == tunnel.name)
            })
            .map(|f| f.command.clone())
            .collect();
        // Another machine running this Mac's tunnel gets a share of its requests and sends
        // them to *its* localhost. (A twin on this Mac is reported below instead.)
        let others: Vec<&crate::engine::ConnectorView> = if connectors.iter().any(|c| c.this_mac) {
            connectors.iter().filter(|c| !c.this_mac).collect()
        } else if connectors.len() > 1 {
            connectors.iter().collect()
        } else {
            Vec::new()
        };
        if running && twins.is_empty() && !others.is_empty() {
            found.add(
                "tunnel.other_connectors",
                Severity::Warning,
                &tunnel.name,
                m::other_connectors::title(),
                m::other_connectors::detail(),
                others
                    .iter()
                    .map(|c| {
                        let colos: Vec<&str> =
                            c.connections.iter().map(|x| x.colo.as_str()).collect();
                        msg::raw(format!(
                            "{} · cloudflared {} · {}",
                            c.origin_ip,
                            c.version,
                            colos.join(", ").to_ascii_uppercase()
                        ))
                    })
                    .collect(),
                Vec::new(),
            );
        }
        if !twins.is_empty() {
            found.add(
                "tunnel.duplicate_local",
                Severity::Warning,
                &tunnel.name,
                m::duplicate_local::title(),
                m::duplicate_local::detail(),
                twins.iter().map(msg::raw).collect(),
                Vec::new(),
            );
        }
        if !has_routes {
            found.add(
                "tunnel.unused_owned",
                Severity::Info,
                &tunnel.name,
                m::unused_owned::title(),
                m::unused_owned::detail(),
                Vec::new(),
                vec![Fix::Change {
                    label: m::fix::delete_tunnel(),
                    change: Change::RemoveTunnel,
                }],
            );
        }
    }

    if let Some(tunnel) = tunnel {
        log_checks(&mut found, &tunnel.name, &facts.logs);
    }

    let known: HashSet<&str> = facts
        .tunnels
        .iter()
        .map(|t| t.id.as_str())
        .chain(tunnel.map(|t| t.id.as_str()))
        .collect();
    for (zone_id, record) in &facts.tunnel_cnames {
        let content = record.content.to_ascii_lowercase();
        let Some(tunnel_id) = content.strip_suffix(TUNNEL_SUFFIX) else {
            continue;
        };
        let owned = facts.owned.contains(&record.id)
            || record
                .comment
                .as_deref()
                .is_some_and(|c| c.starts_with("teitunnel:"));
        let ours = tunnel.is_some_and(|t| t.id == tunnel_id);
        let delete = Fix::Change {
            label: m::fix::delete_record(),
            change: Change::DeleteRecord {
                zone_id: zone_id.clone(),
                hostname: record.name.clone(),
                record_id: record.id.clone(),
            },
        };
        if ours && !routed.contains(record.name.as_str()) {
            found.add(
                if owned {
                    "dns.orphan_owned"
                } else {
                    "dns.orphan_foreign"
                },
                Severity::Warning,
                &record.name,
                m::orphan_route::title(&record.name),
                m::orphan_route::detail(),
                vec![describe(record)],
                vec![delete],
            );
        } else if !known.contains(tunnel_id) {
            found.add(
                if owned {
                    "dns.orphan_owned"
                } else {
                    "dns.orphan_foreign"
                },
                Severity::Warning,
                &record.name,
                m::orphan_tunnel::title(&record.name),
                m::orphan_tunnel::detail(),
                vec![describe(record)],
                vec![delete],
            );
        }
    }

    let shared = shared_networks(&facts.snapshot);
    if !shared.is_empty() {
        if facts
            .warp
            .settings
            .is_some_and(|s| s.gateway_proxy_enabled == Some(false))
        {
            found.add(
                "network.proxy_off",
                Severity::Warning,
                Subject::named("Private networks", m::proxy_off::subject()),
                m::proxy_off::title(),
                m::proxy_off::detail(),
                vec![m::proxy_off::shared(
                    shared
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join(", "),
                )],
                Vec::new(),
            );
        }
        for range in &shared {
            let Some((check, evidence)) = facts
                .warp
                .profile
                .as_ref()
                .and_then(|p| split_tunnel_problem(p, range))
            else {
                continue;
            };
            let (title, detail) = if check == "network.excluded" {
                (
                    m::network_excluded::title(range),
                    m::network_excluded::detail(range),
                )
            } else {
                (
                    m::network_not_included::title(range),
                    m::network_not_included::detail(range),
                )
            };
            found.add(
                check,
                Severity::Warning,
                range.to_string().as_str(),
                title,
                detail,
                evidence,
                Vec::new(),
            );
        }
    }

    for domain in &facts.orphan_logins {
        found.add(
            "access.orphan",
            Severity::Info,
            domain,
            m::access_orphan::title(domain),
            m::access_orphan::detail(),
            vec![m::access_orphan::app(domain)],
            vec![Fix::Change {
                label: m::fix::remove_login(),
                change: Change::RemoveLogin {
                    domain: domain.clone(),
                },
            }],
        );
    }

    for route in &facts.lens_orphans {
        use m::inspect_orphan as o;
        found.add(
            "inspect.orphan",
            Severity::Error,
            route.hostname.as_str(),
            o::title(&route.hostname),
            o::detail(),
            vec![
                o::address(&route.lens_url),
                o::original(&route.original_origin),
            ],
            vec![Fix::Change {
                label: o::fix(),
                change: route.restore_change(route.access.clone()),
            }],
        );
    }

    found.issues
}

#[cfg(test)]
mod tests {
    use cf_api::IngressRule;
    use serde_json::Map;

    use super::*;
    use crate::engine::{ObservedRecord, ObservedTunnel, ZoneRef};

    const T: &str = "6ff42ae2-765d-4adf-8112-31c55c1551ef";

    fn rule(host: Option<&str>, service: &str) -> IngressRule {
        IngressRule {
            hostname: host.map(str::to_owned),
            path: None,
            service: service.into(),
            origin_request: Map::new(),
            extra: Map::new(),
        }
    }

    fn record(id: &str, name: &str, kind: &str, content: &str, proxied: bool) -> DnsRecord {
        DnsRecord {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            content: content.into(),
            proxied,
            comment: None,
            ttl: 1,
        }
    }

    fn healthy() -> AccountFacts {
        let target = tunnel_target(T);
        AccountFacts {
            account_id: "acc".into(),
            tunnel: None,
            snapshot: Snapshot {
                account_id: "acc".into(),
                machine_name: "Mac".into(),
                zones: vec![ZoneRef {
                    id: "z".into(),
                    name: "xyz.com".into(),
                }],
                tunnel: Some(ObservedTunnel {
                    id: T.into(),
                    name: "Mac".into(),
                    config_version: 2,
                    ingress: vec![
                        rule(Some("app.xyz.com"), "http://localhost:3000"),
                        rule(None, "http_status:404"),
                    ],
                }),
                tunnel_names: Vec::new(),
                elsewhere: Vec::new(),
                balance: None,
                site: None,
                held: Vec::new(),
                owner: String::new(),
                now: 0,
                edge: None,
                service_tokens: None,
                records: vec![ObservedRecord {
                    zone_id: "z".into(),
                    record: record("r1", "app.xyz.com", "CNAME", &target, true),
                    owned: true,
                }],
                access: None,
                networks: None,
            },
            tunnels: Vec::new(),
            drift: None,
            domains: Vec::new(),
            tunnel_cnames: vec![(
                "z".into(),
                record("r1", "app.xyz.com", "CNAME", &target, true),
            )],
            owned: HashSet::from(["r1".to_owned()]),
            can_manage_routes: Some(true),
            connector: Some(ConnectorState::Healthy { connections: 4 }),
            listening: HashMap::from([(3000, true)]),
            logs: Vec::new(),
            orphan_logins: Vec::new(),
            lens_orphans: Vec::new(),
            warp: WarpFacts::default(),
        }
    }

    #[test]
    fn a_route_left_on_a_stopped_inspector_can_be_restored() {
        let mut facts = healthy();
        facts
            .lens_orphans
            .push(crate::inspect::routes::InspectedRoute {
                account_id: "a".into(),
                hostname: "app.xyz.com".into(),
                path: None,
                tunnel_id: None,
                original_origin: "http://localhost:3000".into(),
                access: None,
                lens_url: "http://127.0.0.1:49152".into(),
                owner: "app".into(),
                created_at: 1,
            });
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts],
            foreign: Vec::new(),
        });
        let issue = issues.iter().find(|i| i.check == "inspect.orphan").unwrap();
        assert_eq!(issue.severity, Severity::Error);
        assert!(matches!(
            &issue.fixes[0],
            Fix::Change { change: Change::UpdateRoute { route, .. }, .. }
                if route.origin == "http://localhost:3000" && route.hostname == "app.xyz.com"
        ));
    }

    fn checks(facts: AccountFacts) -> Vec<String> {
        diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts],
            foreign: Vec::new(),
        })
        .into_iter()
        .map(|i| i.check)
        .collect()
    }

    #[test]
    fn a_healthy_setup_has_no_issues() {
        let mut facts = healthy();
        facts.tunnels = vec![TunnelSummary {
            id: T.into(),
            name: "Mac".into(),
            status: "healthy".into(),
            created_at: String::new(),
            routes: Some(1),
            connectors: Vec::new(),
            this_mac: true,
            is_default: true,
            connector: None,
        }];
        assert!(checks(facts).is_empty());
    }

    #[test]
    fn dns_problems_are_found_with_a_fix() {
        let mut missing = healthy();
        missing.snapshot.records.clear();
        assert_eq!(checks(missing), ["dns.missing"]);

        let mut grey = healthy();
        grey.snapshot.records[0].record.proxied = false;
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![grey],
            foreign: Vec::new(),
        });
        let issue = issues
            .iter()
            .find(|i| i.check == "dns.not_proxied")
            .unwrap();
        assert!(matches!(
            &issue.fixes[0],
            Fix::Change { change: Change::AddRoute { route }, .. } if route.origin == "http://localhost:3000"
        ));

        let mut conflict = healthy();
        conflict.snapshot.records[0].record = record("r1", "app.xyz.com", "A", "192.0.2.1", false);
        assert!(checks(conflict).contains(&"dns.conflict".to_owned()));

        let mut other = healthy();
        other.snapshot.records[0].record.content = tunnel_target("other");
        assert!(checks(other).contains(&"dns.wrong_target".to_owned()));
    }

    #[test]
    fn connector_and_origin_problems() {
        let mut stopped = healthy();
        stopped.connector = None;
        stopped.listening.insert(3000, false);
        let found = checks(stopped);
        assert!(found.contains(&"tunnel.no_connections".to_owned()));
        assert!(found.contains(&"origin.not_listening".to_owned()));

        let mut looping = healthy();
        looping.connector = Some(ConnectorState::CrashLoop { exit_code: Some(1) });
        assert!(checks(looping).contains(&"tunnel.crash_loop".to_owned()));

        let mut idle = healthy();
        idle.snapshot.tunnel.as_mut().unwrap().ingress = vec![rule(None, "http_status:404")];
        idle.connector = None;
        idle.tunnel_cnames.clear();
        assert_eq!(
            checks(idle),
            ["tunnel.unused_owned"],
            "no routes: nothing to connect"
        );
    }

    #[test]
    fn orphans_owned_and_foreign() {
        let mut facts = healthy();
        let mut owned = record("o1", "old.xyz.com", "CNAME", &tunnel_target(T), true);
        owned.comment = Some("teitunnel:route=abc".into());
        facts.tunnel_cnames.push(("z".into(), owned));
        facts.tunnel_cnames.push((
            "z".into(),
            record("f1", "gone.xyz.com", "CNAME", &tunnel_target("dead"), true),
        ));
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts],
            foreign: Vec::new(),
        });
        let owned = issues.iter().find(|i| i.subject == "old.xyz.com").unwrap();
        assert_eq!(owned.check, "dns.orphan_owned");
        let foreign = issues.iter().find(|i| i.subject == "gone.xyz.com").unwrap();
        assert_eq!(foreign.check, "dns.orphan_foreign");
        assert!(matches!(
            &foreign.fixes[0],
            Fix::Change { change: Change::DeleteRecord { record_id, .. }, .. } if record_id == "f1"
        ));
        assert!(
            !issues.iter().any(|i| i.subject == "app.xyz.com"),
            "a routed hostname isn't an orphan"
        );
    }

    #[test]
    fn a_login_without_its_route_can_be_removed() {
        let mut facts = healthy();
        facts.orphan_logins.push("old.xyz.com/admin".into());
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts],
            foreign: Vec::new(),
        });
        let issue = issues.iter().find(|i| i.check == "access.orphan").unwrap();
        assert_eq!(issue.severity, Severity::Info);
        assert!(matches!(
            &issue.fixes[0],
            Fix::Change { change: Change::RemoveLogin { domain }, .. } if domain == "old.xyz.com/admin"
        ));
        assert!(
            safe_change(issue).is_some(),
            "safe: only Teitunnel's own login"
        );
    }

    #[test]
    fn account_binary_and_domain_checks() {
        let mut facts = healthy();
        facts.can_manage_routes = Some(false);
        facts.domains = vec![Domain {
            id: "z2".into(),
            name: "yx.com".into(),
            status: DomainStatus::Pending,
            name_servers: vec!["ada.ns.cloudflare.com".into()],
            original_name_servers: Vec::new(),
            plan: None,
            paused: false,
        }];
        let issues = diagnose(&Facts {
            binary: BinaryFact::Missing,
            accounts: vec![facts],
            foreign: Vec::new(),
        });
        let found: Vec<&str> = issues.iter().map(|i| i.check.as_str()).collect();
        assert_eq!(found[0], "binary.missing", "errors sort first");
        assert!(found.contains(&"auth.missing_scope"));
        assert!(found.contains(&"zone.pending"));
        assert_eq!(issues[0].id, "binary.missing:-:cloudflared");
        let pending = issues.iter().find(|i| i.check == "zone.pending").unwrap();
        assert_eq!(pending.id, "zone.pending:acc:yx.com");
    }

    #[test]
    fn log_patterns_become_issues() {
        let mut facts = healthy();
        facts.logs = vec![
            "Registered tunnel connection".into(),
            "Failed to dial a quic connection error=\"timeout: no recent network activity\"".into(),
            "Unable to reach the origin service. tls: failed to verify certificate: x509: certificate signed by unknown authority".into(),
        ];
        let found = checks(facts);
        assert!(found.contains(&"net.udp_blocked".to_owned()), "{found:?}");
        assert!(found.contains(&"origin.tls".to_owned()), "{found:?}");
        assert!(!found.contains(&"net.clock_skew".to_owned()));
    }

    #[test]
    fn stale_and_duplicate_connectors() {
        use crate::{
            discovery::cloudflared::{ForeignConnector, ForeignMode},
            engine::{ConnectionView, ConnectorView},
        };
        let mut facts = healthy();
        facts.connector = None;
        facts.tunnels = vec![TunnelSummary {
            id: T.into(),
            name: "Mac".into(),
            status: "healthy".into(),
            created_at: String::new(),
            routes: Some(1),
            connectors: vec![ConnectorView {
                id: "c1".into(),
                version: "2026.9.1".into(),
                origin_ip: "203.0.113.1".into(),
                this_mac: false,
                connections: vec![ConnectionView {
                    colo: "ams01".into(),
                    opened_at: String::new(),
                }],
            }],
            this_mac: true,
            is_default: true,
            connector: None,
        }];
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts],
            foreign: vec![ForeignConnector {
                pid: 9,
                command: "cloudflared tunnel run Mac".into(),
                mode: ForeignMode::Named {
                    tunnel: Some("Mac".into()),
                    config: None,
                },
                service: false,
                metrics: None,
                connections: None,
            }],
        });
        let found: Vec<&str> = issues.iter().map(|i| i.check.as_str()).collect();
        assert!(found.contains(&"tunnel.stale_connections"), "{found:?}");
        assert!(found.contains(&"tunnel.duplicate_local"), "{found:?}");
        assert!(found.contains(&"tunnel.foreign_running"), "{found:?}");
    }

    #[test]
    fn another_machine_running_this_macs_tunnel() {
        use crate::engine::{ConnectionView, ConnectorView};
        let connector = |id: &str, this_mac: bool, ip: &str| ConnectorView {
            id: id.into(),
            version: "2026.9.1".into(),
            origin_ip: ip.into(),
            this_mac,
            connections: vec![ConnectionView {
                colo: "ams01".into(),
                opened_at: String::new(),
            }],
        };
        let with = |connectors: Vec<ConnectorView>| {
            let mut facts = healthy();
            facts.connector = Some(ConnectorState::Healthy { connections: 4 });
            facts.tunnels = vec![TunnelSummary {
                id: T.into(),
                name: "Mac".into(),
                status: "healthy".into(),
                created_at: String::new(),
                routes: Some(1),
                connectors,
                this_mac: true,
                is_default: true,
                connector: None,
            }];
            diagnose(&Facts {
                binary: BinaryFact::Ok,
                accounts: vec![facts],
                foreign: Vec::new(),
            })
        };
        let issues = with(vec![
            connector("mine", true, "203.0.113.1"),
            connector("old-laptop", false, "198.51.100.9"),
        ]);
        let issue = issues
            .iter()
            .find(|i| i.check == "tunnel.other_connectors")
            .expect("reported");
        assert_eq!(
            issue.evidence,
            [msg::raw("198.51.100.9 · cloudflared 2026.9.1 · AMS01")]
        );
        // Only this Mac: nothing to report. Unknown id with a single connector: it's ours.
        for alone in [
            vec![connector("mine", true, "203.0.113.1")],
            vec![connector("unknown", false, "203.0.113.1")],
        ] {
            assert!(
                !with(alone)
                    .iter()
                    .any(|i| i.check == "tunnel.other_connectors")
            );
        }
        // Unknown id, two connectors: one of them isn't this Mac.
        assert!(
            with(vec![
                connector("a", false, "203.0.113.1"),
                connector("b", false, "198.51.100.9"),
            ])
            .iter()
            .any(|i| i.check == "tunnel.other_connectors")
        );
    }

    fn sharing(networks: &[&str]) -> AccountFacts {
        let mut facts = healthy();
        facts.snapshot.networks = Some(crate::engine::NetworkState {
            default_vnet: Some("v".into()),
            routes: networks
                .iter()
                .enumerate()
                .map(|(i, n)| crate::engine::ObservedNetworkRoute {
                    id: format!("n{i}"),
                    network: (*n).into(),
                    tunnel_id: T.into(),
                    tunnel_name: None,
                    virtual_network_id: Some("v".into()),
                    comment: String::new(),
                })
                .collect(),
        });
        facts
    }

    fn entry(address: &str, description: Option<&str>) -> cf_api::SplitTunnelEntry {
        cf_api::SplitTunnelEntry {
            address: Some(address.into()),
            host: None,
            description: description.map(str::to_owned),
        }
    }

    #[test]
    fn warp_settings_that_keep_clients_from_a_shared_network() {
        let mut facts = sharing(&["192.168.1.0/24", "fd00::/64"]);
        facts.warp = WarpFacts {
            settings: Some(cf_api::DeviceSettings {
                gateway_proxy_enabled: Some(false),
                gateway_udp_proxy_enabled: None,
            }),
            // Cloudflare's default: private space is excluded.
            profile: Some(cf_api::DefaultDeviceProfile {
                exclude: Some(vec![
                    entry("192.168.0.0/16", Some("RFC 1918")),
                    entry("10.0.0.0/8", None),
                ]),
                include: None,
            }),
        };
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts.clone()],
            foreign: Vec::new(),
        });
        let found: Vec<(&str, &str)> = issues
            .iter()
            .map(|i| (i.check.as_str(), i.subject.as_str()))
            .collect();
        assert_eq!(
            found,
            [
                ("network.excluded", "192.168.1.0/24"),
                ("network.proxy_off", "Private networks"),
            ]
        );
        assert_eq!(
            issues[0].evidence[0].english(),
            "Excluded: 192.168.0.0/16 (RFC 1918)"
        );

        // Include mode: only listed ranges go through WARP.
        facts.warp = WarpFacts {
            settings: None,
            profile: Some(cf_api::DefaultDeviceProfile {
                exclude: None,
                include: Some(vec![entry("192.168.0.0/16", None)]),
            }),
        };
        assert_eq!(checks(facts.clone()), ["network.not_included"]);
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts.clone()],
            foreign: Vec::new(),
        });
        assert_eq!(issues[0].subject, "fd00::/64");

        // Unknown settings or nothing shared: nothing to say.
        facts.warp = WarpFacts::default();
        assert!(checks(facts).is_empty());
        let mut idle = healthy();
        idle.warp.settings = Some(cf_api::DeviceSettings {
            gateway_proxy_enabled: Some(false),
            gateway_udp_proxy_enabled: None,
        });
        assert!(checks(idle).is_empty());

        // A tunnel that only carries a network is in use, and needs its connector.
        let mut network_only = sharing(&["192.168.1.0/24"]);
        network_only.snapshot.tunnel.as_mut().unwrap().ingress = Vec::new();
        network_only.tunnel_cnames.clear();
        network_only.connector = None;
        assert_eq!(checks(network_only), ["tunnel.no_connections"]);
    }
}
