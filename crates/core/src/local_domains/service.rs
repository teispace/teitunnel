//! The running side of local domains: one HTTPS and one plain HTTP listener in the
//! process's Lens (through its [`Inspector`]), a hosted tap per domain, the `.test` name
//! server, `.local` advertisements, and upkeep (renewal, sleep/wake, network changes).

use std::{
    collections::{HashMap, HashSet},
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, PoisonError, RwLock},
    time::{Duration, SystemTime},
};

use localdomains::{
    CaCert, CaIdentity, CaKeyStore, Clock, DnsResponder, DnsZone, DomainRegistry, FileKeyStore,
    InstallOptions, LeafCache, LocalCa, LocalDomain, LocalName, MemoryKeyStore, Platform,
    PrivilegedAction, SniResolver, Suffix, SystemClock, mdns::MdnsAdvertiser, resolve_check,
    resolver_config,
};
use time::Duration as TimeDuration;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::{
    LocalDomainError,
    acceptor::{CheckedPlain, CheckedTls, PeerPolicy, lan_addresses},
    model::{
        CaView, LocalDomainInput, LocalDomainRow, LocalDomainSettings, LocalDomainView,
        LocalDomainsStatus, LocalTarget, NameResolution, PortProblem, PortReason, PrivilegedStep,
        ResolverView, TrustOptions, TrustStoreView, TrustView, origin_of, parse_target,
    },
    registry,
    trust::{FileTrust, SystemTrust, TrustBackend, system_trusted},
};
use crate::{
    inspect::{
        InspectError, Inspector, TapScope, TapSpec,
        lens::{LensError, Limits, ListenOptions, ListenerInfo, Routing, TapId},
    },
    secrets::Secrets,
    store::Store,
    text::{Text, UserText},
};

/// A CA this close to expiry is replaced (and must be trusted again).
const CA_RENEW_WITHIN: TimeDuration = TimeDuration::days(30);
/// How often the upkeep runs.
const TICK: Duration = Duration::from_secs(30);
/// A gap this long between ticks means the computer slept.
const WAKE_GAP: Duration = Duration::from_secs(90);
/// How often leaves are renewed without a wake.
const RENEW_EVERY: Duration = Duration::from_secs(60 * 60);
/// How long a name lookup may take in a status check.
const RESOLVE_TIMEOUT: Duration = Duration::from_millis(800);

/// Ports and features. [`LocalDomainsOptions::from_env`] is what the app and CLI use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDomainsOptions {
    /// HTTPS port wanted (443); 0 picks a free one.
    pub https_port: u16,
    /// HTTPS port when the first can't be used (8443).
    pub fallback_https_port: u16,
    /// Plain HTTP port wanted (80); 0 picks a free one.
    pub http_port: u16,
    /// Plain HTTP port when the first can't be used (8080).
    pub fallback_http_port: u16,
    /// The `.test` name server's port on `127.0.0.1` (53535; 53 on Windows).
    pub dns_port: u16,
    /// Run the `.test` name server.
    pub dns: bool,
    /// Advertise `.local` names with multicast DNS.
    pub mdns: bool,
    /// Ask the system resolver whether names reach this computer (status checks).
    pub check_resolution: bool,
}

fn env_port(name: &str) -> Option<u16> {
    std::env::var(name).ok()?.trim().parse().ok()
}

impl LocalDomainsOptions {
    /// The defaults, with `TEITUNNEL_LOCAL_HTTPS_PORT`, `TEITUNNEL_LOCAL_HTTP_PORT` and
    /// `TEITUNNEL_LOCAL_DNS_PORT` taking over when set (then there's no fallback port).
    pub fn from_env() -> Self {
        let https = env_port("TEITUNNEL_LOCAL_HTTPS_PORT");
        let http = env_port("TEITUNNEL_LOCAL_HTTP_PORT");
        let windows = Platform::current() == Platform::Windows;
        Self {
            https_port: https.unwrap_or(443),
            fallback_https_port: https.unwrap_or(8443),
            http_port: http.unwrap_or(80),
            fallback_http_port: http.unwrap_or(8080),
            dns_port: env_port("TEITUNNEL_LOCAL_DNS_PORT").unwrap_or(if windows {
                53
            } else {
                localdomains::dns::DEFAULT_PORT
            }),
            dns: true,
            mdns: std::env::var_os("TEITUNNEL_LOCAL_NO_MDNS").is_none(),
            check_resolution: true,
        }
    }

    /// Free ports on loopback, no multicast DNS, no system lookups: for tests.
    pub fn isolated() -> Self {
        Self {
            https_port: 0,
            fallback_https_port: 0,
            http_port: 0,
            fallback_http_port: 0,
            dns_port: 0,
            dns: true,
            mdns: false,
            check_resolution: false,
        }
    }
}

/// Where the CA key lives, who trusts it, and the ports.
#[derive(Debug, Clone)]
pub struct LocalDomainsConfig {
    /// Teitunnel's folder for local domains (the CA certificate, staged files).
    pub dir: PathBuf,
    /// The CA key's store.
    pub keys: Arc<dyn CaKeyStore>,
    /// The trust stores.
    pub trust: Arc<dyn TrustBackend>,
    /// Ports and features.
    pub options: LocalDomainsOptions,
    /// Time, for certificates.
    pub clock: Arc<dyn Clock>,
}

