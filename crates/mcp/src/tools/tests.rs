//! Tool tests against an in-memory backend: every tool, the modes, approvals, plans
//! (fingerprints, staleness, confirmation of foreign records), redaction and paging.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use serde_json::{Value, json};
use teitunnel_core::{
    accounts::{Account, CredentialKind},
    discovery::{LocalService, ServiceKind},
    doctor::{Fix, Issue, Severity},
    domain::OriginOptions,
    engine::{
        ActivityEntry, ActivityKind, ActivityRecord, Actor, Change, Delta, DeltaArea, DnsState,
        Outcome, PlanView, Progress, RouteView, RoutesOverview, StepKind, StepState, StepView,
        TunnelSummary, TunnelView, Verification,
    },
    export::{ExportFile, ExportFormat},
    import::LocalSetup,
    remote_logs::RemoteLogState,
    runtime::ConnectorState,
    text::msg,
};
use tokio::sync::broadcast;

use crate::{
    backend::{
        ApplyApproval, Backend, BackendError, BackendResult, BoxFuture, ChangeEvent,
        DomainShareRequest, LogBatch, LogLine, ProgressSink, RemoteLogBatch, ShareInfo, ShareKind,
        SharedBackend, Target,
    },
    config::{Mode, Settings},
    plans::Plans,
    registry::{ToolClass, ToolContext, ToolProvider},
    traffic::{
        Body, Exchange, ExchangeSummary, HttpMessage, ReplayEdits, ReplayResult, TrafficError,
        TrafficFilter, TrafficPage, TrafficSource, TrafficStats,
    },
};

use super::CoreTools;

/// What the fake holds.
#[derive(Debug, Default)]
pub(crate) struct FakeState {
    pub(crate) routes: Vec<RouteView>,
    /// Bumped to make plans stale.
    pub(crate) version: u64,
    pub(crate) shares: Vec<ShareInfo>,
    pub(crate) activity: Vec<ActivityEntry>,
    pub(crate) applied: Vec<(Change, Option<Actor>)>,
    pub(crate) fixes_run: Vec<String>,
    /// Edge protection of `app.xyz.com`, and its service tokens.
    pub(crate) protection: teitunnel_core::engine::edge::EdgeProtection,
    pub(crate) tokens: Vec<teitunnel_core::protection::ServiceTokenView>,
    pub(crate) protection_applied: Vec<teitunnel_core::protection::ProtectionChange>,
    /// Pauses (`true`) and resumes, by hostname.
    pub(crate) paused: Vec<(String, bool)>,
    pub(crate) quick_paused: Vec<(String, bool)>,
    pub(crate) fronts_applied: Vec<teitunnel_core::fronts::FrontChange>,
    /// Schedules set (`None`: removed), by hostname.
    pub(crate) schedules: Vec<(String, Option<teitunnel_core::schedule::Schedule>)>,
    /// Folders shared.
    pub(crate) folders: Vec<teitunnel_core::folder_share::FolderShare>,
    /// Comment threads of `snapshot:s1` ("Launch").
    pub(crate) threads: Vec<teitunnel_core::comments::Thread>,
}

/// An in-memory Teitunnel with one account (`acc`, "Personal") and one tunnel.
#[derive(Debug)]
pub(crate) struct FakeBackend {
    pub(crate) state: Mutex<FakeState>,
    changes: broadcast::Sender<ChangeEvent>,
}

pub(crate) fn route(hostname: &str, origin: &str) -> RouteView {
    RouteView {
        hostname: hostname.into(),
        path: None,
        origin: origin.into(),
        local: true,
        zone: Some("xyz.com".into()),
        dns: DnsState::Ok,
        access: None,
        client: None,
        tunnel_id: Some("t1".into()),
        temporary: false,
        balanced: false,
        paused: false,
        options: OriginOptions::default(),
    }
}

impl FakeBackend {
    pub(crate) fn new() -> Arc<Self> {
        let (changes, _) = broadcast::channel(16);
        Arc::new(Self {
            state: Mutex::new(FakeState {
                routes: vec![route("app.xyz.com", "http://localhost:3000")],
                ..FakeState::default()
            }),
            changes,
        })
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn hostname(change: &Change) -> Option<String> {
        match change {
            Change::AddRoute { route } => Some(route.hostname.clone()),
            Change::UpdateRoute { hostname, .. }
            | Change::RemoveRoute { hostname, .. }
            | Change::ProtectHostname { hostname, .. } => Some(hostname.clone()),
            _ => None,
        }
    }

    fn view(&self, change: &Change) -> PlanView {
        let state = self.lock();
        let host = Self::hostname(change).unwrap_or_default();
        let exists = state.routes.iter().any(|r| r.hostname == host);
        let steps = match change {
            Change::AddRoute { .. } if exists => Vec::new(),
            Change::RemoveRoute { .. } if !exists => Vec::new(),
            _ => vec![
                StepView {
                    kind: StepKind::PutConfig,
                    description: msg::raw(format!("Update routes for {host}")),
                    command: None,
                },
                StepView {
                    kind: StepKind::CreateRecord,
                    description: msg::raw(format!("Point {host} at the tunnel")),
                    command: Some(format!("cloudflared tunnel route dns t1 {host}")),
                },
            ],
        };
        let fingerprint = format!(
            "fp-{}-{}",
            state.version,
            serde_json::to_string(change).unwrap_or_default().len()
        );
        PlanView {
            steps,
            warnings: Vec::new(),
            requires_confirmation: host == "www.xyz.com",
            fingerprint,
        }
    }
}

fn account() -> Account {
    Account {
        id: "acc".into(),
        name: "Personal".into(),
        credential: CredentialKind::ApiToken,
        limited_zone: None,
    }
}

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, T> {
    Box::pin(async move { value })
}

impl Backend for FakeBackend {
    fn machine_name(&self) -> String {
        "Mac".into()
    }

    fn accounts(&self) -> BoxFuture<'_, BackendResult<Vec<Account>>> {
        ready(Ok(vec![account()]))
    }

    fn capabilities<'a>(&'a self, _account: &'a str) -> BoxFuture<'a, BackendResult<Value>> {
        ready(Ok(json!({ "tunnelsEdit": "yes" })))
    }

    fn domains<'a>(&'a self, _account: &'a str) -> BoxFuture<'a, BackendResult<Value>> {
        ready(Ok(
            json!([{ "id": "z1", "name": "xyz.com", "status": "active", "plan": "Free", "paused": false }]),
        ))
    }

    fn overview<'a>(&'a self, _account: &'a str) -> BoxFuture<'a, BackendResult<RoutesOverview>> {
        let tunnel = TunnelView {
            id: "t1".into(),
            name: "Mac".into(),
            connector: Some(ConnectorState::Healthy { connections: 4 }),
            is_default: true,
        };
        ready(Ok(RoutesOverview {
            tunnel: Some(tunnel.clone()),
            tunnels: vec![tunnel],
            routes: self.lock().routes.clone(),
            zones: Vec::new(),
            networks: None,
        }))
    }

