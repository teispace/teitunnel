//! In-memory Cloudflare and connectors for executor tests, with failure injection.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use cf_api::{
    AccessApp, ApiMessage, DnsRecord, NewAccessApp, NewDnsRecord, Tunnel, TunnelConfig,
    VersionedConfig,
};

use super::{
    cloud::{CloudApi, Connectors},
    types::ZoneRef,
};
use crate::{Secret, runtime::ConnectorState};

/// A tunnel as the fake stores it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FakeTunnel {
    pub(crate) name: String,
    pub(crate) version: u64,
    pub(crate) config: Option<TunnelConfig>,
}

/// Everything the fake Cloudflare holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CloudState {
    pub(crate) zones: Vec<ZoneRef>,
    pub(crate) tunnels: BTreeMap<String, FakeTunnel>,
    pub(crate) records: BTreeMap<String, Vec<DnsRecord>>,
    /// Zero Trust is set up.
    pub(crate) access_org: bool,
    /// Login methods.
    pub(crate) login_methods: Vec<cf_api::IdentityProvider>,
    /// Access applications by id.
    pub(crate) access_apps: BTreeMap<String, NewAccessApp>,
    /// The token can't read Access (reads fail with 403).
    pub(crate) access_forbidden: bool,
    /// Private network routes by id.
    pub(crate) network_routes: BTreeMap<String, cf_api::NetworkRoute>,
    /// The default virtual network's id.
    pub(crate) default_vnet: Option<String>,
    /// The token can't read private networks (reads fail with 403).
    pub(crate) networks_forbidden: bool,
    /// WARP client settings (`None`: can't be read).
    pub(crate) device_settings: Option<cf_api::DeviceSettings>,
    /// The default device profile (`None`: can't be read).
    pub(crate) device_profile: Option<cf_api::DefaultDeviceProfile>,
    /// Load balancing is available (the add-on and the permission); reads 403 otherwise.
    pub(crate) load_balancing: bool,
    /// Monitors, pools and load balancers (by zone) by id.
    pub(crate) lb_monitors: BTreeMap<String, cf_api::Monitor>,
    pub(crate) lb_pools: BTreeMap<String, cf_api::Pool>,
    pub(crate) load_balancers: BTreeMap<String, (String, cf_api::LoadBalancer)>,
    /// Pool endpoints (by address) whose health checks fail.
    pub(crate) lb_failing: BTreeSet<String>,
    /// Workers (Snapshots) by name.
    pub(crate) workers: BTreeMap<String, FakeWorker>,
    /// Custom Domains by id.
    pub(crate) worker_domains: BTreeMap<String, cf_api::WorkerDomain>,
    /// The workers.dev subdomain.
    pub(crate) workers_subdomain: Option<String>,
    /// Asset hashes Cloudflare stores (uploads aren't state the user sees).
    pub(crate) assets: BTreeSet<String>,
    /// Upload and completion tokens → the manifest they're for.
    pub(crate) asset_tokens: BTreeMap<String, (bool, BTreeMap<String, String>)>,
    /// Zone plans (`legacy_id`) by zone id; absent means Free.
    pub(crate) zone_plans: BTreeMap<String, String>,
    /// Entry point rulesets by `(zone id, phase)`: the ruleset id and its rules.
    pub(crate) rulesets: BTreeMap<(String, String), (String, Vec<cf_api::Rule>)>,
    /// The token can't read or write rulesets (403).
    pub(crate) rulesets_forbidden: bool,
    /// The token can't read or write Cache Rules (403), an optional permission.
    pub(crate) cache_rules_forbidden: bool,
    /// Access service tokens by id.
    pub(crate) service_tokens: BTreeMap<String, cf_api::ServiceToken>,
    /// The token can't manage service tokens (403).
    pub(crate) service_tokens_forbidden: bool,
    /// D1 databases by id (real SQLite in memory).
    pub(crate) d1: BTreeMap<String, FakeD1>,
    /// The token can't use D1 (403).
    pub(crate) d1_forbidden: bool,
    /// Worker routes by id: the zone and the route.
    pub(crate) worker_routes: BTreeMap<String, (String, cf_api::WorkerRoute)>,
}

/// A D1 database as the fake keeps it: a name and an in-memory SQLite connection.
#[derive(Clone)]
pub(crate) struct FakeD1 {
    pub(crate) name: String,
    pub(crate) conn: std::sync::Arc<Mutex<rusqlite::Connection>>,
}

impl FakeD1 {
    pub(crate) fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            conn: std::sync::Arc::new(Mutex::new(rusqlite::Connection::open_in_memory().unwrap())),
        }
    }

    /// Runs statements like D1's query endpoint (a batch is one transaction).
    pub(crate) fn run(
        &self,
        statements: &[cf_api::D1Statement],
    ) -> cf_api::Result<Vec<cf_api::D1Result>> {
        use rusqlite::types::Value as Sql;
        let to_sql = |v: &serde_json::Value| match v {
            serde_json::Value::Null => Sql::Null,
            serde_json::Value::Bool(b) => Sql::Integer(i64::from(*b)),
            serde_json::Value::Number(n) => n
                .as_i64()
                .map_or_else(|| Sql::Real(n.as_f64().unwrap_or_default()), Sql::Integer),
            serde_json::Value::String(s) => Sql::Text(s.clone()),
            other => Sql::Text(other.to_string()),
        };
        let sql_error = |e: rusqlite::Error| cf_api::Error::Api {
            status: 400,
            errors: vec![ApiMessage {
                code: 7500,
                message: e.to_string(),
            }],
        };
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction().map_err(sql_error)?;
        let mut out = Vec::new();
        for statement in statements {
            let mut stmt = tx.prepare(&statement.sql).map_err(sql_error)?;
            let names: Vec<String> = stmt
                .column_names()
                .iter()
                .map(|s| (*s).to_owned())
                .collect();
            let params: Vec<Sql> = statement.params.iter().map(to_sql).collect();
            let mut results = Vec::new();
            if names.is_empty() {
                let changes = stmt
                    .execute(rusqlite::params_from_iter(params))
                    .map_err(sql_error)?;
                out.push(cf_api::D1Result {
                    results,
                    success: true,
                    meta: cf_api::D1Meta {
                        changes: changes as u64,
                        last_row_id: tx.last_insert_rowid(),
                        rows_read: 0,
                        rows_written: changes as u64,
                    },
                });
                continue;
            }
            let mut rows = stmt
                .query(rusqlite::params_from_iter(params))
                .map_err(sql_error)?;
            while let Some(row) = rows.next().map_err(sql_error)? {
                let mut object = serde_json::Map::new();
                for (i, name) in names.iter().enumerate() {
                    let value = match row.get::<_, Sql>(i).map_err(sql_error)? {
                        Sql::Null => serde_json::Value::Null,
                        Sql::Integer(n) => serde_json::json!(n),
                        Sql::Real(f) => serde_json::json!(f),
                        Sql::Text(s) => serde_json::json!(s),
                        Sql::Blob(b) => serde_json::json!(b),
                    };
                    object.insert(name.clone(), value);
                }
                results.push(serde_json::Value::Object(object));
            }
            out.push(cf_api::D1Result {
                meta: cf_api::D1Meta {
                    rows_read: results.len() as u64,
                    ..cf_api::D1Meta::default()
                },
                results,
                success: true,
            });
        }
        tx.commit().map_err(sql_error)?;
        Ok(out)
    }
}