impl LocalDomainsConfig {
    /// The real setup under `data_dir`: the key in the keychain (`secrets`), the OS trust
    /// stores. `TEITUNNEL_LOCAL_CA_FILE` keeps the key in a plain file instead (servers
    /// without a keychain), and `TEITUNNEL_TEST_TRUST_FILE` replaces the trust stores
    /// with a file (end-to-end tests).
    pub fn detect(data_dir: &Path, secrets: Option<Secrets>) -> Self {
        let dir = data_dir.join("localdomains");
        let keys: Arc<dyn CaKeyStore> = match std::env::var_os("TEITUNNEL_LOCAL_CA_FILE") {
            Some(path) => Arc::new(FileKeyStore::new_unencrypted(PathBuf::from(path))),
            None => match secrets {
                Some(secrets) => Arc::new(super::keys::KeychainCaStore::new(secrets)),
                None => Arc::new(MemoryKeyStore::default()),
            },
        };
        let trust: Arc<dyn TrustBackend> = match std::env::var_os("TEITUNNEL_TEST_TRUST_FILE") {
            Some(path) => Arc::new(FileTrust::new(PathBuf::from(path))),
            None => match SystemTrust::detect(dir.clone()) {
                Some(system) => Arc::new(system),
                None => Arc::new(FileTrust::in_memory()),
            },
        };
        Self {
            dir,
            keys,
            trust,
            options: LocalDomainsOptions::from_env(),
            clock: Arc::new(SystemClock),
        }
    }

    /// Everything in memory and on free ports (tests).
    pub fn isolated(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            keys: Arc::new(MemoryKeyStore::default()),
            trust: Arc::new(FileTrust::in_memory()),
            options: LocalDomainsOptions::isolated(),
            clock: Arc::new(SystemClock),
        }
    }
}

/// A listener pair's facts.
#[derive(Debug, Clone)]
struct Listeners {
    https: Vec<ListenerInfo>,
    http: Vec<ListenerInfo>,
    https_port: Option<u16>,
    http_port: Option<u16>,
    wildcard: bool,
}

#[derive(Debug, Clone)]
struct TapState {
    id: TapId,
    origin: String,
    inspect: bool,
}

#[derive(Debug, Default)]
struct Runtime {
    listeners: Option<Listeners>,
    taps: HashMap<LocalName, TapState>,
    registry: Option<Arc<RwLock<DomainRegistry>>>,
    leaves: Option<Arc<LeafCache>>,
    zone: Arc<DnsZone>,
    dns: Option<DnsResponder>,
    dns_error: Option<Text>,
    mdns: Option<MdnsAdvertiser>,
    advertised: HashSet<LocalName>,
    error: Option<Text>,
    port_problems: Vec<PortProblem>,
}

#[derive(Debug)]
struct Inner {
    store: Store,
    inspector: Inspector,
    config: LocalDomainsConfig,
    policy: PeerPolicy,
    ca: Mutex<Option<Arc<LocalCa>>>,
    runtime: tokio::sync::Mutex<Runtime>,
    events: broadcast::Sender<()>,
    stop: CancellationToken,
}

/// Local HTTPS domains in this process. Cheap to clone.
#[derive(Debug, Clone)]
pub struct LocalDomains {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// `https://name[:port]` (or `http://`), leaving out the scheme's default port.
fn url_for(name: &LocalName, https: bool, port: u16) -> String {
    match (https, port) {
        (true, 443 | 0) => format!("https://{name}"),
        (true, port) => format!("https://{name}:{port}"),
        (false, 80 | 0) => format!("http://{name}"),
        (false, port) => format!("http://{name}:{port}"),
    }
}

fn step(action: &PrivilegedAction, platform: Platform) -> PrivilegedStep {
    PrivilegedStep {
        command: match (platform, action) {
            (Platform::Windows, PrivilegedAction::Run(invocation)) => invocation.display_windows(),
            _ => action.copyable_posix(),
        },
    }
}

fn steps(actions: &[PrivilegedAction], platform: Platform) -> Vec<PrivilegedStep> {
    actions.iter().map(|a| step(a, platform)).collect()
}

fn ca_view(ca: &LocalCa) -> CaView {
    CaView {
        common_name: ca.common_name().to_owned(),
        sha256: ca.sha256_fingerprint(),
        not_after: ca.not_after().unix_timestamp(),
    }
}

fn reason_of(err: &InspectError) -> PortReason {
    match err {
        InspectError::Lens(LensError::Bind { source, .. }) => match source.kind() {
            io::ErrorKind::AddrInUse => PortReason::InUse,
            io::ErrorKind::PermissionDenied => PortReason::PermissionDenied,
            _ => PortReason::Other,
        },
        _ => PortReason::Other,
    }
}

impl LocalDomains {
    /// Local domains for this process, over its database and inspector.
    pub fn new(store: Store, inspector: Inspector, config: LocalDomainsConfig) -> Self {
        let (events, _) = broadcast::channel(32);
        let policy = PeerPolicy::default();
        policy.refresh();
        Self {
            inner: Arc::new(Inner {
                store,
                inspector,
                config,
                policy,
                ca: Mutex::new(None),
                runtime: tokio::sync::Mutex::new(Runtime::default()),
                events,
                stop: CancellationToken::new(),
            }),
        }
    }

    /// Something changed (domains, listeners, trust): re-read the status.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.inner.events.subscribe()
    }

    fn changed(&self) {
        let _ = self.inner.events.send(());
    }

    /// The platform the trust steps are for.
    pub fn platform(&self) -> Platform {
        self.inner.config.trust.platform()
    }

    /// The database.
    pub fn store(&self) -> &Store {
        &self.inner.store
    }

    fn ca_path(&self) -> PathBuf {
        self.inner.config.dir.join("ca.pem")
    }