    fn tunnels<'a>(
        &'a self,
        _account: &'a str,
    ) -> BoxFuture<'a, BackendResult<Vec<TunnelSummary>>> {
        ready(Ok(vec![TunnelSummary {
            id: "t1".into(),
            name: "Mac".into(),
            status: "healthy".into(),
            created_at: String::new(),
            routes: Some(1),
            connectors: Vec::new(),
            this_mac: true,
            is_default: true,
            connector: Some(ConnectorState::Healthy { connections: 4 }),
        }]))
    }

    fn preview<'a>(
        &'a self,
        _target: &'a Target,
        change: &'a Change,
    ) -> BoxFuture<'a, BackendResult<PlanView>> {
        ready(Ok(self.view(change)))
    }

    fn apply<'a>(
        &'a self,
        _target: &'a Target,
        change: &'a Change,
        approval: ApplyApproval,
        actor: Option<Actor>,
        mut progress: ProgressSink,
    ) -> BoxFuture<'a, BackendResult<Outcome>> {
        Box::pin(async move {
            let view = self.view(change);
            if view.fingerprint != approval.fingerprint {
                return Err(BackendError::Stale(Box::new(view)));
            }
            if view.requires_confirmation && !approval.confirmed {
                return Err(BackendError::NeedsConfirmation);
            }
            for step in 0..u32::try_from(view.steps.len()).unwrap_or(0) {
                progress(Progress {
                    step,
                    state: StepState::Done,
                });
            }
            let host = Self::hostname(change).unwrap_or_default();
            let mut state = self.lock();
            let (kind, before, after) = match change {
                Change::AddRoute { route: input } => {
                    state.routes.push(route(&input.hostname, &input.origin));
                    (ActivityKind::AddRoute, None, Some(input.origin.clone()))
                }
                Change::RemoveRoute { hostname, .. } => {
                    let before = state
                        .routes
                        .iter()
                        .find(|r| &r.hostname == hostname)
                        .map(|r| r.origin.clone());
                    state.routes.retain(|r| &r.hostname != hostname);
                    (ActivityKind::RemoveRoute, before, None)
                }
                _ => (ActivityKind::UpdateRoute, None, None),
            };
            let id = i64::try_from(state.activity.len()).unwrap_or(0) + 1;
            state.activity.insert(
                0,
                ActivityEntry {
                    id,
                    at: id * 1000,
                    summary: format!("{kind:?} {host}"),
                    outcome: "applied".into(),
                    detail: Vec::new(),
                    record: Some(ActivityRecord {
                        kind,
                        hostnames: vec![host.clone()],
                        tunnel: "Mac".into(),
                        steps: Vec::new(),
                        changes: vec![Delta {
                            area: DeltaArea::Route,
                            hostname: host.clone(),
                            path: None,
                            before: before.map(msg::raw),
                            after: after.map(msg::raw),
                        }],
                        summary: None,
                        error: None,
                        leftovers: Vec::new(),
                        connector_error: None,
                        actor: actor.clone(),
                    }),
                },
            );
            state.applied.push((change.clone(), actor));
            let _ = self.changes.send(ChangeEvent::Routes);
            Ok(Outcome::Applied {
                tunnel_id: Some("t1".into()),
                verify: vec![host],
                connector_error: None,
            })
        })
    }

    fn verify<'a>(
        &'a self,
        _account: &'a str,
        hostname: &'a str,
        _patience: Duration,
    ) -> BoxFuture<'a, BackendResult<Verification>> {
        let exists = self.lock().routes.iter().any(|r| r.hostname == hostname);
        let failure = (!exists).then_some(teitunnel_core::engine::Failure::NoRecord);
        ready(Ok(Verification {
            hostname: hostname.to_owned(),
            status: exists.then_some(200),
            message: failure.as_ref().map(|_| msg::raw("No DNS record")),
            failure,
            protected: false,
            event_stream: false,
            transient: false,
        }))
    }

    fn doctor(&self) -> BoxFuture<'_, BackendResult<Vec<Issue>>> {
        ready(Ok(vec![
            Issue {
                id: "dns.orphan:acc:old.xyz.com".into(),
                check: "dns.orphan".into(),
                severity: Severity::Warning,
                account_id: Some("acc".into()),
                subject: "old.xyz.com".into(),
                label: msg::raw("old.xyz.com"),
                title: msg::raw("old.xyz.com points to a deleted tunnel"),
                detail: msg::raw("Anyone could take it over."),
                evidence: vec![msg::raw("CNAME to gone.cfargotunnel.com")],
                fixes: vec![Fix::Change {
                    label: msg::raw("Delete the record"),
                    change: Change::RemoveRoute {
                        hostname: "old.xyz.com".into(),
                        path: None,
                    },
                }],
                tunnel_id: None,
            },
            Issue {
                id: "connector.stopped:acc:Mac".into(),
                check: "connector.stopped".into(),
                severity: Severity::Error,
                account_id: Some("acc".into()),
                subject: "Mac".into(),
                label: msg::raw("Mac"),
                title: msg::raw("The connector isn't running"),
                detail: msg::raw("Routes don't answer."),
                evidence: Vec::new(),
                fixes: vec![Fix::StartConnector {
                    account_id: "acc".into(),
                }],
                tunnel_id: None,
            },
        ]))
    }

    fn run_fix<'a>(
        &'a self,
        issue: &'a Issue,
        _fix: &'a Fix,
        _actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<String>> {
        self.lock().fixes_run.push(issue.id.clone());
        ready(Ok("Started this machine's connector.".into()))
    }

    fn start_quick_share<'a>(
        &'a self,
        origin: &'a str,
        stop_after: Option<Duration>,
    ) -> BoxFuture<'a, BackendResult<ShareInfo>> {
        let share = ShareInfo {
            id: format!("qs-{}", self.lock().shares.len() + 1),
            kind: ShareKind::Quick,
            url: Some("https://calm-river-1234.trycloudflare.com".into()),
            origin: format!("http://localhost:{}", origin.trim()),
            status: "live".into(),
            started_by: "this agent".into(),
            mine: true,
            account_id: None,
            started_at: 1,
            expires_at: stop_after.map(|d| 1 + u64::try_from(d.as_millis()).unwrap_or(0)),
            paused: false,
        };
        self.lock().shares.push(share.clone());
        let _ = self.changes.send(ChangeEvent::Shares);
        ready(Ok(share))
    }

    fn start_domain_share(
        &self,
        request: DomainShareRequest,
        actor: Option<Actor>,
    ) -> BoxFuture<'_, BackendResult<(ShareInfo, Outcome)>> {
        let share = ShareInfo {
            id: request.hostname.clone(),
            kind: ShareKind::Domain,
            url: Some(format!("https://{}", request.hostname)),
            origin: request.origin.clone(),
            status: "live".into(),
            started_by: "this agent".into(),
            mine: true,
            account_id: Some(request.account.clone()),
            started_at: 1,
            expires_at: None,
            paused: false,
        };
        let mut state = self.lock();
        state.shares.push(share.clone());
        state.applied.push((
            Change::AddRoute {
                route: teitunnel_core::engine::RouteInput {
                    hostname: request.hostname,
                    path: None,
                    origin: request.origin,
                    access: request.access,
                    options: None,
                },
            },
            actor,
        ));
        ready(Ok((
            share,
            Outcome::Applied {
                tunnel_id: Some("t1".into()),
                verify: Vec::new(),
                connector_error: None,
            },
        )))
    }

    fn shares(&self) -> BoxFuture<'_, BackendResult<Vec<ShareInfo>>> {
        ready(Ok(self.lock().shares.clone()))
    }

    fn stop_share<'a>(
        &'a self,
        share: &'a ShareInfo,
        _actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<()>> {
        self.lock().shares.retain(|s| s.id != share.id);
        ready(Ok(()))
    }

    fn services(&self) -> BoxFuture<'_, BackendResult<Vec<LocalService>>> {
        ready(Ok(vec![
            LocalService {
                port: 5173,
                all_interfaces: false,
                pid: 42,
                process: "node".into(),
                kind: ServiceKind::Vite,
                project: Some("my-app".into()),
                folder: None,
                origin: "http://localhost:5173".into(),
            },
            LocalService {
                port: 5432,
                all_interfaces: false,
                pid: 43,
                process: "postgres".into(),
                kind: ServiceKind::Database,
                project: None,
                folder: None,
                origin: "tcp://localhost:5432".into(),
            },
        ]))
    }

    fn export<'a>(
        &'a self,
        _account: &'a str,
        _tunnel: Option<&'a str>,
        _format: ExportFormat,
    ) -> BoxFuture<'a, BackendResult<Option<ExportFile>>> {
        ready(Ok(Some(ExportFile {
            file_name: "compose.yaml".into(),
            contents: "services:\n  tunnel:\n    image: cloudflare/cloudflared\n".into(),
        })))
    }

    fn import_scan(&self) -> BoxFuture<'_, BackendResult<Vec<LocalSetup>>> {
        ready(Ok(Vec::new()))
    }

    fn logs<'a>(
        &'a self,
        _account: &'a str,
        _tunnel: Option<&'a str>,
        _route: Option<(&'a str, Option<&'a str>)>,
        _limit: usize,
    ) -> BoxFuture<'a, BackendResult<LogBatch>> {
        let line = |level: &str, message: &str| LogLine {
            time: None,
            level: level.into(),
            message: message.into(),
            error: None,
            fields: serde_json::Map::new(),
        };
        ready(Ok(LogBatch {
            source: "test".into(),
            lines: vec![
                line("info", "Registered tunnel connection"),
                line(
                    "error",
                    "Unable to reach the origin service with token=abc123secret",
                ),
            ],
            note: None,
        }))
    }

    fn remote_logs<'a>(
        &'a self,
        _account: &'a str,
        _tunnel: &'a str,
        _connector: &'a str,
        _limit: usize,
    ) -> BoxFuture<'a, BackendResult<RemoteLogBatch>> {
        ready(Ok(RemoteLogBatch {
            state: RemoteLogState::Ended {
                message: msg::raw("The connector disconnected."),
            },
            lines: Vec::new(),
        }))
    }

    fn activity<'a>(
        &'a self,
        _account: &'a str,
        limit: u32,
    ) -> BoxFuture<'a, BackendResult<Vec<ActivityEntry>>> {
        let entries = self
            .lock()
            .activity
            .iter()
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .cloned()
            .collect();
        ready(Ok(entries))
    }

    fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.changes.subscribe()
    }

    fn stop_own_shares(&self) -> BoxFuture<'_, usize> {
        let mut state = self.lock();
        let before = state.shares.len();
        state.shares.retain(|s| !s.mine);
        ready(before - state.shares.len())
    }

    fn set_paused<'a>(
        &'a self,
        _account: &'a str,
        hostname: &'a str,
        paused: bool,
    ) -> BoxFuture<'a, BackendResult<()>> {
        self.lock().paused.push((hostname.to_owned(), paused));
        ready(Ok(()))
    }

    fn set_quick_paused<'a>(
        &'a self,
        id: &'a str,
        paused: bool,
    ) -> BoxFuture<'a, BackendResult<()>> {
        self.lock().quick_paused.push((id.to_owned(), paused));
        ready(Ok(()))
    }

    fn set_schedule<'a>(
        &'a self,
        _account: &'a str,
        hostname: &'a str,
        schedule: Option<teitunnel_core::schedule::Schedule>,
    ) -> BoxFuture<'a, BackendResult<()>> {
        self.lock().schedules.push((hostname.to_owned(), schedule));
        ready(Ok(()))
    }

    fn share_folder(
        &self,
        folder: teitunnel_core::folder_share::FolderShare,
        domain: Option<(String, String)>,
        _expires_in: Option<Duration>,
        _actor: Option<Actor>,
    ) -> BoxFuture<'_, BackendResult<ShareInfo>> {
        let share = ShareInfo {
            id: domain
                .as_ref()
                .map_or_else(|| "qs-folder".to_owned(), |(_, h)| h.clone()),
            kind: if domain.is_some() {
                ShareKind::Domain
            } else {
                ShareKind::Quick
            },
            url: Some(domain.as_ref().map_or_else(
                || "https://calm-river-1234.trycloudflare.com".to_owned(),
                |(_, h)| format!("https://{h}"),
            )),
            origin: folder.path.clone(),
            status: "live".into(),
            started_by: "this agent".into(),
            mine: true,
            account_id: domain.map(|(a, _)| a),
            started_at: 1,
            expires_at: None,
            paused: false,
        };
        let mut state = self.lock();
        state.folders.push(folder);
        state.shares.push(share.clone());
        ready(Ok(share))
    }

    fn protection<'a>(
        &'a self,
        _account: &'a str,
        hostname: &'a str,
    ) -> BoxFuture<'a, BackendResult<teitunnel_core::protection::ProtectionView>> {
        use teitunnel_core::{engine::edge::ZonePlan, protection::ProtectionView};
        ready(Ok(ProtectionView {
            hostname: hostname.into(),
            zone: "xyz.com".into(),
            plan: ZonePlan::Pro,
            protection: self.lock().protection.clone(),
            quotas: Vec::new(),
            rate_limit_available: true,
            longest_period: 60,
            shares_rate_limit_with: Vec::new(),
        }))
    }

    fn service_tokens<'a>(
        &'a self,
        _account: &'a str,
        _hostname: &'a str,
    ) -> BoxFuture<'a, BackendResult<Vec<teitunnel_core::protection::ServiceTokenView>>> {
        ready(Ok(self.lock().tokens.clone()))
    }

    fn preview_front<'a>(
        &'a self,
        _account: &'a str,
        change: &'a teitunnel_core::fronts::FrontChange,
    ) -> BoxFuture<'a, BackendResult<PlanView>> {
        ready(Ok(PlanView {
            steps: vec![StepView {
                kind: StepKind::ServiceToken,
                description: msg::raw(format!("Put a Worker in front of {}", change.hostname())),
                command: None,
            }],
            warnings: Vec::new(),
            requires_confirmation: false,
            fingerprint: "fp-front".into(),
        }))
    }

    fn apply_front<'a>(
        &'a self,
        _account: &'a str,
        change: &'a teitunnel_core::fronts::FrontChange,
        approval: ApplyApproval,
        _actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<Outcome>> {
        assert_eq!(approval.fingerprint, "fp-front");
        self.lock().fronts_applied.push(change.clone());
        ready(Ok(Outcome::Applied {
            tunnel_id: None,
            verify: Vec::new(),
            connector_error: None,
        }))
    }

    fn preview_protection<'a>(
        &'a self,
        _account: &'a str,
        change: &'a teitunnel_core::protection::ProtectionChange,
    ) -> BoxFuture<'a, BackendResult<PlanView>> {
        ready(Ok(PlanView {
            steps: vec![StepView {
                kind: StepKind::ServiceToken,
                description: msg::raw(format!("Change tokens of {}", change.hostname())),
                command: None,
            }],
            warnings: Vec::new(),
            requires_confirmation: false,
            fingerprint: format!("fp-token-{}", self.lock().tokens.len()),
        }))
    }

    fn apply_protection<'a>(
        &'a self,
        _account: &'a str,
        change: &'a teitunnel_core::protection::ProtectionChange,
        approval: ApplyApproval,
        _actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<(Outcome, Vec<teitunnel_core::engine::edge::IssuedToken>)>>
    {
        use teitunnel_core::protection::{ProtectionChange, ServiceTokenView};
        Box::pin(async move {
            let mut state = self.lock();
            if approval.fingerprint != format!("fp-token-{}", state.tokens.len()) {
                return Err(BackendError::message("stale"));
            }
            state.protection_applied.push(change.clone());
            let mut issued = Vec::new();
            match change {
                ProtectionChange::CreateToken { label, .. } => {
                    state.tokens.push(ServiceTokenView {
                        id: "tok1".into(),
                        label: label.clone(),
                        client_id: "tok1.access".into(),
                        expires_at: None,
                        gone: false,
                    });
                    issued.push(teitunnel_core::engine::edge::IssuedToken {
                        token_id: "tok1".into(),
                        name: label.clone(),
                        client_id: "tok1.access".into(),
                        client_secret: teitunnel_core::Secret::new("0123456789abcdef".into()),
                        expires_at: None,
                    });
                }
                ProtectionChange::RevokeToken { token_id, .. } => {
                    state.tokens.retain(|t| &t.id != token_id);
                }
                _ => {}
            }
            Ok((
                Outcome::Applied {
                    tunnel_id: None,
                    verify: Vec::new(),
                    connector_error: None,
                },
                issued,
            ))
        })
    }

    fn comment_subjects(
        &self,
    ) -> BoxFuture<'_, BackendResult<Vec<teitunnel_core::comments::SubjectView>>> {
        let state = self.lock();
        let open = state.threads.iter().filter(|t| !t.resolved).count();
        let comments = state
            .threads
            .iter()
            .map(|t| t.comments.len())
            .sum::<usize>();
        ready(Ok(vec![teitunnel_core::comments::SubjectView {
            subject: teitunnel_core::comments::Subject::snapshot(
                "s1",
                "acc",
                "Launch",
                "https://preview.xyz.com",
            ),
            open: u32::try_from(open).unwrap_or(u32::MAX),
            comments: u32::try_from(comments).unwrap_or(u32::MAX),
            unread: 0,
            latest_at: None,
        }]))
    }

    fn comment_threads<'a>(
        &'a self,
        _key: &'a str,
    ) -> BoxFuture<'a, BackendResult<Vec<teitunnel_core::comments::Thread>>> {
        ready(Ok(self.lock().threads.clone()))
    }

    fn comment_reply<'a>(
        &'a self,
        _key: &'a str,
        thread: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, BackendResult<teitunnel_core::comments::Thread>> {
        let mut state = self.lock();
        let result = state
            .threads
            .iter_mut()
            .find(|t| t.id == thread)
            .map(|t| {
                t.comments.push(teitunnel_core::comments::Comment {
                    id: "r1".into(),
                    author: "Me".into(),
                    email: None,
                    verified: false,
                    by_owner: true,
                    body: body.into(),
                    created_at: 2,
                });
                t.clone()
            })
            .ok_or_else(|| BackendError::NotFound("no thread".into()));
        ready(result)
    }

    fn comment_resolve<'a>(
        &'a self,
        _key: &'a str,
        thread: &'a str,
        resolved: bool,
    ) -> BoxFuture<'a, BackendResult<teitunnel_core::comments::Thread>> {
        let mut state = self.lock();
        let result = state
            .threads
            .iter_mut()
            .find(|t| t.id == thread)
            .map(|t| {
                t.resolved = resolved;
                t.clone()
            })
            .ok_or_else(|| BackendError::NotFound("no thread".into()));
        ready(result)
    }
}