impl std::fmt::Debug for FakeD1 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FakeD1({})", self.name)
    }
}

impl PartialEq for FakeD1 {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}

/// A Worker version as the fake stores it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FakeVersion {
    pub(crate) id: String,
    /// Metadata without the one-off assets token.
    pub(crate) metadata: serde_json::Value,
    /// Path → hash.
    pub(crate) assets: BTreeMap<String, String>,
}

/// A Worker as the fake stores it.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct FakeWorker {
    pub(crate) versions: Vec<FakeVersion>,
    pub(crate) active: String,
    pub(crate) workers_dev: bool,
}

impl FakeWorker {
    pub(crate) fn live(&self) -> Option<&FakeVersion> {
        self.versions.iter().find(|v| v.id == self.active)
    }
}

/// State with ids and versions stripped, for "is it back to how it was?" checks.
pub(crate) type Normalized = (
    Vec<(String, Option<serde_json::Value>)>,
    Vec<(String, String, String, String, bool, u32, Option<String>)>,
    Vec<serde_json::Value>,
    usize,
    Vec<(String, String, String, Option<String>)>,
    Vec<(String, Option<serde_json::Value>, bool)>,
    Vec<(String, String)>,
    (
        Vec<(String, String, Vec<cf_api::NewRule>)>,
        Vec<String>,
        Vec<String>,
        Vec<(String, String, Option<String>)>,
    ),
);

impl CloudState {
    pub(crate) fn normalized(&self) -> Normalized {
        let tunnels = self
            .tunnels
            .values()
            .map(|t| {
                (
                    t.name.clone(),
                    t.config.as_ref().and_then(|c| serde_json::to_value(c).ok()),
                )
            })
            .collect();
        let mut records: Vec<_> = self
            .records
            .iter()
            .flat_map(|(zone, list)| {
                list.iter().map(|r| {
                    (
                        zone.clone(),
                        r.name.clone(),
                        r.kind.clone(),
                        r.content.clone(),
                        r.proxied,
                        r.ttl,
                        r.comment.clone(),
                    )
                })
            })
            .collect();
        records.sort();
        let mut apps: Vec<serde_json::Value> = self
            .access_apps
            .values()
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect();
        apps.sort_by_key(ToString::to_string);
        let mut networks: Vec<_> = self
            .network_routes
            .values()
            .map(|r| {
                (
                    r.network.clone(),
                    r.tunnel_id.clone(),
                    r.comment.clone(),
                    r.virtual_network_id.clone(),
                )
            })
            .collect();
        networks.sort();
        // Workers by what serves (the live version's settings and files), not by ids.
        let workers = self
            .workers
            .iter()
            .map(|(name, w)| {
                (
                    name.clone(),
                    w.live().map(|v| serde_json::json!([v.metadata, v.assets])),
                    w.workers_dev,
                )
            })
            .collect();
        let mut domains: Vec<_> = self
            .worker_domains
            .values()
            .map(|d| (d.hostname.clone(), d.service.clone()))
            .collect();
        domains.sort();
        // Rules by definition and order (ids change when a rule is recreated); an
        // empty entry point is the same as none.
        let rulesets = self
            .rulesets
            .iter()
            .filter(|(_, (_, rules))| !rules.is_empty())
            .map(|((zone, phase), (_, rules))| {
                (
                    zone.clone(),
                    phase.clone(),
                    rules.iter().map(cf_api::Rule::to_new).collect(),
                )
            })
            .collect();
        let mut tokens: Vec<String> = self
            .service_tokens
            .values()
            .map(|t| t.name.clone())
            .collect();
        tokens.sort();
        let mut databases: Vec<String> = self.d1.values().map(|d| d.name.clone()).collect();
        databases.sort();
        let mut routes: Vec<(String, String, Option<String>)> = self
            .worker_routes
            .values()
            .map(|(zone, r)| (zone.clone(), r.pattern.clone(), r.script.clone()))
            .collect();
        routes.sort();
        (
            tunnels,
            records,
            apps,
            self.login_methods.len(),
            networks,
            workers,
            domains,
            (rulesets, tokens, databases, routes),
        )
    }

    pub(crate) fn record_count(&self) -> usize {
        self.records.values().map(Vec::len).sum()
    }
}

/// The fake. Mutations are numbered from 0; `fail_from` makes mutation `n` and every
/// later one fail, `fail_once` only mutation `n`.
#[derive(Debug, Default)]
pub(crate) struct FakeCloud {
    pub(crate) state: Mutex<CloudState>,
    ids: AtomicU32,
    mutations: AtomicU32,
    fail_once: Mutex<Option<u32>>,
    fail_from: Mutex<Option<u32>>,
}

fn injected() -> cf_api::Error {
    cf_api::Error::Api {
        status: 500,
        errors: vec![ApiMessage {
            code: 1000,
            message: "injected failure".into(),
        }],
    }
}

fn forbidden() -> cf_api::Error {
    cf_api::Error::Api {
        status: 403,
        errors: vec![ApiMessage {
            code: 10000,
            message: "Authentication error".into(),
        }],
    }
}

fn conflict() -> cf_api::Error {
    cf_api::Error::Api {
        status: 409,
        errors: vec![ApiMessage {
            code: 1002,
            message: "still referenced".into(),
        }],
    }
}