    /// The CA, loaded (or created) once. Creating one writes its key to the store.
    async fn ensure_ca(&self) -> Result<Arc<LocalCa>, LocalDomainError> {
        if let Some(ca) = lock(&self.inner.ca).clone() {
            return Ok(ca);
        }
        let keys = Arc::clone(&self.inner.config.keys);
        let clock = Arc::clone(&self.inner.config.clock);
        let (ca, created) = tokio::task::spawn_blocking(move || {
            LocalCa::load_or_create(&*keys, &CaIdentity::current(), &*clock, CA_RENEW_WITHIN)
        })
        .await
        .map_err(|e| LocalDomainError::Ca(e.to_string()))??;
        if created {
            tracing::info!(ca = %ca.common_name(), "created the local certificate authority");
        }
        ca.write_cert_pem(&self.ca_path())?;
        let ca = Arc::new(ca);
        *lock(&self.inner.ca) = Some(Arc::clone(&ca));
        Ok(ca)
    }

    /// The CA if one exists (never creates one).
    async fn existing_ca(&self) -> Option<Arc<LocalCa>> {
        if let Some(ca) = lock(&self.inner.ca).clone() {
            return Some(ca);
        }
        let keys = Arc::clone(&self.inner.config.keys);
        let loaded = tokio::task::spawn_blocking(move || {
            keys.load()
                .ok()
                .flatten()
                .and_then(|secret| LocalCa::from_secret(&secret).ok())
        })
        .await
        .ok()
        .flatten()?;
        let _ = loaded.write_cert_pem(&self.ca_path());
        let ca = Arc::new(loaded);
        *lock(&self.inner.ca) = Some(Arc::clone(&ca));
        Some(ca)
    }

    fn cert(&self, ca: &LocalCa) -> CaCert {
        CaCert::from_ca(ca, self.ca_path())
    }

    /// Adds a local domain and serves it.
    ///
    /// # Errors
    /// An invalid name or target, a duplicate, or the database; serving problems show in
    /// the status instead.
    pub async fn add(&self, input: &LocalDomainInput) -> Result<LocalDomainView, LocalDomainError> {
        let row = self.save_new(input).await?;
        self.sync_logged().await;
        self.view(&row.name).await
    }

    /// Checks and saves a new local domain without serving it here (the CLI hands it to
    /// the running app, or serves it itself).
    ///
    /// # Errors
    /// An invalid name or target, a duplicate, or the database.
    pub async fn save_new(
        &self,
        input: &LocalDomainInput,
    ) -> Result<LocalDomainRow, LocalDomainError> {
        let name = LocalName::parse_any(&input.name)?;
        if registry::get(&self.inner.store, &name).await?.is_some() {
            return Err(LocalDomainError::Duplicate(name.to_string()));
        }
        let row = LocalDomainRow {
            name: name.clone(),
            target: parse_target(&input.target)?,
            wildcard: input.wildcard,
            https: input.https,
            inspect: input.inspect,
            project: None,
            created_at: now_secs(),
        };
        self.validate(&row).await?;
        registry::save(&self.inner.store, &row).await?;
        Ok(row)
    }

    /// Changes a local domain (same name).
    ///
    /// # Errors
    /// Unknown name, invalid target, or the database.
    pub async fn update(
        &self,
        input: &LocalDomainInput,
    ) -> Result<LocalDomainView, LocalDomainError> {
        let name = LocalName::parse_any(&input.name)?;
        let Some(mut row) = registry::get(&self.inner.store, &name).await? else {
            return Err(LocalDomainError::NotFound(name.to_string()));
        };
        row.target = parse_target(&input.target)?;
        row.wildcard = input.wildcard;
        row.https = input.https;
        row.inspect = input.inspect;
        self.validate(&row).await?;
        registry::save(&self.inner.store, &row).await?;
        self.sync_logged().await;
        self.view(&name).await
    }

    /// Records a local domain's requests in the inspector, or stops.
    ///
    /// # Errors
    /// Unknown name, or the database.
    pub async fn set_inspect(
        &self,
        name: &str,
        inspect: bool,
    ) -> Result<LocalDomainView, LocalDomainError> {
        let name = LocalName::parse_any(name)?;
        let Some(mut row) = registry::get(&self.inner.store, &name).await? else {
            return Err(LocalDomainError::NotFound(name.to_string()));
        };
        row.inspect = inspect;
        registry::save(&self.inner.store, &row).await?;
        self.sync_logged().await;
        self.view(&name).await
    }

    async fn validate(&self, row: &LocalDomainRow) -> Result<(), LocalDomainError> {
        let others: Vec<LocalDomain> = registry::list(&self.inner.store)
            .await?
            .into_iter()
            .filter(|r| r.name != row.name)
            .map(|r| domain_of(&r))
            .collect();
        let registry = DomainRegistry::from_domains(&Suffix::ALL, others)?;
        registry.validate(&domain_of(row))?;
        Ok(())
    }

    /// Removes a local domain (its captures stay in the inspector's history).
    ///
    /// # Errors
    /// Unknown name, or the database.
    pub async fn remove(&self, name: &str) -> Result<(), LocalDomainError> {
        self.forget(name).await?;
        self.sync_logged().await;
        Ok(())
    }

    /// Deletes a local domain from the database without touching what this process
    /// serves (the CLI, before asking the app to reload).
    ///
    /// # Errors
    /// Unknown name, or the database.
    pub async fn forget(&self, name: &str) -> Result<(), LocalDomainError> {
        let name = LocalName::parse_any(name)?;
        if !registry::remove(&self.inner.store, &name).await? {
            return Err(LocalDomainError::NotFound(name.to_string()));
        }
        Ok(())
    }