/// Captured traffic in memory.
#[derive(Debug, Default)]
pub(crate) struct FakeTraffic {
    pub(crate) exchanges: Mutex<Vec<Exchange>>,
    pub(crate) arrived: tokio::sync::Notify,
}

pub(crate) fn exchange(id: &str, started_at_ms: u64, path: &str) -> Exchange {
    Exchange {
        summary: ExchangeSummary {
            id: id.into(),
            scope: "demo.xyz.com".into(),
            started_at_ms,
            method: "POST".into(),
            host: "demo.xyz.com".into(),
            path: path.into(),
            status: Some(500),
            duration_ms: Some(12),
            request_bytes: 2,
            response_bytes: 4,
            state: "complete".into(),
        },
        request: HttpMessage {
            headers: vec![
                ("Authorization".into(), "Bearer sk_live_123".into()),
                ("Stripe-Signature".into(), "t=1,v1=deadbeef".into()),
                ("Content-Type".into(), "application/json".into()),
            ],
            body: Body {
                encoding: "utf8".into(),
                data: "{}".into(),
                size: 2,
                truncated: false,
            },
        },
        response: Some(HttpMessage {
            headers: vec![("Set-Cookie".into(), "session=abc".into())],
            body: Body {
                encoding: "utf8".into(),
                data: "oops".into(),
                size: 4,
                truncated: false,
            },
        }),
        ttfb_ms: Some(10),
        error: None,
    }
}