fn not_found() -> cf_api::Error {
    cf_api::Error::Api {
        status: 404,
        errors: vec![ApiMessage {
            code: 1003,
            message: "not found".into(),
        }],
    }
}

impl FakeCloud {
    pub(crate) fn new(state: CloudState) -> Self {
        Self {
            state: Mutex::new(state),
            ..Self::default()
        }
    }

    pub(crate) fn snapshot(&self) -> CloudState {
        self.state.lock().unwrap().clone()
    }

    /// Changes the account behind Teitunnel's back (someone in the dashboard).
    pub(crate) fn edit(&self, change: impl FnOnce(&mut CloudState)) {
        change(&mut self.state.lock().unwrap());
    }

    pub(crate) fn fail_once(&self, n: u32) {
        *self.fail_once.lock().unwrap() = Some(n);
    }

    pub(crate) fn fail_from(&self, n: u32) {
        *self.fail_from.lock().unwrap() = Some(n);
    }

    pub(crate) fn reset_failures(&self) {
        *self.fail_once.lock().unwrap() = None;
        *self.fail_from.lock().unwrap() = None;
        self.mutations.store(0, Ordering::SeqCst);
    }

    /// Mutations made so far.
    pub(crate) fn mutations(&self) -> u32 {
        self.mutations.load(Ordering::SeqCst)
    }

    fn mutate(&self) -> cf_api::Result<()> {
        let n = self.mutations.fetch_add(1, Ordering::SeqCst);
        let once = *self.fail_once.lock().unwrap() == Some(n);
        let from = self.fail_from.lock().unwrap().is_some_and(|f| n >= f);
        if once || from {
            Err(injected())
        } else {
            Ok(())
        }
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.ids.fetch_add(1, Ordering::SeqCst))
    }

    fn with_zone<T>(
        &self,
        zone: &str,
        f: impl FnOnce(&mut Vec<DnsRecord>) -> cf_api::Result<T>,
    ) -> cf_api::Result<T> {
        let mut state = self.state.lock().unwrap();
        if !state.zones.iter().any(|z| z.id == zone) {
            return Err(not_found());
        }
        f(state.records.entry(zone.to_owned()).or_default())
    }
}

/// A record as Cloudflare would create it next to `records`, or its refusal.
fn new_record(
    records: &[DnsRecord],
    id: String,
    record: &NewDnsRecord,
) -> cf_api::Result<DnsRecord> {
    let clash = records.iter().any(|r| {
        r.name.eq_ignore_ascii_case(&record.name) && (r.kind == "CNAME" || record.kind == "CNAME")
    });
    if clash {
        return Err(cf_api::Error::Api {
            status: 400,
            errors: vec![ApiMessage {
                code: 81053,
                message: "An A, AAAA, or CNAME record with that host already exists.".into(),
            }],
        });
    }
    Ok(DnsRecord {
        id,
        name: record.name.clone(),
        kind: record.kind.clone(),
        content: record.content.clone(),
        proxied: record.proxied,
        comment: record.comment.clone(),
        ttl: record.ttl,
    })
}

impl CloudState {
    /// Whether the token may not use `phase`'s rules.
    fn phase_forbidden(&self, phase: &str) -> bool {
        self.rulesets_forbidden || (self.cache_rules_forbidden && phase == cf_api::PHASE_CACHE)
    }
}

fn tunnel_view(id: &str, t: &FakeTunnel) -> Tunnel {
    Tunnel {
        id: id.to_owned(),
        name: t.name.clone(),
        status: "inactive".into(),
        created_at: String::new(),
        deleted_at: None,
        remote_config: true,
        connections: Vec::new(),
    }
}

impl CloudApi for FakeCloud {
    async fn zones(&self, _account: &str) -> cf_api::Result<Vec<ZoneRef>> {
        Ok(self.state.lock().unwrap().zones.clone())
    }

    async fn tunnel(&self, _account: &str, id: &str) -> cf_api::Result<Option<Tunnel>> {
        let state = self.state.lock().unwrap();
        Ok(state.tunnels.get(id).map(|t| tunnel_view(id, t)))
    }

    async fn tunnel_config(&self, _account: &str, id: &str) -> cf_api::Result<VersionedConfig> {
        let state = self.state.lock().unwrap();
        let t = state.tunnels.get(id).ok_or_else(not_found)?;
        Ok(VersionedConfig {
            version: t.version,
            config: t.config.clone(),
        })
    }