    /// Lets phones and computers on the network open `.local` names, or stops that.
    ///
    /// # Errors
    /// The database.
    pub async fn set_lan(&self, lan: bool) -> Result<(), LocalDomainError> {
        registry::save_settings(&self.inner.store, LocalDomainSettings { lan }).await?;
        self.sync_logged().await;
        Ok(())
    }

    /// Serves what's in the database (starts, updates or stops the listeners). Call at
    /// launch, and after something else changed the database (a project, the CLI).
    ///
    /// # Errors
    /// Why the domains can't be served (also kept in the status).
    pub async fn sync(&self) -> Result<(), LocalDomainError> {
        let rows = registry::list(&self.inner.store).await?;
        let settings = registry::settings(&self.inner.store).await?;
        self.inner.policy.set_lan(settings.lan);
        let mut rt = self.inner.runtime.lock().await;
        let result = if rows.is_empty() {
            self.shut_down(&mut rt).await;
            Ok(())
        } else {
            self.reconcile(&mut rt, &rows, settings).await
        };
        rt.error = result.as_ref().err().map(UserText::text);
        drop(rt);
        self.changed();
        result
    }

    async fn sync_logged(&self) {
        if let Err(err) = self.sync().await {
            tracing::warn!(%err, "local domains couldn't be served");
        }
    }

    /// Stops serving (the domains stay in the database). Call when the process quits.
    pub async fn stop(&self) {
        self.inner.stop.cancel();
        let mut rt = self.inner.runtime.lock().await;
        self.shut_down(&mut rt).await;
    }

    async fn shut_down(&self, rt: &mut Runtime) {
        if let Some(listeners) = rt.listeners.take() {
            for info in listeners.https.iter().chain(&listeners.http) {
                self.inner.inspector.close_listener(info.id).await;
            }
        }
        for (_, tap) in rt.taps.drain() {
            self.inner.inspector.stop(&tap.id).await;
        }
        if let Some(dns) = rt.dns.take() {
            dns.shutdown().await;
        }
        if let Some(mdns) = rt.mdns.take()
            && let Err(err) = mdns.shutdown()
        {
            tracing::debug!(%err, "mDNS didn't stop cleanly");
        }
        rt.advertised.clear();
        rt.registry = None;
        rt.leaves = None;
        rt.port_problems.clear();
        rt.dns_error = None;
    }

