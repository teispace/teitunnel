//! Share on your own domain: a temporary route (a hostname on one of the account's
//! domains, on this machine's default tunnel) that goes away when the share is stopped,
//! when it expires, or when whoever started it (the app, or a `teitunnel-cli share`)
//! exits. It is made and removed through the plan → apply engine like any route; this
//! module only remembers which routes are temporary and when they end (M10-03, D-068).

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::{
    accounts::Accounts,
    engine::{
        AccessRule, Approval, Change, Connectors, Context, Engine, EngineError, Outcome, PlanError,
        RouteInput,
    },
    runtime,
    text::Text,
};

/// Who started a share: the app, or a CLI process (its [`runtime::this_process`]).
pub const APP_OWNER: &str = "app";

/// A temporary route, as remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DomainShare {
    /// Account id.
    pub account_id: String,
    /// The public hostname.
    pub hostname: String,
    /// The service shared, as the user gave it.
    pub origin: String,
    /// [`APP_OWNER`], or the CLI process that started it.
    pub owner: String,
    /// When it ends by itself (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub expires_at: Option<u64>,
    /// When it started (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
}

impl DomainShare {
    /// Whether it should be gone at `now` (milliseconds): expired, or its CLI owner exited.
    pub fn is_over(&self, now: u64) -> bool {
        self.expires_at.is_some_and(|at| at <= now)
            || (self.owner != APP_OWNER && !runtime::is_running(&self.owner))
    }
}

/// Milliseconds since the epoch.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// What to share and for how long.
#[derive(Debug, Clone)]
pub struct ShareRequest<'a> {
    /// Hostname on one of the account's domains.
    pub hostname: &'a str,
    /// The service: a port, `host:port` or a URL.
    pub origin: &'a str,
    /// Require a login.
    pub access: Option<AccessRule>,
    /// When it ends by itself (milliseconds since the epoch).
    pub expires_at: Option<u64>,
    /// [`APP_OWNER`] or a CLI process.
    pub owner: &'a str,
}

/// Starts a share: remembers it first (so a crash can't leave the route behind
/// unnoticed), then adds the route through a plan. A plan that would replace a DNS record
/// Teitunnel didn't create is refused (`NeedsConfirmation`): a temporary share never
/// takes over someone else's hostname.
///
/// # Errors
/// Engine errors; nothing is left remembered after one.
pub async fn start<C, K>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    request: ShareRequest<'_>,
) -> Result<Outcome, EngineError>
where
    C: crate::engine::CloudApi,
    K: Connectors,
{
    let ctx = Context {
        tunnel: None,
        ..ctx
    };
    let share = DomainShare {
        account_id: ctx.account.to_owned(),
        hostname: request.hostname.trim().to_ascii_lowercase(),
        origin: request.origin.trim().to_owned(),
        owner: request.owner.to_owned(),
        expires_at: request.expires_at,
        created_at: now_ms(),
    };
    let change = Change::AddRoute {
        route: RouteInput {
            hostname: share.hostname.clone(),
            path: None,
            origin: share.origin.clone(),
            access: request.access,
        },
    };
    let intent = engine.intent_for(api, ctx, &change).await?;
    let plan = engine.preview(api, ctx, &intent).await?;
    if plan.requires_confirmation {
        return Err(EngineError::NeedsConfirmation);
    }
    let local = engine.local();
    local
        .record_share(&share)
        .await
        .map_err(crate::engine::ObserveError::from)?;
    let approval = Approval {
        fingerprint: &plan.fingerprint,
        confirmed: false,
    };
    let outcome = engine
        .apply(api, connectors, ctx, &intent, approval, |_| {})
        .await;
    if !matches!(outcome, Ok(Outcome::Applied { .. })) {
        let _ = local.forget_share(ctx.account, &share.hostname).await;
    }
    outcome
}

/// Stops a share: removes its route (and the DNS record and login Teitunnel added for
/// it) through a plan, then forgets it. A route that's already gone is fine.
///
/// # Errors
/// A message; the share stays remembered so a later sweep can try again.
pub async fn stop<C, K>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    hostname: &str,
) -> Result<(), Text>
where
    C: crate::engine::CloudApi,
    K: Connectors,
{
    let ctx = Context {
        tunnel: None,
        ..ctx
    };
    let change = Change::RemoveRoute {
        hostname: hostname.to_owned(),
        path: None,
    };
    let removed = async {
        use crate::text::UserText as _;
        let engine_error = |e: EngineError| {
            (
                matches!(
                    e,
                    EngineError::Plan(PlanError::NoSuchRoute(_) | PlanError::NoTunnel)
                ),
                e.text(),
            )
        };
        let intent = engine
            .intent_for(api, ctx, &change)
            .await
            .map_err(engine_error)?;
        let plan = engine
            .preview(api, ctx, &intent)
            .await
            .map_err(engine_error)?;
        let approval = Approval {
            fingerprint: &plan.fingerprint,
            confirmed: false,
        };
        match engine
            .apply(api, connectors, ctx, &intent, approval, |_| {})
            .await
            .map_err(engine_error)?
        {
            Outcome::Applied { .. } => Ok(()),
            Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
                Err((false, error))
            }
        }
    }
    .await;
    // `true`: the route is already gone, which is what stopping wants.
    match removed {
        Ok(()) | Err((true, _)) => {
            use crate::text::UserText as _;
            engine
                .local()
                .forget_share(ctx.account, hostname)
                .await
                .map_err(|e| e.text())
        }
        Err((false, message)) => Err(message),
    }
}