impl FakeTraffic {
    pub(crate) fn push(&self, exchange: Exchange) {
        self.exchanges
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(exchange);
        self.arrived.notify_waiters();
    }

    fn matching(&self, filter: &TrafficFilter) -> Vec<Exchange> {
        let mut list: Vec<Exchange> = self
            .exchanges
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|e| filter.matches_summary(&e.summary))
            .cloned()
            .collect();
        list.reverse();
        list
    }
}

impl TrafficSource for FakeTraffic {
    fn list<'a>(
        &'a self,
        filter: &'a TrafficFilter,
        _cursor: Option<&'a str>,
        limit: usize,
    ) -> BoxFuture<'a, Result<TrafficPage, TrafficError>> {
        let exchanges = self
            .matching(filter)
            .into_iter()
            .take(limit)
            .map(|e| e.summary)
            .collect();
        ready(Ok(TrafficPage {
            exchanges,
            next_cursor: None,
        }))
    }

    fn get<'a>(
        &'a self,
        id: &'a str,
        _body_limit: usize,
    ) -> BoxFuture<'a, Result<Exchange, TrafficError>> {
        let found = self
            .exchanges
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .find(|e| e.summary.id == id)
            .cloned();
        ready(found.ok_or_else(|| TrafficError::NotFound(id.to_owned())))
    }

    fn replay<'a>(
        &'a self,
        id: &'a str,
        _edits: &'a ReplayEdits,
        times: u32,
    ) -> BoxFuture<'a, Result<Vec<ReplayResult>, TrafficError>> {
        Box::pin(async move {
            let original = self.get(id, 0).await?;
            let mut out = Vec::new();
            for n in 0..times {
                let mut replay = original.clone();
                replay.summary.id = format!("{id}-replay-{n}");
                replay.summary.status = Some(200);
                self.push(replay.clone());
                out.push(ReplayResult {
                    exchange: replay.summary,
                });
            }
            Ok(out)
        })
    }

    fn next_matching<'a>(
        &'a self,
        filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<ExchangeSummary, TrafficError>> {
        Box::pin(async move {
            loop {
                let arrived = self.arrived.notified();
                if let Some(found) = self.matching(filter).pop() {
                    return Ok(found.summary);
                }
                arrived.await;
            }
        })
    }

    fn stats<'a>(
        &'a self,
        filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<TrafficStats, TrafficError>> {
        let count = self.matching(filter).len() as u64;
        ready(Ok(TrafficStats {
            count,
            ..TrafficStats::default()
        }))
    }
}

pub(crate) fn settings(mode: Mode) -> Settings {
    Settings {
        mode,
        allow_secrets: false,
    }
}

pub(crate) fn actor() -> Actor {
    Actor {
        via: "mcp".into(),
        client: "test-agent".into(),
        version: None,
    }
}

