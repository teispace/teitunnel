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
use serde::Serialize;
use tokio::net::TcpStream;

use crate::{
    accounts::{Domain, DomainStatus},
    domain::RouteOrigin,
    engine::{
        AccessNeed, Change, CloudApi, Connectors, Context, Drift, Engine, EngineError,
        ObserveError, RouteInput, Snapshot, TunnelSummary, observe, tunnel_target,
    },
    runtime::ConnectorState,
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
        label: String,
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
    /// What it's about, e.g. a hostname or a tunnel name.
    pub subject: String,
    /// One line.
    pub title: String,
    /// What it means and what to do.
    pub detail: String,
    /// Supporting facts (records, states).
    pub evidence: Vec<String>,
    /// Fixes, the recommended one first.
    pub fixes: Vec<Fix>,
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
        ctx.machine_name,
        None,
        &AccessNeed::none(),
    )
    .await?;
    let tunnels = engine.tunnels(api, connectors, ctx.account).await?;
    let drift = engine.drift(api, ctx.account).await?;
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
    Ok(AccountFacts {
        account_id: ctx.account.to_owned(),
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
    })
}

/// Checks everything: the binary and every connected account. An account that can't
/// be read becomes an issue itself rather than failing the run.
pub async fn run(
    accounts: &crate::accounts::Accounts,
    engine: &Engine,
    machine: &crate::machine::MachineTunnels,
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
        let ctx = Context {
            account: &account.id,
            machine_name,
        };
        let gathered = async {
            let api = accounts
                .client(&account.id)
                .await
                .map_err(|e| e.to_string())?;
            let domains = accounts
                .domains(&account.id)
                .await
                .map_err(|e| e.to_string())?;
            let can_manage = accounts
                .capabilities(&account.id)
                .await
                .ok()
                .map(|c| c.can_manage_routes());
            gather(engine, &api, machine, ctx, domains, can_manage)
                .await
                .map_err(|e| e.to_string())
        }
        .await;
        match gathered {
            Ok(account_facts) => facts.accounts.push(account_facts),
            Err(message) => unreachable.push(Issue {
                id: format!("account.unreachable:{}:{}", account.id, account.name),
                check: "account.unreachable".into(),
                severity: Severity::Error,
                account_id: Some(account.id.clone()),
                subject: account.name.clone(),
                title: format!("Couldn't check {}", account.name),
                detail: message,
                evidence: Vec::new(),
                fixes: vec![Fix::Reconnect],
            }),
        }
    }
    let mut issues = diagnose(&facts);
    issues.extend(unreachable);
    issues.sort_by(|a, b| {
        (a.severity, &a.subject, &a.check).cmp(&(b.severity, &b.subject, &b.check))
    });
    issues
}