    async fn put_tunnel_config(
        &self,
        _account: &str,
        id: &str,
        config: &TunnelConfig,
    ) -> cf_api::Result<VersionedConfig> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let t = state.tunnels.get_mut(id).ok_or_else(not_found)?;
        t.version += 1;
        t.config = Some(config.clone());
        Ok(VersionedConfig {
            version: t.version,
            config: t.config.clone(),
        })
    }

    async fn tunnels(&self, _account: &str) -> cf_api::Result<Vec<Tunnel>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .tunnels
            .iter()
            .map(|(id, t)| tunnel_view(id, t))
            .collect())
    }

    async fn clean_connections(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        let state = self.state.lock().unwrap();
        state.tunnels.get(id).map(|_| ()).ok_or_else(not_found)
    }

    async fn tunnel_token(&self, _account: &str, id: &str) -> cf_api::Result<Secret<String>> {
        let state = self.state.lock().unwrap();
        state.tunnels.get(id).ok_or_else(not_found)?;
        Ok(Secret::new(format!("token-for-{id}")))
    }

    async fn create_tunnel(&self, _account: &str, name: &str) -> cf_api::Result<Tunnel> {
        self.mutate()?;
        let id = self.next_id("tunnel");
        let tunnel = FakeTunnel {
            name: name.to_owned(),
            version: 0,
            config: None,
        };
        let view = tunnel_view(&id, &tunnel);
        self.state.lock().unwrap().tunnels.insert(id, tunnel);
        Ok(view)
    }

    async fn delete_tunnel(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state
            .lock()
            .unwrap()
            .tunnels
            .remove(id)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    async fn records_named(&self, zone: &str, name: &str) -> cf_api::Result<Vec<DnsRecord>> {
        self.with_zone(zone, |records| {
            Ok(records
                .iter()
                .filter(|r| r.name.eq_ignore_ascii_case(name))
                .cloned()
                .collect())
        })
    }

    async fn cname_records(&self, zone: &str) -> cf_api::Result<Vec<DnsRecord>> {
        self.with_zone(zone, |records| {
            Ok(records
                .iter()
                .filter(|r| r.kind == "CNAME")
                .cloned()
                .collect())
        })
    }

    async fn records_with_comment(
        &self,
        zone: &str,
        needle: &str,
    ) -> cf_api::Result<Vec<DnsRecord>> {
        self.with_zone(zone, |records| {
            Ok(records
                .iter()
                .filter(|r| r.comment.as_deref().is_some_and(|c| c.contains(needle)))
                .cloned()
                .collect())
        })
    }

    async fn create_record(&self, zone: &str, record: &NewDnsRecord) -> cf_api::Result<DnsRecord> {
        self.mutate()?;
        let id = self.next_id("rec");
        self.with_zone(zone, |records| {
            let created = new_record(records, id, record)?;
            records.push(created.clone());
            Ok(created)
        })
    }

    async fn create_records(
        &self,
        zone: &str,
        records: &[NewDnsRecord],
    ) -> cf_api::Result<Vec<DnsRecord>> {
        // One call, all or nothing, like Cloudflare's batch endpoint.
        self.mutate()?;
        let ids: Vec<_> = records.iter().map(|_| self.next_id("rec")).collect();
        self.with_zone(zone, |existing| {
            let mut next = existing.clone();
            for (id, record) in ids.into_iter().zip(records) {
                let created = new_record(&next, id, record)?;
                next.push(created);
            }
            let created = next[existing.len()..].to_vec();
            *existing = next;
            Ok(created)
        })
    }

    async fn update_record(
        &self,
        zone: &str,
        id: &str,
        record: &NewDnsRecord,
    ) -> cf_api::Result<DnsRecord> {
        self.mutate()?;
        self.with_zone(zone, |records| {
            let r = records
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or_else(not_found)?;
            if r.kind != record.kind {
                // As Cloudflare since 2026-06-30: a type change is a delete and a create.
                return Err(cf_api::Error::Api {
                    status: 400,
                    errors: vec![ApiMessage {
                        code: 9000,
                        message: "DNS record type cannot be changed.".into(),
                    }],
                });
            }
            r.name.clone_from(&record.name);
            r.kind.clone_from(&record.kind);
            r.content.clone_from(&record.content);
            r.proxied = record.proxied;
            r.ttl = record.ttl;
            r.comment.clone_from(&record.comment);
            Ok(r.clone())
        })
    }

    async fn replace_record(
        &self,
        zone: &str,
        id: &str,
        record: &NewDnsRecord,
    ) -> cf_api::Result<DnsRecord> {
        self.mutate()?;
        let new_id = self.next_id("rec");
        self.with_zone(zone, |records| {
            let at = records
                .iter()
                .position(|r| r.id == id)
                .ok_or_else(not_found)?;
            let created = DnsRecord {
                id: new_id,
                name: record.name.clone(),
                kind: record.kind.clone(),
                content: record.content.clone(),
                proxied: record.proxied,
                comment: record.comment.clone(),
                ttl: record.ttl,
            };
            records[at] = created.clone();
            Ok(created)
        })
    }

    async fn delete_record(&self, zone: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.with_zone(zone, |records| {
            let before = records.len();
            records.retain(|r| r.id != id);
            if records.len() == before {
                Err(not_found())
            } else {
                Ok(())
            }
        })
    }

    async fn access_setup(
        &self,
        _account: &str,
    ) -> cf_api::Result<(bool, Vec<cf_api::IdentityProvider>)> {
        let state = self.state.lock().unwrap();
        if state.access_forbidden {
            return Err(forbidden());
        }
        Ok((state.access_org, state.login_methods.clone()))
    }

    async fn access_apps_for(
        &self,
        _account: &str,
        domain: &str,
    ) -> cf_api::Result<Vec<AccessApp>> {
        let state = self.state.lock().unwrap();
        if state.access_forbidden {
            return Err(forbidden());
        }
        Ok(state
            .access_apps
            .iter()
            .filter(|(_, app)| app.domain.eq_ignore_ascii_case(domain))
            .map(|(id, app)| fake_app(id, app))
            .collect())
    }

    async fn create_access_app(
        &self,
        _account: &str,
        app: &NewAccessApp,
    ) -> cf_api::Result<AccessApp> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        if !state.access_org {
            return Err(cf_api::Error::Api {
                status: 400,
                errors: vec![ApiMessage {
                    code: 12130,
                    message: "access.api.error.not_enabled".into(),
                }],
            });
        }
        let id = self.next_id("app");
        state.access_apps.insert(id.clone(), app.clone());
        Ok(fake_app(&id, app))
    }

    async fn update_access_app(
        &self,
        _account: &str,
        id: &str,
        app: &NewAccessApp,
    ) -> cf_api::Result<AccessApp> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let slot = state.access_apps.get_mut(id).ok_or_else(not_found)?;
        *slot = app.clone();
        Ok(fake_app(id, app))
    }

    async fn delete_access_app(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        state
            .access_apps
            .remove(id)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    async fn network_routes(&self, _account: &str) -> cf_api::Result<Vec<cf_api::NetworkRoute>> {
        let state = self.state.lock().unwrap();
        if state.networks_forbidden {
            return Err(forbidden());
        }
        Ok(state.network_routes.values().cloned().collect())
    }

    async fn default_virtual_network(&self, _account: &str) -> cf_api::Result<Option<String>> {
        let state = self.state.lock().unwrap();
        if state.networks_forbidden {
            return Err(forbidden());
        }
        Ok(state.default_vnet.clone())
    }

    async fn create_network_route(
        &self,
        _account: &str,
        network: &str,
        tunnel: &str,
        comment: &str,
        virtual_network: Option<&str>,
    ) -> cf_api::Result<cf_api::NetworkRoute> {
        self.mutate()?;
        let id = self.next_id("net");
        let mut state = self.state.lock().unwrap();
        let vnet = virtual_network
            .map(str::to_owned)
            .or_else(|| state.default_vnet.clone());
        // Cloudflare refuses a range that's already routed in the same virtual network.
        if state
            .network_routes
            .values()
            .any(|r| r.network == network && r.virtual_network_id == vnet)
        {
            return Err(cf_api::Error::Api {
                status: 409,
                errors: vec![ApiMessage {
                    code: 1014,
                    message: "route already exists".into(),
                }],
            });
        }
        let route = cf_api::NetworkRoute {
            id: id.clone(),
            network: network.to_owned(),
            tunnel_id: tunnel.to_owned(),
            tunnel_name: None,
            virtual_network_id: vnet,
            comment: comment.to_owned(),
        };
        state.network_routes.insert(id, route.clone());
        Ok(route)
    }

    async fn delete_network_route(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state
            .lock()
            .unwrap()
            .network_routes
            .remove(id)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    async fn device_settings(&self, _account: &str) -> cf_api::Result<cf_api::DeviceSettings> {
        self.state
            .lock()
            .unwrap()
            .device_settings
            .ok_or_else(forbidden)
    }

    async fn default_device_profile(
        &self,
        _account: &str,
    ) -> cf_api::Result<cf_api::DefaultDeviceProfile> {
        self.state
            .lock()
            .unwrap()
            .device_profile
            .clone()
            .ok_or_else(forbidden)
    }

    async fn create_one_time_pin(&self, _account: &str) -> cf_api::Result<String> {
        self.mutate()?;
        let id = self.next_id("idp");
        self.state
            .lock()
            .unwrap()
            .login_methods
            .push(cf_api::IdentityProvider {
                id: id.clone(),
                name: "One-time PIN".into(),
                kind: "onetimepin".into(),
            });
        Ok(id)
    }

    async fn delete_login_method(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let before = state.login_methods.len();
        state.login_methods.retain(|m| m.id != id);
        if state.login_methods.len() == before {
            Err(not_found())
        } else {
            Ok(())
        }
    }

    async fn lb_monitors(&self, _account: &str) -> cf_api::Result<Vec<cf_api::Monitor>> {
        let state = self.state.lock().unwrap();
        if !state.load_balancing {
            return Err(forbidden());
        }
        Ok(state.lb_monitors.values().cloned().collect())
    }

    async fn create_lb_monitor(
        &self,
        _account: &str,
        monitor: &cf_api::Monitor,
    ) -> cf_api::Result<cf_api::Monitor> {
        self.mutate()?;
        let mut created = monitor.clone();
        created.id = self.next_id("mon");
        self.state
            .lock()
            .unwrap()
            .lb_monitors
            .insert(created.id.clone(), created.clone());
        Ok(created)
    }

    async fn delete_lb_monitor(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        // Cloudflare refuses to delete a monitor a pool still uses.
        if state
            .lb_pools
            .values()
            .any(|p| p.monitor.as_deref() == Some(id))
        {
            return Err(conflict());
        }
        state.lb_monitors.remove(id);
        Ok(())
    }

    async fn lb_pools(&self, _account: &str) -> cf_api::Result<Vec<cf_api::Pool>> {
        let state = self.state.lock().unwrap();
        if !state.load_balancing {
            return Err(forbidden());
        }
        Ok(state.lb_pools.values().cloned().collect())
    }

    async fn create_lb_pool(
        &self,
        _account: &str,
        pool: &cf_api::Pool,
    ) -> cf_api::Result<cf_api::Pool> {
        self.mutate()?;
        let mut created = pool.clone();
        created.id = self.next_id("pool");
        self.state
            .lock()
            .unwrap()
            .lb_pools
            .insert(created.id.clone(), created.clone());
        Ok(created)
    }

    async fn update_lb_pool(
        &self,
        _account: &str,
        id: &str,
        pool: &cf_api::Pool,
    ) -> cf_api::Result<cf_api::Pool> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let Some(existing) = state.lb_pools.get_mut(id) else {
            return Err(not_found());
        };
        let mut updated = pool.clone();
        updated.id = id.to_owned();
        *existing = updated.clone();
        Ok(updated)
    }

    async fn delete_lb_pool(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        // Cloudflare refuses to delete a pool a load balancer still uses.
        if state
            .load_balancers
            .values()
            .any(|(_, lb)| lb.default_pools.iter().any(|p| p == id) || lb.fallback_pool == id)
        {
            return Err(conflict());
        }
        state.lb_pools.remove(id);
        Ok(())
    }

    async fn lb_pool_health(&self, _account: &str, id: &str) -> cf_api::Result<cf_api::PoolHealth> {
        let state = self.state.lock().unwrap();
        let pool = state.lb_pools.get(id).ok_or_else(not_found)?;
        let seen = pool
            .origins
            .iter()
            .map(|o| {
                let failing = state.lb_failing.contains(&o.address);
                cf_api::OriginHealth {
                    address: o.address.clone(),
                    healthy: !failing,
                    failure_reason: failing.then(|| "HTTP timeout occurred".to_owned()),
                    response_code: Some(if failing { 0 } else { 200 }),
                }
            })
            .collect::<Vec<_>>();
        Ok(cf_api::PoolHealth {
            regions: ["Amsterdam, NL", "Tokyo, JP"]
                .into_iter()
                .map(|r| (r.to_owned(), seen.clone()))
                .collect(),
        })
    }

    async fn load_balancers(&self, zone: &str) -> cf_api::Result<Vec<cf_api::LoadBalancer>> {
        let state = self.state.lock().unwrap();
        if !state.load_balancing {
            return Err(forbidden());
        }
        Ok(state
            .load_balancers
            .values()
            .filter(|(z, _)| z == zone)
            .map(|(_, lb)| lb.clone())
            .collect())
    }

    async fn create_load_balancer(
        &self,
        zone: &str,
        balancer: &cf_api::LoadBalancer,
    ) -> cf_api::Result<cf_api::LoadBalancer> {
        self.mutate()?;
        let mut created = balancer.clone();
        created.id = self.next_id("lb");
        self.state
            .lock()
            .unwrap()
            .load_balancers
            .insert(created.id.clone(), (zone.to_owned(), created.clone()));
        Ok(created)
    }

    async fn delete_load_balancer(&self, _zone: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state.lock().unwrap().load_balancers.remove(id);
        Ok(())
    }

    async fn workers_subdomain(&self, _account: &str) -> cf_api::Result<Option<String>> {
        Ok(self.state.lock().unwrap().workers_subdomain.clone())
    }

    async fn worker_deployments(
        &self,
        _account: &str,
        script: &str,
    ) -> cf_api::Result<Option<Vec<cf_api::WorkerDeployment>>> {
        let state = self.state.lock().unwrap();
        Ok(state.workers.get(script).map(|w| {
            vec![cf_api::WorkerDeployment {
                id: format!("deployment-{}", w.active),
                created_on: None,
                versions: vec![cf_api::DeploymentVersion {
                    version_id: w.active.clone(),
                    percentage: 100.0,
                }],
            }]
        }))
    }

    async fn worker_domains(
        &self,
        _account: &str,
        service: Option<&str>,
        hostname: Option<&str>,
    ) -> cf_api::Result<Vec<cf_api::WorkerDomain>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .worker_domains
            .values()
            .filter(|d| service.is_none_or(|s| d.service == s))
            .filter(|d| hostname.is_none_or(|h| d.hostname.eq_ignore_ascii_case(h)))
            .cloned()
            .collect())
    }

    async fn worker_on_workers_dev(&self, _account: &str, script: &str) -> cf_api::Result<bool> {
        let state = self.state.lock().unwrap();
        state
            .workers
            .get(script)
            .map(|w| w.workers_dev)
            .ok_or_else(not_found)
    }

    async fn create_assets_upload_session(
        &self,
        _account: &str,
        _script: &str,
        manifest: &BTreeMap<String, cf_api::AssetEntry>,
    ) -> cf_api::Result<cf_api::UploadSession> {
        self.mutate()?;
        let jwt = self.next_id("session");
        let mut state = self.state.lock().unwrap();
        let mut missing: Vec<String> = manifest
            .values()
            .map(|e| e.hash.clone())
            .filter(|h| !state.assets.contains(h))
            .collect();
        missing.sort();
        missing.dedup();
        let paths = manifest
            .iter()
            .map(|(p, e)| (p.clone(), e.hash.clone()))
            .collect();
        // Nothing to send: the session's token completes the upload.
        state
            .asset_tokens
            .insert(jwt.clone(), (missing.is_empty(), paths));
        Ok(cf_api::UploadSession {
            jwt,
            buckets: missing.chunks(2).map(<[String]>::to_vec).collect(),
        })
    }

    async fn upload_assets(
        &self,
        _account: &str,
        jwt: &str,
        files: &[cf_api::AssetFile],
    ) -> cf_api::Result<Option<String>> {
        self.mutate()?;
        let done = self.next_id("done");
        let mut state = self.state.lock().unwrap();
        let Some((_, paths)) = state.asset_tokens.get(jwt).cloned() else {
            return Err(forbidden());
        };
        for file in files {
            state.assets.insert(file.hash.clone());
        }
        if paths.values().all(|h| state.assets.contains(h)) {
            state.asset_tokens.insert(done.clone(), (true, paths));
            return Ok(Some(done));
        }
        Ok(None)
    }

    async fn put_worker_script(
        &self,
        _account: &str,
        script: &str,
        metadata: &serde_json::Value,
        _modules: &[cf_api::WorkerModule],
    ) -> cf_api::Result<()> {
        self.mutate()?;
        let id = self.next_id("version");
        let mut state = self.state.lock().unwrap();
        let version = fake_version(&state, id, metadata)?;
        let worker = state.workers.entry(script.to_owned()).or_default();
        worker.active = version.id.clone();
        worker.versions.push(version);
        Ok(())
    }

    async fn upload_worker_version(
        &self,
        _account: &str,
        script: &str,
        metadata: &serde_json::Value,
        _modules: &[cf_api::WorkerModule],
    ) -> cf_api::Result<cf_api::WorkerVersion> {
        self.mutate()?;
        let id = self.next_id("version");
        let mut state = self.state.lock().unwrap();
        let mut version = fake_version(&state, id, metadata)?;
        let worker = state.workers.get_mut(script).ok_or_else(not_found)?;
        if metadata.get("keep_bindings").is_some()
            && let Some(live) = worker.live()
        {
            // Secrets carry over from the live version.
            let secrets: Vec<serde_json::Value> = live.metadata["bindings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|b| b["type"] == "secret_text")
                .cloned()
                .collect();
            if let Some(bindings) = version.metadata["bindings"].as_array_mut() {
                bindings.extend(secrets);
            }
            version
                .metadata
                .as_object_mut()
                .map(|m| m.remove("keep_bindings"));
        }
        worker.versions.push(version.clone());
        Ok(cf_api::WorkerVersion {
            id: version.id,
            number: Some(worker.versions.len() as u64),
        })
    }

    async fn deploy_worker_version(
        &self,
        _account: &str,
        script: &str,
        version_id: &str,
    ) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let worker = state.workers.get_mut(script).ok_or_else(not_found)?;
        if !worker.versions.iter().any(|v| v.id == version_id) {
            return Err(not_found());
        }
        version_id.clone_into(&mut worker.active);
        Ok(())
    }

    async fn set_worker_on_workers_dev(
        &self,
        _account: &str,
        script: &str,
        enabled: bool,
    ) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        state
            .workers
            .get_mut(script)
            .ok_or_else(not_found)?
            .workers_dev = enabled;
        Ok(())
    }

    async fn delete_worker_script(&self, _account: &str, script: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        state.workers.remove(script);
        state.worker_domains.retain(|_, d| d.service != script);
        Ok(())
    }

    async fn attach_worker_domain(
        &self,
        _account: &str,
        hostname: &str,
        zone_id: &str,
        service: &str,
    ) -> cf_api::Result<cf_api::WorkerDomain> {
        self.mutate()?;
        let id = self.next_id("domain");
        let mut state = self.state.lock().unwrap();
        let clash = state.records.get(zone_id).is_some_and(|records| {
            records.iter().any(|r| {
                r.name.eq_ignore_ascii_case(hostname)
                    && matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME")
            })
        }) || state
            .worker_domains
            .values()
            .any(|d| d.hostname.eq_ignore_ascii_case(hostname) && d.service != service);
        if clash || !state.workers.contains_key(service) {
            return Err(conflict());
        }
        let zone_name = state
            .zones
            .iter()
            .find(|z| z.id == zone_id)
            .map(|z| z.name.clone())
            .ok_or_else(not_found)?;
        let domain = cf_api::WorkerDomain {
            id: id.clone(),
            hostname: hostname.to_owned(),
            service: service.to_owned(),
            zone_id: zone_id.to_owned(),
            zone_name,
        };
        state.worker_domains.insert(id, domain.clone());
        Ok(domain)
    }

    async fn detach_worker_domain(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state.lock().unwrap().worker_domains.remove(id);
        Ok(())
    }

    async fn zone_plan(&self, zone: &str) -> cf_api::Result<Option<String>> {
        let state = self.state.lock().unwrap();
        if !state.zones.iter().any(|z| z.id == zone) {
            return Err(not_found());
        }
        Ok(Some(
            state
                .zone_plans
                .get(zone)
                .cloned()
                .unwrap_or_else(|| "free".into()),
        ))
    }

    async fn phase_entrypoint(
        &self,
        zone: &str,
        phase: &str,
    ) -> cf_api::Result<Option<cf_api::Ruleset>> {
        let state = self.state.lock().unwrap();
        if state.phase_forbidden(phase) {
            return Err(forbidden());
        }
        Ok(state
            .rulesets
            .get(&(zone.to_owned(), phase.to_owned()))
            .map(|(id, rules)| cf_api::Ruleset {
                id: id.clone(),
                phase: phase.to_owned(),
                rules: rules.clone(),
            }))
    }

    async fn create_rule(
        &self,
        zone: &str,
        phase: &str,
        ruleset: Option<&str>,
        rule: &cf_api::NewRule,
        index: Option<u32>,
    ) -> cf_api::Result<(String, cf_api::Rule)> {
        self.mutate()?;
        let new_ruleset = self.next_id("ruleset");
        let mut state = self.state.lock().unwrap();
        // Unique among the rules a state made by another fake already has.
        let rule_id = loop {
            let id = self.next_id("rule");
            if !state
                .rulesets
                .values()
                .any(|(_, rules)| rules.iter().any(|r| r.id == id))
            {
                break id;
            }
        };
        if state.phase_forbidden(phase) {
            return Err(forbidden());
        }
        // Like Cloudflare: a Free zone's rate limits can't match a hostname.
        let plan = state.zone_plans.get(zone).map_or("free", String::as_str);
        if phase == cf_api::PHASE_RATE_LIMIT
            && plan == "free"
            && rule.expression.contains("http.host")
        {
            return Err(cf_api::Error::Api {
                status: 400,
                errors: vec![ApiMessage {
                    code: 20120,
                    message: "the field http.host is not available on this plan".into(),
                }],
            });
        }
        let key = (zone.to_owned(), phase.to_owned());
        let created = cf_api::Rule {
            id: rule_id,
            action: rule.action.clone(),
            expression: rule.expression.clone(),
            description: rule.description.clone(),
            enabled: rule.enabled,
            action_parameters: rule.action_parameters.clone(),
            ratelimit: rule.ratelimit.clone(),
        };
        let entry = match (ruleset, state.rulesets.get_mut(&key)) {
            (Some(id), Some(entry)) if entry.0 == id => entry,
            (None, None) => state
                .rulesets
                .entry(key)
                .or_insert((new_ruleset, Vec::new())),
            (None, Some(_)) => return Err(conflict()),
            (Some(_), _) => return Err(not_found()),
        };
        let at = index
            .and_then(|i| usize::try_from(i).ok())
            .map_or(entry.1.len(), |i| i.saturating_sub(1).min(entry.1.len()));
        entry.1.insert(at, created.clone());
        Ok((entry.0.clone(), created))
    }

    async fn update_rule(
        &self,
        zone: &str,
        ruleset: &str,
        rule_id: &str,
        rule: &cf_api::NewRule,
    ) -> cf_api::Result<cf_api::Rule> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let found = state
            .rulesets
            .iter_mut()
            .find(|((z, _), (id, _))| z == zone && id == ruleset)
            .and_then(|(_, (_, rules))| rules.iter_mut().find(|r| r.id == rule_id))
            .ok_or_else(not_found)?;
        found.action.clone_from(&rule.action);
        found.expression.clone_from(&rule.expression);
        found.description.clone_from(&rule.description);
        found.enabled = rule.enabled;
        found.action_parameters.clone_from(&rule.action_parameters);
        found.ratelimit.clone_from(&rule.ratelimit);
        Ok(found.clone())
    }

    async fn delete_rule(&self, zone: &str, ruleset: &str, rule_id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        if let Some((_, (_, rules))) = state
            .rulesets
            .iter_mut()
            .find(|((z, _), (id, _))| z == zone && id == ruleset)
        {
            rules.retain(|r| r.id != rule_id);
        }
        Ok(())
    }

    async fn service_tokens(&self, _account: &str) -> cf_api::Result<Vec<cf_api::ServiceToken>> {
        let state = self.state.lock().unwrap();
        if state.service_tokens_forbidden {
            return Err(forbidden());
        }
        Ok(state.service_tokens.values().cloned().collect())
    }

    async fn create_service_token(
        &self,
        _account: &str,
        name: &str,
        _duration: &str,
    ) -> cf_api::Result<cf_api::IssuedServiceToken> {
        self.mutate()?;
        let id = loop {
            let id = self.next_id("token");
            if !self.state.lock().unwrap().service_tokens.contains_key(&id) {
                break id;
            }
        };
        let token = cf_api::ServiceToken {
            client_id: format!("{id}.access"),
            id: id.clone(),
            name: name.to_owned(),
            expires_at: Some("2027-09-24T00:00:00Z".into()),
            created_at: None,
        };
        self.state
            .lock()
            .unwrap()
            .service_tokens
            .insert(id.clone(), token.clone());
        Ok(cf_api::IssuedServiceToken {
            token,
            client_secret: format!("secret-for-{id}"),
        })
    }

    async fn rotate_service_token(
        &self,
        _account: &str,
        id: &str,
    ) -> cf_api::Result<cf_api::IssuedServiceToken> {
        self.mutate()?;
        let n = self.next_id("rotation");
        let state = self.state.lock().unwrap();
        let token = state
            .service_tokens
            .get(id)
            .cloned()
            .ok_or_else(not_found)?;
        Ok(cf_api::IssuedServiceToken {
            token,
            client_secret: format!("secret-for-{id}-{n}"),
        })
    }

    async fn delete_service_token(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state.lock().unwrap().service_tokens.remove(id);
        Ok(())
    }

    async fn d1_databases(
        &self,
        _account: &str,
        name: &str,
    ) -> cf_api::Result<Vec<cf_api::D1Database>> {
        let state = self.state.lock().unwrap();
        if state.d1_forbidden {
            return Err(forbidden());
        }
        Ok(state
            .d1
            .iter()
            .filter(|(_, d)| d.name == name)
            .map(|(id, d)| cf_api::D1Database {
                uuid: id.clone(),
                name: d.name.clone(),
                created_at: None,
            })
            .collect())
    }

    async fn create_d1_database(
        &self,
        _account: &str,
        name: &str,
    ) -> cf_api::Result<cf_api::D1Database> {
        self.mutate()?;
        let id = self.next_id("d1");
        let mut state = self.state.lock().unwrap();
        if state.d1_forbidden {
            return Err(forbidden());
        }
        state.d1.insert(id.clone(), FakeD1::new(name));
        Ok(cf_api::D1Database {
            uuid: id,
            name: name.to_owned(),
            created_at: None,
        })
    }

    async fn delete_d1_database(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state.lock().unwrap().d1.remove(id);
        Ok(())
    }

    async fn d1_query(
        &self,
        _account: &str,
        database: &str,
        statements: &[cf_api::D1Statement],
    ) -> cf_api::Result<Vec<cf_api::D1Result>> {
        let db = {
            let state = self.state.lock().unwrap();
            if state.d1_forbidden {
                return Err(forbidden());
            }
            state.d1.get(database).cloned().ok_or_else(not_found)?
        };
        // Reads aren't counted as mutations; writes through the query endpoint are.
        if statements.iter().any(|s| {
            !s.sql
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("SELECT")
        }) {
            self.mutate()?;
        }
        db.run(statements)
    }

    async fn worker_routes(&self, zone: &str) -> cf_api::Result<Vec<cf_api::WorkerRoute>> {
        let state = self.state.lock().unwrap();
        if !state.zones.iter().any(|z| z.id == zone) {
            return Err(not_found());
        }
        Ok(state
            .worker_routes
            .values()
            .filter(|(z, _)| z == zone)
            .map(|(_, r)| r.clone())
            .collect())
    }

    async fn create_worker_route(
        &self,
        zone: &str,
        pattern: &str,
        script: &str,
    ) -> cf_api::Result<cf_api::WorkerRoute> {
        self.mutate()?;
        let id = self.next_id("wroute");
        let mut state = self.state.lock().unwrap();
        if !state.zones.iter().any(|z| z.id == zone) {
            return Err(not_found());
        }
        if state
            .worker_routes
            .values()
            .any(|(z, r)| z == zone && r.pattern == pattern)
        {
            return Err(conflict());
        }
        if !state.workers.contains_key(script) {
            return Err(not_found());
        }
        let route = cf_api::WorkerRoute {
            id: id.clone(),
            pattern: pattern.to_owned(),
            script: Some(script.to_owned()),
            request_limit_fail_open: Some(true),
        };
        state
            .worker_routes
            .insert(id, (zone.to_owned(), route.clone()));
        Ok(route)
    }

    async fn delete_worker_route(&self, _zone: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state.lock().unwrap().worker_routes.remove(id);
        Ok(())
    }
}