/// Stops every share `over` says should go, in every account. Returns what couldn't be
/// stopped (they're tried again next time).
pub async fn sweep<K: Connectors>(
    accounts: &Accounts,
    engine: &Engine,
    connectors: &K,
    machine_name: &str,
    over: impl Fn(&DomainShare) -> bool,
) -> Vec<Text> {
    let mut failures = Vec::new();
    let shares = engine.local().shares(None).await.unwrap_or_default();
    for share in shares.iter().filter(|s| over(s)) {
        let Ok(api) = accounts.client(&share.account_id).await else {
            continue;
        };
        let ctx = Context {
            account: &share.account_id,
            machine_name,
            tunnel: None,
        };
        if let Err(message) = stop(engine, &api, connectors, ctx, &share.hostname).await {
            tracing::warn!(
                hostname = %share.hostname,
                "couldn't stop a domain share: {}",
                message.english()
            );
            failures.push(message);
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{
            Local,
            fake::{CloudState, FakeCloud, FakeConnectors},
        },
        store::Store,
    };

    const CTX: Context<'static> = Context {
        account: "acc",
        machine_name: "Mac",
        tunnel: None,
    };

    fn cloud() -> FakeCloud {
        FakeCloud::new(CloudState {
            zones: vec![crate::engine::ZoneRef {
                id: "z".into(),
                name: "xyz.com".into(),
            }],
            ..CloudState::default()
        })
    }

    fn request<'a>(hostname: &'a str, owner: &'a str) -> ShareRequest<'a> {
        ShareRequest {
            hostname,
            origin: "3000",
            access: None,
            expires_at: None,
            owner,
        }
    }

    fn hostnames(cloud: &FakeCloud) -> Vec<String> {
        cloud
            .snapshot()
            .records
            .values()
            .flatten()
            .map(|r| r.name.clone())
            .collect()
    }

    #[tokio::test]
    async fn starts_and_stops_through_the_engine() {
        let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
        let (cloud, conns) = (cloud(), FakeConnectors::default());
        let outcome = start(
            &engine,
            &cloud,
            &conns,
            CTX,
            request("Demo.xyz.com", APP_OWNER),
        )
        .await
        .unwrap();
        assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
        assert_eq!(hostnames(&cloud), ["demo.xyz.com"]);
        let shares = engine.local().shares(Some("acc")).await.unwrap();
        assert_eq!(shares.len(), 1);
        assert_eq!(
            (shares[0].hostname.as_str(), shares[0].owner.as_str()),
            ("demo.xyz.com", APP_OWNER)
        );

        stop(&engine, &cloud, &conns, CTX, "demo.xyz.com")
            .await
            .unwrap();
        assert!(hostnames(&cloud).is_empty(), "the DNS record went too");
        assert!(engine.local().shares(None).await.unwrap().is_empty());
        // Stopping again (already gone) is fine.
        stop(&engine, &cloud, &conns, CTX, "demo.xyz.com")
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn never_takes_over_someone_elses_hostname() {
        let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
        let (cloud, conns) = (cloud(), FakeConnectors::default());
        cloud.state.lock().unwrap().records.insert(
            "z".into(),
            vec![cf_api::DnsRecord {
                id: "theirs".into(),
                name: "www.xyz.com".into(),
                kind: "A".into(),
                content: "192.0.2.1".into(),
                proxied: false,
                comment: None,
                ttl: 300,
            }],
        );
        let refused = start(
            &engine,
            &cloud,
            &conns,
            CTX,
            request("www.xyz.com", APP_OWNER),
        )
        .await;
        assert!(
            matches!(refused, Err(EngineError::NeedsConfirmation)),
            "{refused:?}"
        );
        assert!(engine.local().shares(None).await.unwrap().is_empty());
        assert_eq!(hostnames(&cloud), ["www.xyz.com"]);
    }

    #[tokio::test]
    async fn sweeps_expired_shares_and_those_of_exited_clis() {
        let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
        let (cloud, conns) = (cloud(), FakeConnectors::default());
        let me = runtime::this_process();
        let gone = "1-1"; // pid 1's start time is never 1
        start(&engine, &cloud, &conns, CTX, request("a.xyz.com", &me))
            .await
            .unwrap();
        start(&engine, &cloud, &conns, CTX, request("b.xyz.com", gone))
            .await
            .unwrap();
        let mut timed = request("c.xyz.com", APP_OWNER);
        timed.expires_at = Some(now_ms() - 1);
        start(&engine, &cloud, &conns, CTX, timed).await.unwrap();
        start(
            &engine,
            &cloud,
            &conns,
            CTX,
            request("d.xyz.com", APP_OWNER),
        )
        .await
        .unwrap();

        let shares = engine.local().shares(None).await.unwrap();
        let now = now_ms();
        let over: Vec<&str> = shares
            .iter()
            .filter(|s| s.is_over(now))
            .map(|s| s.hostname.as_str())
            .collect();
        assert_eq!(over, ["b.xyz.com", "c.xyz.com"]);

        for share in shares.iter().filter(|s| s.is_over(now)) {
            stop(&engine, &cloud, &conns, CTX, &share.hostname)
                .await
                .unwrap();
        }
        let mut left = hostnames(&cloud);
        left.sort();
        assert_eq!(left, ["a.xyz.com", "d.xyz.com"]);
    }
}