    async fn reconcile(
        &self,
        rt: &mut Runtime,
        rows: &[LocalDomainRow],
        settings: LocalDomainSettings,
    ) -> Result<(), LocalDomainError> {
        let any_https = rows.iter().any(|r| r.https);
        // Certificates.
        if any_https && rt.leaves.is_none() {
            let ca = self.ensure_ca().await?;
            rt.leaves = Some(Arc::new(LeafCache::new(
                ca,
                Arc::clone(&self.inner.config.clock),
            )));
        }
        let https_domains: Vec<LocalDomain> =
            rows.iter().filter(|r| r.https).map(domain_of).collect();
        let registry = DomainRegistry::from_domains(&Suffix::ALL, https_domains)?;
        match &rt.registry {
            Some(shared) => {
                *shared.write().unwrap_or_else(PoisonError::into_inner) = registry;
            }
            None => rt.registry = Some(Arc::new(RwLock::new(registry))),
        }

        // Listeners.
        let wildcard = settings.lan || rows.iter().any(|r| r.name.suffix() == Suffix::Local);
        let stale = rt.listeners.as_ref().is_some_and(|l| {
            (wildcard && !l.wildcard)
                || (any_https && l.https.is_empty())
                || l.https
                    .iter()
                    .chain(&l.http)
                    .any(|info| !self.inner.inspector.is_listening(info.id))
        });
        if stale && let Some(old) = rt.listeners.take() {
            for info in old.https.iter().chain(&old.http) {
                self.inner.inspector.close_listener(info.id).await;
            }
        }
        if rt.listeners.is_none() {
            rt.port_problems.clear();
            let listeners = self.bind(rt, wildcard).await?;
            rt.listeners = Some(listeners);
        }
        let Some(listeners) = rt.listeners.clone() else {
            return Ok(());
        };

        // Taps.
        let wanted: HashSet<&LocalName> = rows.iter().map(|r| &r.name).collect();
        let gone: Vec<LocalName> = rt
            .taps
            .keys()
            .filter(|name| !wanted.contains(name))
            .cloned()
            .collect();
        for name in gone {
            if let Some(tap) = rt.taps.remove(&name) {
                self.inner.inspector.stop(&tap.id).await;
            }
        }
        for row in rows {
            let Some(origin) = origin_of(&row.target) else {
                continue;
            };
            let current = rt.taps.get(&row.name).cloned();
            match current {
                Some(tap) if tap.origin == origin && self.inner.inspector.view(&tap.id).is_ok() => {
                    if tap.inspect != row.inspect {
                        self.inner.inspector.set_capture(&tap.id, row.inspect)?;
                        if let Some(entry) = rt.taps.get_mut(&row.name) {
                            entry.inspect = row.inspect;
                        }
                    }
                }
                _ => {
                    let port = if row.https {
                        listeners.https_port
                    } else {
                        listeners.http_port
                    };
                    let url = url_for(&row.name, row.https, port.unwrap_or(0));
                    let mut spec = TapSpec::new(
                        TapScope::LocalDomain {
                            name: row.name.to_string(),
                        },
                        row.name.as_str(),
                        &origin,
                    );
                    spec.public_url = Some(url.clone());
                    let id = self
                        .inner
                        .inspector
                        .start_hosted(spec, url, row.inspect)
                        .await?;
                    rt.taps.insert(
                        row.name.clone(),
                        TapState {
                            id,
                            origin,
                            inspect: row.inspect,
                        },
                    );
                }
            }
        }

        // Routing.
        let mut secure = Vec::new();
        let mut plain = Vec::new();
        let mut redirect = Vec::new();
        for row in rows {
            let names = std::iter::once(row.name.to_string())
                .chain(row.wildcard.then(|| row.name.wildcard()));
            let tap = rt.taps.get(&row.name).map(|t| t.id.clone());
            for name in names {
                match (&tap, row.https) {
                    (Some(tap), true) => {
                        secure.push((name.clone(), tap.clone()));
                        redirect.push(name);
                    }
                    (Some(tap), false) => plain.push((name, tap.clone())),
                    (None, _) => {}
                }
            }
        }
        for info in &listeners.https {
            self.inner.inspector.set_routing(
                info.id,
                Routing::Hosts {
                    hosts: secure.clone(),
                    fallback: None,
                },
            )?;
        }
        for info in &listeners.http {
            self.inner.inspector.set_routing(
                info.id,
                Routing::HttpsRedirect {
                    hosts: plain.clone(),
                    redirect: redirect.clone(),
                    https_port: listeners.https_port,
                },
            )?;
        }

        // `.test` names.
        rt.zone.set(
            rows.iter()
                .filter(|r| r.name.suffix() == Suffix::Test)
                .map(|r| (r.name.clone(), r.wildcard)),
        );
        let wants_dns =
            self.inner.config.options.dns && rows.iter().any(|r| r.name.suffix() == Suffix::Test);
        if wants_dns && rt.dns.is_none() {
            let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.inner.config.options.dns_port));
            match DnsResponder::start(addr, Arc::clone(&rt.zone)).await {
                Ok(dns) => {
                    rt.dns = Some(dns);
                    rt.dns_error = None;
                }
                Err(err) => {
                    tracing::warn!(%err, "the .test name server couldn't start");
                    rt.dns_error = Some(super::m::error::dns(&err));
                }
            }
        } else if !wants_dns && let Some(dns) = rt.dns.take() {
            dns.shutdown().await;
            rt.dns_error = None;
        }

        // `.local` names.
        self.advertise(rt, rows, &listeners);
        Ok(())
    }

    fn advertise(&self, rt: &mut Runtime, rows: &[LocalDomainRow], listeners: &Listeners) {
        let local: Vec<&LocalDomainRow> = rows
            .iter()
            .filter(|r| r.name.suffix() == Suffix::Local)
            .collect();
        if !self.inner.config.options.mdns || local.is_empty() {
            if let Some(mdns) = rt.mdns.take() {
                let _ = mdns.shutdown();
            }
            rt.advertised.clear();
            return;
        }
        if rt.mdns.is_none() {
            match MdnsAdvertiser::new() {
                Ok(mdns) => rt.mdns = Some(mdns),
                Err(err) => {
                    tracing::warn!(%err, "multicast DNS couldn't start");
                    return;
                }
            }
        }
        let Some(mdns) = rt.mdns.as_mut() else {
            return;
        };
        let wanted: HashSet<LocalName> = local.iter().map(|r| r.name.clone()).collect();
        for name in rt
            .advertised
            .difference(&wanted)
            .cloned()
            .collect::<Vec<_>>()
        {
            let _ = mdns.withdraw(&name);
            rt.advertised.remove(&name);
        }
        for row in local {
            let port = if row.https {
                listeners.https_port
            } else {
                listeners.http_port
            };
            let Some(port) = port else { continue };
            match mdns.advertise(&row.name, &[], port) {
                Ok(()) => {
                    rt.advertised.insert(row.name.clone());
                }
                Err(err) => tracing::warn!(%err, name = %row.name, "couldn't advertise"),
            }
        }
    }

    /// Binds the HTTPS listener (when a domain uses HTTPS) and the plain one.
    async fn bind(&self, rt: &mut Runtime, wildcard: bool) -> Result<Listeners, LocalDomainError> {
        let options = &self.inner.config.options;
        let policy = self.inner.policy.clone();
        let mut out = Listeners {
            https: Vec::new(),
            http: Vec::new(),
            https_port: None,
            http_port: None,
            wildcard,
        };
        if let (Some(leaves), Some(registry)) = (&rt.leaves, &rt.registry) {
            let policy_for_certs: Arc<dyn localdomains::CertPolicy> = registry.clone();
            let resolver = Arc::new(SniResolver::new(policy_for_certs, Arc::clone(leaves)));
            let config = localdomains::server_config(resolver)
                .map_err(|e| LocalDomainError::Ca(e.to_string()))?;
            let acceptor = Arc::new(CheckedTls {
                policy: policy.clone(),
                tls: tokio_rustls::TlsAcceptor::from(Arc::new(config)),
            });
            let (infos, port, problem) = self
                .bind_port(
                    options.https_port,
                    options.fallback_https_port,
                    wildcard,
                    acceptor,
                )
                .await;
            rt.port_problems.extend(problem);
            let Some(port) = port else {
                return Err(LocalDomainError::NoPort {
                    port: options.https_port,
                    fallback: options.fallback_https_port,
                });
            };
            out.https = infos;
            out.https_port = Some(port);
        }
        let acceptor = Arc::new(CheckedPlain { policy });
        let (infos, port, problem) = self
            .bind_port(
                options.http_port,
                options.fallback_http_port,
                wildcard,
                acceptor,
            )
            .await;
        rt.port_problems.extend(problem);
        out.http = infos;
        out.http_port = port;
        Ok(out)
    }

    /// Binds `preferred` (then `fallback`): loopback, or the wildcard address when asked
    /// or when the system allows the port only there (macOS below 1024).
    async fn bind_port(
        &self,
        preferred: u16,
        fallback: u16,
        wildcard: bool,
        acceptor: Arc<dyn crate::inspect::lens::Acceptor>,
    ) -> (Vec<ListenerInfo>, Option<u16>, Option<PortProblem>) {
        let mut first_failure: Option<PortReason> = None;
        let mut candidates = vec![preferred];
        if fallback != preferred {
            candidates.push(fallback);
        }
        for port in candidates {
            let modes: &[bool] = if wildcard { &[true] } else { &[false, true] };
            for &on_wildcard in modes {
                match self
                    .listen_on(port, on_wildcard, Arc::clone(&acceptor))
                    .await
                {
                    Ok((infos, bound)) => {
                        let problem = first_failure.map(|reason| PortProblem {
                            port: preferred,
                            reason,
                            fallback: Some(bound),
                        });
                        return (infos, Some(bound), problem);
                    }
                    Err(reason) => {
                        first_failure.get_or_insert(reason);
                        // Only a refused loopback bind is worth a try on the wildcard.
                        if reason != PortReason::PermissionDenied {
                            break;
                        }
                    }
                }
            }
        }
        let problem = first_failure.map(|reason| PortProblem {
            port: preferred,
            reason,
            fallback: None,
        });
        (Vec::new(), None, problem)
    }

    async fn listen_on(
        &self,
        port: u16,
        wildcard: bool,
        acceptor: Arc<dyn crate::inspect::lens::Acceptor>,
    ) -> Result<(Vec<ListenerInfo>, u16), PortReason> {
        let (primary, secondary): (IpAddr, IpAddr) = if wildcard {
            (Ipv6Addr::UNSPECIFIED.into(), Ipv4Addr::UNSPECIFIED.into())
        } else {
            (Ipv4Addr::LOCALHOST.into(), Ipv6Addr::LOCALHOST.into())
        };
        let options = |ip: IpAddr, port: u16| ListenOptions {
            addr: SocketAddr::new(ip, port),
            allow_non_loopback: wildcard,
            routing: Routing::Hosts {
                hosts: Vec::new(),
                fallback: None,
            },
            acceptor: Arc::clone(&acceptor),
            limits: Limits::default(),
        };
        let first = match self.inner.inspector.listen(options(primary, port)).await {
            Ok(info) => info,
            // No IPv6 at all: the IPv4 wildcard alone.
            Err(err) if wildcard && reason_of(&err) == PortReason::Other => self
                .inner
                .inspector
                .listen(options(secondary, port))
                .await
                .map_err(|e| reason_of(&e))?,
            Err(err) => return Err(reason_of(&err)),
        };
        let bound = first.addr.port();
        let mut infos = vec![first];
        // The other family on the same port; a dual-stack wildcard already covers it.
        if first.addr.ip() == primary
            && let Ok(info) = self.inner.inspector.listen(options(secondary, bound)).await
        {
            infos.push(info);
        }
        Ok((infos, bound))
    }

    /// Checks the listeners, renews certificates and refreshes network facts until
    /// [`LocalDomains::stop`]. Spawn it once per process.
    pub async fn run(self) {
        let mut last_tick = SystemTime::now();
        let mut last_renew = SystemTime::now();
        loop {
            tokio::select! {
                () = self.inner.stop.cancelled() => break,
                () = tokio::time::sleep(TICK) => {}
            }
            let now = SystemTime::now();
            let woke = now
                .duration_since(last_tick)
                .is_ok_and(|gap| gap > WAKE_GAP);
            last_tick = now;
            let renew = woke
                || now
                    .duration_since(last_renew)
                    .is_ok_and(|gap| gap > RENEW_EVERY);
            if renew {
                last_renew = now;
            }
            self.upkeep(woke, renew).await;
        }
    }

    /// One round of upkeep (public for tests and callers that manage their own timer).
    pub async fn upkeep(&self, woke: bool, renew: bool) {
        self.inner.policy.refresh();
        let (needs_sync, leaves) = {
            let rt = self.inner.runtime.lock().await;
            let dead = rt.listeners.as_ref().is_some_and(|l| {
                l.https
                    .iter()
                    .chain(&l.http)
                    .any(|info| !self.inner.inspector.is_listening(info.id))
            });
            (dead || woke, rt.leaves.clone())
        };
        if renew && let Some(leaves) = leaves {
            match tokio::task::spawn_blocking(move || leaves.renew_due()).await {
                Ok(Ok(0)) | Err(_) => {}
                Ok(Ok(count)) => tracing::info!(count, "renewed local certificates"),
                Ok(Err(err)) => tracing::warn!(%err, "couldn't renew local certificates"),
            }
        }
        let has_domains = registry::list(&self.inner.store)
            .await
            .is_ok_and(|rows| !rows.is_empty());
        let running = self.inner.runtime.lock().await.listeners.is_some();
        let restart = has_domains && (needs_sync || !running);
        let leftover = !has_domains && running;
        if restart || leftover {
            if woke {
                // Network addresses may have changed: advertise again.
                let mut rt = self.inner.runtime.lock().await;
                if let Some(mdns) = rt.mdns.take() {
                    let _ = mdns.shutdown();
                }
                rt.advertised.clear();
            }
            self.sync_logged().await;
        }
    }

    /// Renews every certificate due, now.
    ///
    /// # Errors
    /// Issuing failed.
    pub async fn renew(&self) -> Result<usize, LocalDomainError> {
        let leaves = self.inner.runtime.lock().await.leaves.clone();
        let Some(leaves) = leaves else { return Ok(0) };
        tokio::task::spawn_blocking(move || leaves.renew_due())
            .await
            .map_err(|e| LocalDomainError::Ca(e.to_string()))?
            .map_err(|e| LocalDomainError::Ca(e.to_string()))
    }

    /// When a domain's current certificate expires, if one was issued.
    pub async fn certificate_expiry(&self, name: &str) -> Option<i64> {
        let rt = self.inner.runtime.lock().await;
        let registry = rt.registry.as_ref()?;
        let request = registry
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .cert_request(name)?;
        rt.leaves
            .as_ref()?
            .not_after(&request)
            .map(time::OffsetDateTime::unix_timestamp)
    }

    async fn resolution(&self, name: &LocalName) -> NameResolution {
        if !self.inner.config.options.check_resolution || name.suffix() == Suffix::Local {
            return NameResolution::Unchecked;
        }
        let lookup = resolve_check::resolves_locally(name.as_str());
        match tokio::time::timeout(RESOLVE_TIMEOUT, lookup).await {
            Ok(resolve_check::Resolution::Loopback) => NameResolution::Ok,
            Ok(resolve_check::Resolution::Elsewhere { .. }) => NameResolution::Elsewhere,
            Ok(resolve_check::Resolution::NotFound) | Err(_) => match name.suffix() {
                Suffix::Test => NameResolution::NeedsResolver,
                // Browsers resolve `.localhost` themselves; some tools don't.
                _ => NameResolution::Ok,
            },
        }
    }

    async fn view(&self, name: &LocalName) -> Result<LocalDomainView, LocalDomainError> {
        let status = self.status().await;
        status
            .domains
            .into_iter()
            .find(|d| d.name == name.as_str())
            .ok_or_else(|| LocalDomainError::NotFound(name.to_string()))
    }

    /// Everything at a glance (no prompts; name lookups are bounded).
    pub async fn status(&self) -> LocalDomainsStatus {
        let rows = registry::list(&self.inner.store).await.unwrap_or_default();
        let settings = registry::settings(&self.inner.store)
            .await
            .unwrap_or_default();
        let options = &self.inner.config.options;
        let (listeners, taps, dns_port, dns_error, error, port_problems) = {
            let rt = self.inner.runtime.lock().await;
            (
                rt.listeners.clone(),
                rt.taps.clone(),
                rt.dns.as_ref().map(DnsResponder::local_addr),
                rt.dns_error.clone(),
                rt.error.clone(),
                rt.port_problems.clone(),
            )
        };
        let https_port = listeners
            .as_ref()
            .and_then(|l| l.https_port)
            .unwrap_or(options.https_port);
        let http_port = listeners
            .as_ref()
            .and_then(|l| l.http_port)
            .unwrap_or(options.http_port);
        let mut domains = Vec::with_capacity(rows.len());
        for row in &rows {
            let tap = taps.get(&row.name);
            let requests = tap
                .and_then(|t| self.inner.inspector.metrics(&t.id).ok())
                .map_or(0, |m| m.requests);
            let serving = tap.is_some()
                && listeners.as_ref().is_some_and(|l| {
                    if row.https {
                        !l.https.is_empty()
                    } else {
                        !l.http.is_empty()
                    }
                });
            domains.push(LocalDomainView {
                name: row.name.to_string(),
                url: url_for(
                    &row.name,
                    row.https,
                    if row.https { https_port } else { http_port },
                ),
                origin: origin_of(&row.target),
                target: LocalTarget::from(&row.target),
                wildcard: row.wildcard,
                https: row.https,
                inspect: row.inspect,
                project: row.project.clone(),
                created_at: row.created_at,
                serving,
                resolution: self.resolution(&row.name).await,
                tap_id: tap.map(|t| t.id.clone()),
                requests,
            });
        }
        let platform = self.platform();
        let port = dns_port.map_or(options.dns_port, |a| a.port());
        let needed = rows.iter().any(|r| r.name.suffix() == Suffix::Test);
        let resolver = ResolverView {
            needed,
            responding: dns_port.is_some(),
            port,
            configured: needed
                && domains
                    .iter()
                    .filter(|d| d.name.ends_with(".test"))
                    .all(|d| d.resolution == NameResolution::Ok),
            error: dns_error,
            setup: resolver_config::setup(platform, port)
                .map(|actions| steps(&actions, platform))
                .unwrap_or_default(),
            teardown: steps(&resolver_config::teardown(platform), platform),
        };
        let ca = lock(&self.inner.ca).as_deref().map(ca_view);
        LocalDomainsStatus {
            running: listeners.is_some(),
            https_port: listeners.as_ref().and_then(|l| l.https_port),
            http_port: listeners.as_ref().and_then(|l| l.http_port),
            port_problems,
            lan: settings.lan,
            lan_addresses: lan_addresses(),
            resolver,
            ca,
            domains,
            error,
            platform: platform.into(),
        }
    }

    fn trust_view(&self, ca: Option<&LocalCa>, report: &localdomains::TrustReport) -> TrustView {
        let platform = self.platform();
        TrustView {
            trusted: system_trusted(&report.stores),
            stores: report.stores.iter().map(TrustStoreView::from).collect(),
            steps: steps(&report.privileged, platform),
            ca: ca.map(ca_view),
            platform: platform.into(),
        }
    }

    /// Where the CA is trusted (runs the platform's tools; no prompts). Without a CA yet,
    /// nothing is trusted.
    pub async fn trust_status(&self) -> TrustView {
        let Some(ca) = self.existing_ca().await else {
            return self.trust_view(None, &localdomains::TrustReport::default());
        };
        let stores = self.inner.config.trust.status(&self.cert(&ca)).await;
        self.trust_view(
            Some(&ca),
            &localdomains::TrustReport {
                stores,
                privileged: Vec::new(),
            },
        )
    }

    /// Trusts the CA (creating it first if needed). The system may ask for a password
    /// (macOS) or to confirm (Windows); steps needing an administrator come back.
    ///
    /// # Errors
    /// The CA couldn't be created or read.
    pub async fn trust(&self, options: TrustOptions) -> Result<TrustView, LocalDomainError> {
        let ca = self.ensure_ca().await?;
        let report = self
            .inner
            .config
            .trust
            .install(
                &self.cert(&ca),
                InstallOptions {
                    nss: options.browsers,
                    firefox_enterprise_roots: options.firefox_system_roots,
                },
            )
            .await;
        self.changed();
        Ok(self.trust_view(Some(&ca), &report))
    }

    /// Stops trusting the CA. With `forget`, also deletes its key (a new one is made the
    /// next time a domain needs it, and must be trusted again).
    ///
    /// # Errors
    /// The key couldn't be deleted.
    pub async fn untrust(&self, forget: bool) -> Result<TrustView, LocalDomainError> {
        let ca = self.existing_ca().await;
        let report = match &ca {
            Some(ca) => self.inner.config.trust.uninstall(&self.cert(ca)).await,
            None => localdomains::TrustReport::default(),
        };
        if forget {
            let keys = Arc::clone(&self.inner.config.keys);
            tokio::task::spawn_blocking(move || keys.delete())
                .await
                .map_err(|e| LocalDomainError::Ca(e.to_string()))??;
            *lock(&self.inner.ca) = None;
            let _ = std::fs::remove_file(self.ca_path());
            {
                let mut rt = self.inner.runtime.lock().await;
                self.shut_down(&mut rt).await;
            }
            self.sync_logged().await;
        }
        self.changed();
        Ok(self.trust_view(if forget { None } else { ca.as_deref() }, &report))
    }

    /// The CA certificate (PEM) for phones and other devices, creating the CA if needed.
    ///
    /// # Errors
    /// The CA couldn't be created or read.
    pub async fn ca_certificate(&self) -> Result<String, LocalDomainError> {
        Ok(self.ensure_ca().await?.cert_pem().to_owned())
    }

    /// The CA as an Apple configuration profile (iPhone, iPad).
    ///
    /// # Errors
    /// The CA couldn't be created, or the profile couldn't be written.
    pub async fn ca_profile(&self) -> Result<Vec<u8>, LocalDomainError> {
        let ca = self.ensure_ca().await?;
        localdomains::mobile::apple_mobileconfig(&ca)
            .map_err(|e| LocalDomainError::Ca(e.to_string()))
    }

    /// Runs steps that need an administrator through `pkexec` (Linux only; the system
    /// asks for the password). Only with the person's consent.
    ///
    /// # Errors
    /// Not Linux, no `pkexec`, or a step failed.
    pub async fn run_as_admin(&self, what: AdminTask) -> Result<(), LocalDomainError> {
        use localdomains::{Runner as _, SystemRunner, privileged, process::find_program};
        if self.platform() != Platform::Linux {
            return Err(LocalDomainError::NotSupported);
        }
        let path = std::env::var_os("PATH");
        let pkexec = find_program("pkexec", path.as_deref(), &[Path::new("/usr/bin")])
            .ok_or(LocalDomainError::NoPkexec)?;
        let actions = match what {
            AdminTask::Resolver => {
                let port = self.status().await.resolver.port;
                resolver_config::setup(Platform::Linux, port)
                    .map_err(|e| LocalDomainError::Privileged(e.to_string()))?
            }
            AdminTask::TrustStore => {
                let ca = self.ensure_ca().await?;
                self.inner
                    .config
                    .trust
                    .install(&self.cert(&ca), InstallOptions::default())
                    .await
                    .privileged
            }
        };
        let staging = self.inner.config.dir.join("staging");
        std::fs::create_dir_all(&staging)?;
        let invocations = privileged::pkexec_invocations(&actions, &pkexec, &staging)?;
        for invocation in invocations {
            let output = SystemRunner
                .run(&invocation)
                .await
                .map_err(|e| LocalDomainError::Privileged(e.to_string()))?;
            if !output.success {
                return Err(LocalDomainError::Privileged(
                    output.stderr.trim().to_owned(),
                ));
            }
        }
        let _ = std::fs::remove_dir_all(&staging);
        self.changed();
        Ok(())
    }

    /// The local domains' rows (for the doctor and projects).
    ///
    /// # Errors
    /// The database.
    pub async fn rows(&self) -> Result<Vec<LocalDomainRow>, LocalDomainError> {
        Ok(registry::list(&self.inner.store).await?)
    }

    /// The loaded CA's expiry, if one is loaded.
    pub(crate) fn ca_expiry(&self) -> Option<i64> {
        lock(&self.inner.ca)
            .as_ref()
            .map(|ca| ca.not_after().unix_timestamp())
    }

    /// Replaces the CA with a new one (it must be trusted again), keeping the domains.
    ///
    /// # Errors
    /// The key couldn't be replaced.
    pub async fn renew_ca(&self) -> Result<TrustView, LocalDomainError> {
        let was_trusted = self.trust_status().await.trusted;
        self.untrust(true).await?;
        if was_trusted {
            return self.trust(TrustOptions::default()).await;
        }
        Ok(self.trust_status().await)
    }
}

/// Steps [`LocalDomains::run_as_admin`] can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum AdminTask {
    /// The `.test` resolver entry.
    Resolver,
    /// The Linux system trust store.
    TrustStore,
}

fn domain_of(row: &LocalDomainRow) -> LocalDomain {
    LocalDomain {
        name: row.name.clone(),
        target: row.target.clone(),
        wildcard: row.wildcard,
        created_at: row.created_at,
    }
}