struct Harness {
    backend: Arc<FakeBackend>,
    traffic: Arc<FakeTraffic>,
    tools: CoreTools,
}

fn harness() -> Harness {
    let backend = FakeBackend::new();
    let traffic = Arc::new(FakeTraffic::default());
    let shared: SharedBackend = backend.clone();
    let tools = CoreTools::new(shared, traffic.clone(), Arc::new(Plans::default()));
    Harness {
        backend,
        traffic,
        tools,
    }
}

impl Harness {
    async fn call(&self, mode: Mode, name: &str, args: Value) -> Result<Value, String> {
        let ctx = ToolContext::detached(settings(mode), actor());
        let Value::Object(args) = args else {
            return Err("arguments must be an object".into());
        };
        self.tools
            .call(name, args, &ctx)
            .await
            .map(|out| crate::redaction::value(out.structured, false))
            .map_err(|e| e.message)
    }
}

#[test]
fn every_tool_is_listed_with_schemas_and_annotations() {
    let h = harness();
    let specs = h.tools.tools();
    let names: Vec<String> = specs.iter().map(|s| s.tool.name.to_string()).collect();
    for expected in [
        "share_port",
        "stop_share",
        "list_shares",
        "list_local_services",
        "list_routes",
        "list_domains",
        "list_tunnels",
        "plan_change",
        "apply_plan",
        "verify_route",
        "undo_last",
        "doctor",
        "fix_issue",
        "logs_tail",
        "remote_logs",
        "connector_status",
        "export_config",
        "import_scan",
        "accounts",
        "recent_activity",
        "traffic_list",
        "traffic_get",
        "traffic_replay",
        "wait_for_request",
        "traffic_stats",
        "traffic_export",
        "traffic_openapi",
        "pause_share",
        "resume_share",
        "schedule_share",
        "share_folder",
    ] {
        assert!(names.iter().any(|n| n == expected), "{expected} is missing");
    }
    for spec in &specs {
        let tool = &spec.tool;
        assert_eq!(
            tool.input_schema.get("type"),
            Some(&json!("object")),
            "{}",
            tool.name
        );
        assert!(
            tool.output_schema.is_some(),
            "{} has an output schema",
            tool.name
        );
        assert!(
            tool.description.as_ref().is_some_and(|d| d.len() > 80),
            "{} is described",
            tool.name
        );
        let hints = tool.annotations.as_ref().expect("annotated");
        assert!(hints.title.is_some());
        let read_only = hints.read_only_hint == Some(true);
        assert_eq!(
            read_only,
            spec.class.read_only(),
            "{}: readOnlyHint matches its class",
            tool.name
        );
        if spec.class == ToolClass::Destructive {
            assert_eq!(hints.destructive_hint, Some(true), "{}", tool.name);
        }
        // Names follow the spec's tool name rules.
        assert!(
            tool.name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        );
    }
    let schema = specs
        .iter()
        .find(|s| s.tool.name == "plan_change")
        .map(|s| serde_json::to_string(&s.tool.input_schema).unwrap())
        .unwrap();
    for change in [
        "addRoute",
        "removeRoute",
        "requireLogin",
        "addNetwork",
        "createTunnel",
        "importRoutes",
    ] {
        assert!(schema.contains(change), "plan_change knows {change}");
    }
}

#[tokio::test]
async fn plan_then_apply_in_full_mode_records_the_agent() {
    let h = harness();
    let plan = h
        .call(Mode::Full, "plan_change", json!({ "change": { "type": "addRoute", "hostname": "new.xyz.com", "origin": "4000" } }))
        .await
        .unwrap();
    let id = plan["plan"]["planId"].as_str().unwrap().to_owned();
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_owned();
    assert_eq!(plan["plan"]["steps"].as_array().unwrap().len(), 2);
    assert!(
        plan["plan"]["text"]
            .as_str()
            .unwrap()
            .contains("1. Update routes for new.xyz.com")
    );
    assert!(
        h.backend.lock().applied.is_empty(),
        "planning changes nothing"
    );

    // A wrong fingerprint is refused.
    let wrong = h
        .call(
            Mode::Full,
            "apply_plan",
            json!({ "planId": id, "fingerprint": "nope" }),
        )
        .await;
    assert!(wrong.unwrap_err().contains("fingerprint"));

    let applied = h
        .call(
            Mode::Full,
            "apply_plan",
            json!({ "planId": id, "fingerprint": fingerprint }),
        )
        .await
        .unwrap();
    assert_eq!(applied["outcome"], "applied", "{applied}");
    assert_eq!(applied["verification"][0]["ok"], true);
    assert!(applied["undo"].as_str().unwrap().contains("undo_last"));
    {
        let state = h.backend.lock();
        assert_eq!(state.applied.len(), 1);
        assert_eq!(state.applied[0].1.as_ref().unwrap().client, "test-agent");
    }

    // A plan applies once.
    let again = h
        .call(
            Mode::Full,
            "apply_plan",
            json!({ "planId": id, "fingerprint": fingerprint }),
        )
        .await;
    assert!(again.unwrap_err().contains("No plan"));
}

#[tokio::test]
async fn ask_mode_never_applies_without_approval() {
    let h = harness();
    let plan = h
        .call(
            Mode::Ask,
            "plan_change",
            json!({ "change": { "type": "removeRoute", "hostname": "app.xyz.com" } }),
        )
        .await
        .unwrap();
    assert!(
        plan["plan"]["approval"]
            .as_str()
            .unwrap()
            .contains("confirmed")
    );
    let (id, fp) = (
        plan["plan"]["planId"].clone(),
        plan["plan"]["fingerprint"].clone(),
    );
    let first = h
        .call(
            Mode::Ask,
            "apply_plan",
            json!({ "planId": id, "fingerprint": fp }),
        )
        .await
        .unwrap();
    assert_eq!(first["outcome"], "needsApproval");
    assert!(h.backend.lock().applied.is_empty());
    let second = h
        .call(
            Mode::Ask,
            "apply_plan",
            json!({ "planId": id, "fingerprint": fp, "confirmed": true }),
        )
        .await
        .unwrap();
    assert_eq!(second["outcome"], "applied");
    assert!(h.backend.lock().routes.is_empty());
}