/// A version from script metadata; its assets token must be a completed upload.
fn fake_version(
    state: &CloudState,
    id: String,
    metadata: &serde_json::Value,
) -> cf_api::Result<FakeVersion> {
    if metadata.get("assets").is_none() {
        // A Worker without files (the offline page, the webhook inbox).
        let mut metadata = metadata.clone();
        metadata.as_object_mut().map(|m| m.remove("annotations"));
        return Ok(FakeVersion {
            id,
            metadata,
            assets: BTreeMap::new(),
        });
    }
    let jwt = metadata["assets"]["jwt"].as_str().unwrap_or_default();
    let Some((true, paths)) = state.asset_tokens.get(jwt) else {
        return Err(cf_api::Error::Api {
            status: 400,
            errors: vec![ApiMessage {
                code: 10021,
                message: "invalid assets token".into(),
            }],
        });
    };
    let mut metadata = metadata.clone();
    metadata["assets"].as_object_mut().map(|a| a.remove("jwt"));
    metadata.as_object_mut().map(|m| m.remove("annotations"));
    Ok(FakeVersion {
        id,
        metadata,
        assets: paths.clone(),
    })
}

fn fake_app(id: &str, app: &NewAccessApp) -> AccessApp {
    AccessApp {
        id: id.to_owned(),
        name: app.name.clone(),
        domain: app.domain.clone(),
        kind: app.kind.clone(),
        session_duration: Some(app.session_duration.clone()),
        policies: app
            .policies
            .iter()
            .enumerate()
            .map(|(i, p)| cf_api::AccessPolicy {
                id: Some(format!("{id}-policy-{i}")),
                ..p.clone()
            })
            .collect(),
        allowed_idps: app.allowed_idps.clone(),
        auto_redirect_to_identity: app.auto_redirect_to_identity,
    }
}