/// Runs the checks, then applies every safe fix in every account.
pub async fn fix_all_safe(
    accounts: &crate::accounts::Accounts,
    engine: &Engine,
    machine: &crate::machine::MachineTunnels,
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

/// Whether an issue's fix may run without review: DNS repairs and orphan deletions only.
/// Whether it really is safe is decided by its plan (no confirmation needed = nothing
/// Teitunnel doesn't own is touched).
fn candidate(issue: &Issue) -> Option<&Change> {
    match issue.fixes.first()? {
        Fix::Change { change, .. }
            if matches!(
                change,
                Change::AddRoute { .. } | Change::DeleteRecord { .. }
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
        let Some(change) = candidate(issue) else {
            report.skipped += 1;
            continue;
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
    issues: Vec<Issue>,
}

impl Found<'_> {
    #[allow(clippy::too_many_arguments)]
    fn add(
        &mut self,
        check: &str,
        severity: Severity,
        subject: &str,
        title: String,
        detail: &str,
        evidence: Vec<String>,
        fixes: Vec<Fix>,
    ) {
        self.issues.push(Issue {
            id: format!("{check}:{}:{subject}", self.account.unwrap_or("-")),
            check: check.to_owned(),
            severity,
            account_id: self.account.map(str::to_owned),
            subject: subject.to_owned(),
            title,
            detail: detail.to_owned(),
            evidence,
            fixes,
        });
    }
}

fn describe(record: &DnsRecord) -> String {
    format!(
        "{} {} {}{}",
        record.name,
        record.kind,
        record.content,
        if record.proxied { " (proxied)" } else { "" }
    )
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
            "This network seems to block QUIC (UDP)".into(),
            "cloudflared falls back to HTTP/2, which works but can be slower to reconnect. If connections keep dropping, check firewall rules for UDP port 7844.",
            vec![line],
            Vec::new(),
        );
    }
    if let Some(line) = last(&["x509"]).or_else(|| last(&["tls", "origin"])) {
        found.add(
            "origin.tls",
            Severity::Warning,
            tunnel,
            "An HTTPS origin's certificate isn't trusted".into(),
            "The connector couldn't verify the origin's certificate. Use http:// for a local origin, or a certificate the Mac trusts.",
            vec![line],
            Vec::new(),
        );
    }
    if let Some(line) = last(&["certificate", "not yet valid"]).or_else(|| last(&["clock skew"])) {
        found.add(
            "net.clock_skew",
            Severity::Warning,
            tunnel,
            "This Mac's clock may be wrong".into(),
            "Certificates look expired or not yet valid, which usually means the clock is off. Turn on Set time and date automatically in System Settings.",
            vec![line],
            Vec::new(),
        );
    }
}

/// Runs every check.
pub fn diagnose(facts: &Facts) -> Vec<Issue> {
    let mut found = Found {
        account: None,
        issues: Vec::new(),
    };
    match &facts.binary {
        BinaryFact::Ok => {}
        BinaryFact::Missing => found.add(
            "binary.missing",
            Severity::Error,
            "cloudflared",
            "cloudflared isn't installed".into(),
            "Routes and Quick Share need cloudflared. Teitunnel can install a verified copy.",
            Vec::new(),
            vec![Fix::InstallBinary],
        ),
        BinaryFact::Unsupported { version } => found.add(
            "binary.unsupported",
            Severity::Warning,
            "cloudflared",
            "cloudflared is too old".into(),
            "Some features need a newer cloudflared. Install the managed copy to update.",
            version.iter().map(|v| format!("Version {v}")).collect(),
            vec![Fix::InstallBinary],
        ),
    }
    for connector in &facts.foreign {
        use crate::discovery::cloudflared::ForeignMode;
        let what = match &connector.mode {
            ForeignMode::QuickTunnel { origin } => format!("a Quick Tunnel for {origin}"),
            ForeignMode::Named {
                tunnel: Some(name), ..
            } => format!("tunnel {name}"),
            ForeignMode::Named { .. } => "a tunnel".to_owned(),
            ForeignMode::Other => "cloudflared".to_owned(),
        };
        found.add(
            "tunnel.foreign_running",
            Severity::Info,
            &format!("cloudflared (pid {})", connector.pid),
            format!("cloudflared is running {what} outside Teitunnel"),
            if connector.service {
                "It was started as a background service. Teitunnel leaves it alone; you can stop it from Tunnels, or import its routes."
            } else {
                "It was started outside Teitunnel. Teitunnel leaves it alone; you can stop it from Tunnels, or import its routes."
            },
            vec![connector.command.clone()],
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
        issues: Vec::new(),
    };

    if facts.can_manage_routes == Some(false) {
        found.add(
            "auth.missing_scope",
            Severity::Warning,
            "Permissions",
            "This account's token can't manage routes".into(),
            "Routes need Cloudflare Tunnel · Edit and DNS · Edit. Create a token with those permissions and connect it again.",
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
                format!("{} is waiting for its nameservers", domain.name),
                "Routes on this domain won't work until the registrar uses Cloudflare's nameservers.",
                domain.name_servers.iter().map(|ns| format!("Nameserver {ns}")).collect(),
                Vec::new(),
            );
        }
    }

    if let Some(drift) = &facts.drift {
        found.add(
            "config.drift",
            Severity::Warning,
            "Routes",
            "This Mac's routes were changed outside Teitunnel".into(),
            "Keep the changes, or restore what Teitunnel set up.",
            drift.changes.iter().map(|c| c.hostname.clone()).collect(),
            vec![
                Fix::KeepTheirs {
                    account_id: account.to_owned(),
                },
                Fix::Change {
                    label: "Restore Mine".into(),
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
            label: "Fix the DNS Record".into(),
            change: Change::AddRoute {
                route: RouteInput {
                    hostname: hostname.to_owned(),
                    path: rule.path.clone(),
                    origin: rule.service.clone(),
                    access: None,
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
                format!("{hostname} has no DNS record"),
                "The tunnel serves this hostname, but nothing points it at the tunnel, so it doesn't resolve.",
                Vec::new(),
                vec![fix()],
            );
        } else if let Some(record) = records.iter().find(points_here) {
            if !record.record.proxied {
                found.add(
                    "dns.not_proxied",
                    Severity::Error,
                    hostname,
                    format!("{hostname} isn't proxied through Cloudflare"),
                    "Tunnel hostnames only work when the record is proxied (orange cloud).",
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
                (
                    "dns.wrong_target",
                    format!("{hostname} points at another tunnel"),
                )
            } else {
                (
                    "dns.conflict",
                    format!("Another record answers for {hostname}"),
                )
            };
            found.add(
                check,
                Severity::Error,
                hostname,
                title,
                "Requests for this hostname don't reach this Mac. Point the record at this Mac's tunnel.",
                records.iter().map(|r| describe(&r.record)).collect(),
                vec![fix()],
            );
        }

        if let Ok(origin) = RouteOrigin::parse(&rule.service)
            && let Some(port) = origin.port()
            && facts.listening.get(&port) == Some(&false)
        {
            found.add(
                "origin.not_listening",
                Severity::Warning,
                hostname,
                format!("Nothing is listening on port {port}"),
                "Start the app this route sends traffic to; visitors see an error until it runs.",
                vec![format!("{hostname} → {}", rule.service)],
                Vec::new(),
            );
        }
    }

    if let Some(tunnel) = tunnel {
        let has_routes = !routes.is_empty();
        match &facts.connector {
            _ if !has_routes => {}
            None | Some(ConnectorState::Stopped) => found.add(
                "tunnel.no_connections",
                Severity::Error,
                &tunnel.name,
                "This Mac's connector isn't running".into(),
                "Its routes don't answer until the connector runs.",
                Vec::new(),
                vec![Fix::StartConnector {
                    account_id: account.to_owned(),
                }],
            ),
            Some(ConnectorState::CrashLoop { exit_code }) => found.add(
                "tunnel.crash_loop",
                Severity::Error,
                &tunnel.name,
                "This Mac's connector keeps stopping".into(),
                "cloudflared exited several times in a row. Its logs say why; starting it again retries.",
                exit_code.iter().map(|c| format!("Last exit code {c}")).collect(),
                vec![Fix::StartConnector {
                    account_id: account.to_owned(),
                }],
            ),
            Some(ConnectorState::Degraded) => found.add(
                "tunnel.degraded",
                Severity::Warning,
                &tunnel.name,
                "This Mac's connector lost its connection".into(),
                "cloudflared is running but not connected to Cloudflare. It reconnects on its own; check the network if it persists.",
                Vec::new(),
                Vec::new(),
            ),
            Some(_) => {}
        }
        let running = matches!(
            facts.connector,
            Some(ref state) if !matches!(state, ConnectorState::Stopped)
        );
        let listed = facts
            .tunnels
            .iter()
            .find(|t| t.id == tunnel.id)
            .map_or(0, |t| t.connections.len());
        if !running && listed > 0 {
            found.add(
                "tunnel.stale_connections",
                Severity::Warning,
                &tunnel.name,
                "Cloudflare still lists connections for this Mac's tunnel".into(),
                "This Mac's connector isn't running, so they're left over, or another machine runs this tunnel with its token. Clean them up if nothing else should run it.",
                vec![format!("{listed} connection{}", if listed == 1 { "" } else { "s" })],
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
        if !twins.is_empty() {
            found.add(
                "tunnel.duplicate_local",
                Severity::Warning,
                &tunnel.name,
                "Another cloudflared on this Mac runs this Mac's tunnel".into(),
                "Two connectors on one Mac add nothing but confusion in logs and metrics. Stop the other one from Tunnels.",
                twins,
                Vec::new(),
            );
        }
        if !has_routes {
            found.add(
                "tunnel.unused_owned",
                Severity::Info,
                &tunnel.name,
                "This Mac's tunnel has no routes".into(),
                "It's kept so adding a route is quick. Delete it if you don't need it.",
                Vec::new(),
                vec![Fix::Change {
                    label: "Delete the Tunnel".into(),
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
            label: "Delete the Record".into(),
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
                format!(
                    "{} points at this Mac's tunnel but has no route",
                    record.name
                ),
                "Visitors get a 404 from the tunnel. Delete the record, or add a route for it.",
                vec![describe(record)],
                vec![delete],
            );
        } else if !known.contains(tunnel_id) {
            found.add(
                if owned { "dns.orphan_owned" } else { "dns.orphan_foreign" },
                Severity::Warning,
                &record.name,
                format!("{} points at a tunnel that no longer exists", record.name),
                "The hostname shows a Cloudflare error. Delete the record, or route it to a tunnel.",
                vec![describe(record)],
                vec![delete],
            );
        }
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
                records: vec![ObservedRecord {
                    zone_id: "z".into(),
                    record: record("r1", "app.xyz.com", "CNAME", &target, true),
                    owned: true,
                }],
                access: None,
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
        }
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
            connections: Vec::new(),
            this_mac: true,
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
            engine::ConnectionView,
        };
        let mut facts = healthy();
        facts.connector = None;
        facts.tunnels = vec![TunnelSummary {
            id: T.into(),
            name: "Mac".into(),
            status: "healthy".into(),
            created_at: String::new(),
            routes: Some(1),
            connections: vec![ConnectionView {
                colo: "ams01".into(),
                version: "2026.9.1".into(),
                origin_ip: "203.0.113.1".into(),
                opened_at: String::new(),
            }],
            this_mac: true,
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
}