#[tokio::test]
async fn read_only_mode_declines_changes() {
    let h = harness();
    let plan = h
        .call(
            Mode::ReadOnly,
            "plan_change",
            json!({ "change": { "type": "removeRoute", "hostname": "app.xyz.com" } }),
        )
        .await
        .unwrap();
    assert!(plan["next"].as_str().unwrap().contains("read-only"));
    let applied = h
        .call(Mode::ReadOnly, "apply_plan", json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"], "confirmed": true }))
        .await
        .unwrap();
    assert_eq!(applied["outcome"], "declined");
    assert_eq!(h.backend.lock().routes.len(), 1);
}

#[tokio::test]
async fn stale_plans_come_back_for_review() {
    let h = harness();
    let plan = h
        .call(
            Mode::Full,
            "plan_change",
            json!({ "change": { "type": "addRoute", "hostname": "b.xyz.com", "origin": "5000" } }),
        )
        .await
        .unwrap();
    h.backend.lock().version += 1; // Cloudflare changed meanwhile.
    let result = h
        .call(
            Mode::Full,
            "apply_plan",
            json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"] }),
        )
        .await
        .unwrap();
    assert_eq!(result["outcome"], "stale");
    let fresh = &result["newPlan"];
    assert_ne!(fresh["fingerprint"], plan["plan"]["fingerprint"]);
    assert!(h.backend.lock().applied.is_empty());
    let applied = h
        .call(
            Mode::Full,
            "apply_plan",
            json!({ "planId": fresh["planId"], "fingerprint": fresh["fingerprint"] }),
        )
        .await
        .unwrap();
    assert_eq!(applied["outcome"], "applied");
}

#[tokio::test]
async fn foreign_records_need_confirmation_even_in_full_mode() {
    let h = harness();
    let plan = h
        .call(
            Mode::Full,
            "plan_change",
            json!({ "change": { "type": "addRoute", "hostname": "www.xyz.com", "origin": "80" } }),
        )
        .await
        .unwrap();
    assert_eq!(plan["plan"]["requiresConfirmation"], true);
    let args =
        json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"] });
    let first = h
        .call(Mode::Full, "apply_plan", args.clone())
        .await
        .unwrap();
    assert_eq!(first["outcome"], "needsApproval");
    let mut confirmed = args;
    confirmed["confirmed"] = json!(true);
    assert_eq!(
        h.call(Mode::Full, "apply_plan", confirmed).await.unwrap()["outcome"],
        "applied"
    );
}

#[tokio::test]
async fn edits_keep_what_they_dont_mention() {
    let h = harness();
    h.backend.lock().routes[0].access = Some(teitunnel_core::engine::AccessRule {
        emails: vec!["me@xyz.com".into()],
        email_domains: Vec::new(),
    });
    h.call(
        Mode::Full,
        "plan_change",
        json!({ "change": { "type": "updateRoute", "hostname": "app.xyz.com", "origin": "3001" } }),
    )
    .await
    .unwrap();
    // The plan was made from a change that keeps the login.
    let plan = h
        .call(Mode::Full, "plan_change", json!({ "change": { "type": "requireLogin", "hostname": "app.xyz.com", "allow": ["@xyz.com"] } }))
        .await
        .unwrap();
    assert!(
        plan["plan"]["summary"]
            .as_str()
            .unwrap()
            .contains("@xyz.com")
    );
    let missing = h
        .call(
            Mode::Full,
            "plan_change",
            json!({ "change": { "type": "removeLogin", "hostname": "nope.xyz.com" } }),
        )
        .await;
    assert!(missing.unwrap_err().contains("no route"));
    let bad = h
        .call(
            Mode::Full,
            "plan_change",
            json!({ "change": { "type": "teleport" } }),
        )
        .await;
    assert!(bad.unwrap_err().contains("Invalid arguments"));
}

#[tokio::test]
async fn undo_plans_the_reverse_of_an_agents_change() {
    let h = harness();
    let nothing = h.call(Mode::Full, "undo_last", json!({})).await;
    assert!(nothing.unwrap_err().contains("No change made by an agent"));
    let plan = h
        .call(
            Mode::Full,
            "plan_change",
            json!({ "change": { "type": "addRoute", "hostname": "c.xyz.com", "origin": "6000" } }),
        )
        .await
        .unwrap();
    h.call(
        Mode::Full,
        "apply_plan",
        json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"] }),
    )
    .await
    .unwrap();
    let undo = h.call(Mode::Full, "undo_last", json!({})).await.unwrap();
    assert_eq!(undo["undoing"]["by"], "test-agent");
    assert!(undo["plan"]["text"].as_str().unwrap().contains("c.xyz.com"));
    h.call(
        Mode::Full,
        "apply_plan",
        json!({ "planId": undo["plan"]["planId"], "fingerprint": undo["plan"]["fingerprint"] }),
    )
    .await
    .unwrap();
    assert!(
        !h.backend
            .lock()
            .routes
            .iter()
            .any(|r| r.hostname == "c.xyz.com")
    );

    // The removal (an agent's too) is undone by adding the route back.
    let back = h.call(Mode::Full, "undo_last", json!({})).await.unwrap();
    assert!(
        back["plan"]["summary"]
            .as_str()
            .unwrap()
            .starts_with("Undo")
    );
    let activity = h
        .call(Mode::Full, "recent_activity", json!({ "agentsOnly": true }))
        .await
        .unwrap();
    assert_eq!(activity["entries"].as_array().unwrap().len(), 2);
    assert_eq!(activity["entries"][0]["by"], "test-agent");
}

#[tokio::test]
async fn shares_quickly_or_on_a_domain_with_approval() {
    let h = harness();
    let asked = h
        .call(Mode::Ask, "share_port", json!({ "target": "3000" }))
        .await
        .unwrap();
    assert_eq!(asked["outcome"], "needsApproval");
    assert!(
        h.backend.lock().shares.is_empty(),
        "nothing goes online unasked"
    );
    let shared = h
        .call(
            Mode::Ask,
            "share_port",
            json!({ "target": "3000", "expiresInMinutes": 30, "confirmed": true }),
        )
        .await
        .unwrap();
    assert_eq!(shared["outcome"], "shared");
    assert_eq!(
        shared["share"]["url"],
        "https://calm-river-1234.trycloudflare.com"
    );
    assert!(shared["share"]["expiresAt"].is_u64());

    let bad = h
        .call(
            Mode::Full,
            "share_port",
            json!({ "target": "3000", "allow": ["me@xyz.com"] }),
        )
        .await;
    assert!(bad.unwrap_err().contains("needs `hostname`"));
    let long = h
        .call(
            Mode::Full,
            "share_port",
            json!({ "target": "3000", "expiresInMinutes": 99999 }),
        )
        .await;
    assert!(long.is_err());

    let domain = h
        .call(
            Mode::Full,
            "share_port",
            json!({ "target": "8080", "hostname": "demo.xyz.com", "allow": ["@xyz.com"] }),
        )
        .await
        .unwrap();
    assert_eq!(domain["share"]["url"], "https://demo.xyz.com");
    let recorded = h.backend.lock().applied.last().cloned().unwrap();
    assert_eq!(recorded.1.unwrap().client, "test-agent");

    let listed = h.call(Mode::Ask, "list_shares", json!({})).await.unwrap();
    assert_eq!(listed["total"], 2);
    // Its own shares stop without asking.
    let stopped = h
        .call(
            Mode::Ask,
            "stop_share",
            json!({ "share": "https://calm-river-1234.trycloudflare.com/" }),
        )
        .await
        .unwrap();
    assert_eq!(stopped["outcome"], "stopped");
    let missing = h
        .call(Mode::Ask, "stop_share", json!({ "share": "nope" }))
        .await;
    assert!(missing.unwrap_err().contains("list_shares"));
}

#[tokio::test]
async fn stopping_the_persons_share_needs_approval() {
    let h = harness();
    h.backend.lock().shares.push(ShareInfo {
        id: "theirs.xyz.com".into(),
        kind: ShareKind::Domain,
        url: Some("https://theirs.xyz.com".into()),
        origin: "3000".into(),
        status: "live".into(),
        started_by: "the app".into(),
        mine: false,
        account_id: Some("acc".into()),
        started_at: 1,
        expires_at: None,
        paused: false,
    });
    let asked = h
        .call(
            Mode::Ask,
            "stop_share",
            json!({ "share": "theirs.xyz.com" }),
        )
        .await
        .unwrap();
    assert_eq!(asked["outcome"], "needsApproval");
    assert_eq!(h.backend.lock().shares.len(), 1);
    let stopped = h
        .call(
            Mode::Full,
            "stop_share",
            json!({ "share": "theirs.xyz.com" }),
        )
        .await
        .unwrap();
    assert_eq!(stopped["outcome"], "stopped");
}

#[tokio::test]
async fn reads_routes_domains_tunnels_services_and_accounts() {
    let h = harness();
    let routes = h.call(Mode::Ask, "list_routes", json!({})).await.unwrap();
    assert_eq!(routes["routes"][0]["hostname"], "app.xyz.com");
    assert_eq!(routes["routes"][0]["status"], "live");
    assert_eq!(routes["routes"][0]["url"], "https://app.xyz.com");
    assert_eq!(routes["tunnels"][0]["connector"], "healthy (4 connections)");
    let down = h
        .call(Mode::Ask, "list_routes", json!({ "status": "down" }))
        .await
        .unwrap();
    assert_eq!(down["total"], 0);
    let domains = h.call(Mode::Ask, "list_domains", json!({})).await.unwrap();
    assert_eq!(domains["domains"][0]["name"], "xyz.com");
    let tunnels = h
        .call(Mode::Ask, "list_tunnels", json!({ "account": "personal" }))
        .await
        .unwrap();
    assert_eq!(tunnels["tunnels"][0]["thisMachine"], true);
    let unknown = h
        .call(Mode::Ask, "list_tunnels", json!({ "account": "work" }))
        .await;
    assert!(unknown.unwrap_err().contains("No connected account"));
    let services = h
        .call(Mode::Ask, "list_local_services", json!({ "kind": "vite" }))
        .await
        .unwrap();
    assert_eq!(services["total"], 1);
    assert_eq!(services["services"][0]["origin"], "http://localhost:5173");
    let accounts = h
        .call(Mode::Ask, "accounts", json!({ "capabilities": true }))
        .await
        .unwrap();
    assert_eq!(accounts["accounts"][0]["credential"], "apiToken");
    assert!(accounts["accounts"][0]["capabilities"].is_object());
    let status = h
        .call(Mode::Ask, "connector_status", json!({}))
        .await
        .unwrap();
    assert_eq!(status["machine"], "Mac");
    let export = h
        .call(
            Mode::Ask,
            "export_config",
            json!({ "format": "dockerCompose" }),
        )
        .await
        .unwrap();
    assert_eq!(export["fileName"], "compose.yaml");
    let setups = h.call(Mode::Ask, "import_scan", json!({})).await.unwrap();
    assert!(setups["setups"].as_array().unwrap().is_empty());
    let verify = h
        .call(
            Mode::Ask,
            "verify_route",
            json!({ "hostname": "gone.xyz.com" }),
        )
        .await
        .unwrap();
    assert_eq!(verify["ok"], false);
    assert_eq!(verify["failure"], "noRecord");
}

#[tokio::test]
async fn doctor_and_fixes() {
    let h = harness();
    let doctor = h.call(Mode::Ask, "doctor", json!({})).await.unwrap();
    assert_eq!(doctor["total"], 2);
    assert_eq!(doctor["errors"], 1);
    let errors = h
        .call(Mode::Ask, "doctor", json!({ "severity": "error" }))
        .await
        .unwrap();
    assert_eq!(errors["total"], 1);
    assert!(
        h.call(Mode::Ask, "doctor", json!({ "severity": "loud" }))
            .await
            .is_err()
    );

    // A Cloudflare fix that isn't "safe" comes back as a plan.
    let planned = h
        .call(
            Mode::Full,
            "fix_issue",
            json!({ "issueId": "dns.orphan:acc:old.xyz.com" }),
        )
        .await
        .unwrap();
    assert_eq!(planned["outcome"], "planned");
    assert!(planned["plan"]["planId"].is_string());

    // A fix outside Cloudflare runs after approval.
    let asked = h
        .call(
            Mode::Ask,
            "fix_issue",
            json!({ "issueId": "connector.stopped:acc:Mac" }),
        )
        .await
        .unwrap();
    assert_eq!(asked["outcome"], "needsApproval");
    let done = h
        .call(
            Mode::Ask,
            "fix_issue",
            json!({ "issueId": "connector.stopped:acc:Mac", "confirmed": true }),
        )
        .await
        .unwrap();
    assert_eq!(done["outcome"], "done");
    assert_eq!(h.backend.lock().fixes_run, ["connector.stopped:acc:Mac"]);
    assert!(
        h.call(Mode::Ask, "fix_issue", json!({ "issueId": "gone" }))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn logs_are_redacted_and_filtered() {
    let h = harness();
    let logs = h
        .call(Mode::Ask, "logs_tail", json!({ "level": "error" }))
        .await
        .unwrap();
    let lines = logs["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1);
    let text = lines[0]["message"].as_str().unwrap();
    assert!(
        text.contains("token=[redacted]") && !text.contains("abc123secret"),
        "{text}"
    );
    let remote = h
        .call(
            Mode::Ask,
            "remote_logs",
            json!({ "tunnel": "Mac", "seconds": 1 }),
        )
        .await;
    assert!(
        remote.unwrap_err().contains("No machine runs"),
        "no connectors to stream"
    );
}

#[tokio::test]
async fn traffic_is_masked_and_waited_for() {
    let h = harness();
    h.traffic.push(exchange("ex-1", 1_000, "/webhooks/stripe"));
    let listed = h
        .call(Mode::Ask, "traffic_list", json!({ "status": "5xx" }))
        .await
        .unwrap();
    assert_eq!(listed["exchanges"][0]["id"], "ex-1");
    let got = h
        .call(Mode::Ask, "traffic_get", json!({ "id": "ex-1" }))
        .await
        .unwrap();
    let headers = got["request"]["headers"].as_array().unwrap();
    assert_eq!(headers[0][1], "[masked]", "Authorization");
    assert_eq!(headers[1][1], "[masked]", "webhook signature");
    assert_eq!(headers[2][1], "application/json");
    assert_eq!(got["response"]["headers"][0][1], "[masked]", "Set-Cookie");

    // Waiting: a request that arrives while waiting is returned in full.
    let traffic = h.traffic.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        traffic.push(exchange("ex-2", 5_000_000_000_000, "/webhooks/github"));
    });
    let waited = h
        .call(
            Mode::Ask,
            "wait_for_request",
            json!({ "pathContains": "/webhooks/github", "timeoutSeconds": 5, "sinceMs": 2_000 }),
        )
        .await
        .unwrap();
    assert_eq!(waited["outcome"], "received", "{waited}");
    assert_eq!(waited["details"]["request"]["headers"][0][1], "[masked]");
    let timeout = h
        .call(
            Mode::Ask,
            "wait_for_request",
            json!({ "pathContains": "/never", "timeoutSeconds": 1 }),
        )
        .await
        .unwrap();
    assert_eq!(timeout["outcome"], "timeout");

    let replay = h
        .call(Mode::Ask, "traffic_replay", json!({ "id": "ex-1" }))
        .await
        .unwrap();
    assert_eq!(replay["outcome"], "needsApproval");
    let replay = h
        .call(
            Mode::Full,
            "traffic_replay",
            json!({ "id": "ex-1", "times": 2 }),
        )
        .await
        .unwrap();
    assert_eq!(replay["replays"].as_array().unwrap().len(), 2);
    let stats = h.call(Mode::Ask, "traffic_stats", json!({})).await.unwrap();
    assert_eq!(stats["count"], 4);
    let curl = h
        .call(
            Mode::Ask,
            "traffic_export",
            json!({ "ids": ["ex-1"], "format": "curl" }),
        )
        .await
        .unwrap();
    let text = curl["contents"].as_str().unwrap();
    assert!(
        text.contains("curl -X POST") && !text.contains("sk_live_123"),
        "{text}"
    );
    assert!(
        h.call(
            Mode::Ask,
            "traffic_export",
            json!({ "ids": [], "format": "har" })
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn an_agent_pauses_its_own_quick_share_without_asking() {
    let h = harness();
    let shared = h
        .call(Mode::Full, "share_port", json!({ "target": "3000" }))
        .await
        .unwrap();
    let url = shared["share"]["url"].as_str().unwrap().to_owned();
    let paused = h
        .call(Mode::Ask, "pause_share", json!({ "share": url }))
        .await
        .unwrap();
    assert_eq!(paused["outcome"], "paused", "{paused}");
    let id = shared["share"]["id"].as_str().unwrap().to_owned();
    assert_eq!(h.backend.lock().quick_paused, [(id, true)]);
    assert!(
        h.backend.lock().paused.is_empty(),
        "not treated as a domain"
    );
}

#[tokio::test]
async fn an_offline_page_is_shown_to_the_person_before_it_goes_up() {
    let h = harness();
    let shared: SharedBackend = h.backend.clone();
    let fronts = super::FrontTools::new(shared);
    let call = |args: Value| {
        let ctx = ToolContext::detached(settings(Mode::Ask), actor());
        let Value::Object(args) = args else {
            panic!("an object")
        };
        let fronts = &fronts;
        async move {
            fronts
                .call("set_offline_page", args, &ctx)
                .await
                .map(|out| out.structured)
                .unwrap()
        }
    };
    let args = json!({ "hostname": "demo.xyz.com", "title": "Back at 5" });
    let asked = call(args).await;
    assert_eq!(asked["outcome"], "needsApproval");
    assert!(
        h.backend.lock().fronts_applied.is_empty(),
        "nothing unasked"
    );
    let args = json!({ "hostname": "demo.xyz.com", "title": "Back at 5", "confirmed": true });
    let done = call(args).await;
    assert_eq!(done["outcome"], "applied", "{done}");
    let applied = h.backend.lock().fronts_applied.clone();
    let [
        teitunnel_core::fronts::FrontChange::Offline {
            page: Some(page), ..
        },
    ] = applied.as_slice()
    else {
        panic!("{applied:?}");
    };
    assert_eq!(page.title, "Back at 5");
}

#[tokio::test]
async fn pausing_and_scheduling_ask_first() {
    let h = harness();
    // Ask mode without a client that can ask: the agent must show the person first.
    let asked = h
        .call(
            Mode::Ask,
            "pause_share",
            json!({ "share": "https://Demo.xyz.com/" }),
        )
        .await
        .unwrap();
    assert_eq!(asked["outcome"], "needsApproval");
    assert!(h.backend.lock().paused.is_empty());
    let paused = h
        .call(
            Mode::Ask,
            "pause_share",
            json!({ "share": "https://Demo.xyz.com/", "confirmed": true }),
        )
        .await
        .unwrap();
    assert_eq!(paused["outcome"], "paused");
    h.call(
        Mode::Full,
        "resume_share",
        json!({ "share": "demo.xyz.com" }),
    )
    .await
    .unwrap();
    assert_eq!(
        h.backend.lock().paused,
        [
            ("demo.xyz.com".to_owned(), true),
            ("demo.xyz.com".to_owned(), false)
        ]
    );

    let scheduled = h
        .call(
            Mode::Full,
            "schedule_share",
            json!({ "share": "demo.xyz.com", "days": "mon-fri", "from": "9:00", "to": "18:00", "timeZone": "Europe/Berlin" }),
        )
        .await
        .unwrap();
    assert_eq!(scheduled["outcome"], "scheduled");
    assert!(scheduled["message"].as_str().unwrap().contains("09:00"));
    let bad = h
        .call(
            Mode::Full,
            "schedule_share",
            json!({ "share": "demo.xyz.com", "days": "someday", "from": "9:00", "to": "18:00" }),
        )
        .await;
    assert!(bad.is_err());
    let missing = h
        .call(
            Mode::Full,
            "schedule_share",
            json!({ "share": "demo.xyz.com" }),
        )
        .await;
    assert!(missing.unwrap_err().contains("off: true"));
    h.call(
        Mode::Full,
        "schedule_share",
        json!({ "share": "demo.xyz.com", "off": true }),
    )
    .await
    .unwrap();
    let schedules = h.backend.lock().schedules.clone();
    assert_eq!(schedules.len(), 2);
    assert_eq!(schedules[0].1.as_ref().unwrap().from, "09:00");
    assert!(schedules[1].1.is_none());
    // A read-only server declines (and doesn't list them, see protocol_tests).
    let declined = h
        .call(
            Mode::ReadOnly,
            "pause_share",
            json!({ "share": "demo.xyz.com" }),
        )
        .await
        .unwrap();
    assert_eq!(declined["outcome"], "declined");
    assert_eq!(h.backend.lock().paused.len(), 2);
}

#[tokio::test]
async fn folders_are_shared_with_approval() {
    let h = harness();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().to_str().unwrap().to_owned();
    let asked = h
        .call(
            Mode::Ask,
            "share_folder",
            json!({ "path": path, "spa": true }),
        )
        .await
        .unwrap();
    assert_eq!(asked["outcome"], "needsApproval");
    assert!(asked["message"].as_str().unwrap().contains(".env"));
    let shared = h
        .call(
            Mode::Full,
            "share_folder",
            json!({ "path": path, "hostname": "docs.xyz.com", "listing": true }),
        )
        .await
        .unwrap();
    assert_eq!(shared["outcome"], "shared");
    assert_eq!(shared["share"]["url"], "https://docs.xyz.com");
    let folders = h.backend.lock().folders.clone();
    assert!(folders[0].lists() && !folders[0].spa);
    assert!(
        h.call(
            Mode::Full,
            "share_folder",
            json!({ "path": "/definitely/not/here" })
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn traffic_tools_explain_when_the_inspector_isnt_running() {
    let shared: SharedBackend = FakeBackend::new();
    let tools = CoreTools::new(
        shared,
        Arc::new(crate::traffic::NoTraffic),
        Arc::new(Plans::default()),
    );
    let ctx = ToolContext::detached(settings(Mode::Ask), actor());
    let err = tools
        .call("traffic_list", serde_json::Map::new(), &ctx)
        .await
        .unwrap_err();
    assert!(err.message.contains("inspector isn't running"));
}

#[tokio::test]
async fn pages_long_lists() {
    let h = harness();
    {
        let mut state = h.backend.lock();
        for i in 0..70 {
            state
                .routes
                .push(route(&format!("r{i:02}.xyz.com"), "http://localhost:3000"));
        }
    }
    let first = h
        .call(Mode::Ask, "list_routes", json!({ "limit": 50 }))
        .await
        .unwrap();
    assert_eq!(first["routes"].as_array().unwrap().len(), 50);
    assert_eq!(first["total"], 71);
    let next = first["nextCursor"].as_str().unwrap();
    let second = h
        .call(Mode::Ask, "list_routes", json!({ "cursor": next }))
        .await
        .unwrap();
    assert_eq!(second["routes"].as_array().unwrap().len(), 21);
    assert!(second["nextCursor"].is_null());
}

#[test]
fn builds_login_rules_from_allow_lists() {
    let rule =
        super::access_rule(&["me@xyz.com".into(), "@team.io".into(), "corp.com".into()]).unwrap();
    assert_eq!(rule.emails, ["me@xyz.com"]);
    assert_eq!(rule.email_domains, ["@team.io", "corp.com"]);
    assert!(super::access_rule(&[]).is_none());
}
