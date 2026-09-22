//! In-memory Cloudflare and connectors for executor tests, with failure injection.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use cf_api::{ApiMessage, DnsRecord, NewDnsRecord, Tunnel, TunnelConfig, VersionedConfig};

use super::{
    cloud::{CloudApi, Connectors},
    types::ZoneRef,
};
use crate::Secret;

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
}

/// State with ids and versions stripped, for "is it back to how it was?" checks.
pub(crate) type Normalized = (
    Vec<(String, Option<serde_json::Value>)>,
    Vec<(String, String, String, String, bool, u32, Option<String>)>,
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
        (tunnels, records)
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

    async fn tunnel_names(&self, _account: &str) -> cf_api::Result<Vec<String>> {
        let state = self.state.lock().unwrap();
        Ok(state.tunnels.values().map(|t| t.name.clone()).collect())
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

    async fn create_record(&self, zone: &str, record: &NewDnsRecord) -> cf_api::Result<DnsRecord> {
        self.mutate()?;
        let id = self.next_id("rec");
        self.with_zone(zone, |records| {
            let clash = records.iter().any(|r| {
                r.name.eq_ignore_ascii_case(&record.name)
                    && (r.kind == "CNAME" || record.kind == "CNAME")
            });
            if clash {
                return Err(cf_api::Error::Api {
                    status: 400,
                    errors: vec![ApiMessage {
                        code: 81053,
                        message: "An A, AAAA, or CNAME record with that host already exists."
                            .into(),
                    }],
                });
            }
            let created = DnsRecord {
                id,
                name: record.name.clone(),
                kind: record.kind.clone(),
                content: record.content.clone(),
                proxied: record.proxied,
                comment: record.comment.clone(),
                ttl: record.ttl,
            };
            records.push(created.clone());
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
            r.name.clone_from(&record.name);
            r.kind.clone_from(&record.kind);
            r.content.clone_from(&record.content);
            r.proxied = record.proxied;
            r.ttl = record.ttl;
            r.comment.clone_from(&record.comment);
            Ok(r.clone())
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
}

/// Records connector calls; `stop` can be made to fail.
#[derive(Debug, Default)]
pub(crate) struct FakeConnectors {
    pub(crate) calls: Mutex<Vec<String>>,
    pub(crate) running: Mutex<BTreeSet<String>>,
    pub(crate) fail_stop: AtomicBool,
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
    fn is_running(&self, tunnel_id: &str) -> bool {
        self.running.lock().unwrap().contains(tunnel_id)
    }

    async fn start(
        &self,
        _account: &str,
        tunnel_id: &str,
        token: Secret<String>,
    ) -> Result<(), String> {
        assert_eq!(token.expose(), &format!("token-for-{tunnel_id}"));
        self.call(format!("start {tunnel_id}"));
        self.running.lock().unwrap().insert(tunnel_id.to_owned());
        Ok(())
    }

    async fn stop(&self, tunnel_id: &str) -> Result<(), String> {
        if self.fail_stop.load(Ordering::SeqCst) {
            return Err("injected stop failure".into());
        }
        self.call(format!("stop {tunnel_id}"));
        self.running.lock().unwrap().remove(tunnel_id);
        Ok(())
    }

    async fn deleted(&self, tunnel_id: &str) {
        self.call(format!("deleted {tunnel_id}"));
    }
}