/// Records connector calls; `stop` can be made to fail.
#[derive(Debug, Default)]
pub(crate) struct FakeConnectors {
    pub(crate) calls: Mutex<Vec<String>>,
    pub(crate) running: Mutex<BTreeSet<String>>,
    pub(crate) fail_stop: AtomicBool,
    /// Connector ids of running connectors, by tunnel id.
    pub(crate) ids: Mutex<BTreeMap<String, String>>,
}

impl FakeConnectors {
    pub(crate) fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn call(&self, what: String) {
        self.calls.lock().unwrap().push(what);
    }
}

impl Connectors for FakeConnectors {
    async fn connector_id(&self, tunnel_id: &str) -> Option<String> {
        self.ids.lock().unwrap().get(tunnel_id).cloned()
    }

    fn state(&self, tunnel_id: &str) -> Option<ConnectorState> {
        self.running
            .lock()
            .unwrap()
            .contains(tunnel_id)
            .then_some(ConnectorState::Healthy { connections: 4 })
    }

    async fn start(
        &self,
        _account: &str,
        tunnel_id: &str,
        token: Secret<String>,
    ) -> Result<(), crate::text::Text> {
        assert_eq!(token.expose(), &format!("token-for-{tunnel_id}"));
        self.call(format!("start {tunnel_id}"));
        self.running.lock().unwrap().insert(tunnel_id.to_owned());
        Ok(())
    }

    async fn stop(&self, tunnel_id: &str) -> Result<(), crate::text::Text> {
        if self.fail_stop.load(Ordering::SeqCst) {
            return Err(crate::text::msg::raw("injected stop failure"));
        }
        self.call(format!("stop {tunnel_id}"));
        self.running.lock().unwrap().remove(tunnel_id);
        Ok(())
    }

    async fn deleted(&self, tunnel_id: &str) {
        self.call(format!("deleted {tunnel_id}"));
    }
}
