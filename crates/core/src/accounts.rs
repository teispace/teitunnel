//! Cloudflare accounts: which credentials Teitunnel holds and for which accounts.
//!
//! SQLite keeps only metadata (id, name, credential kind). The credential itself goes
//! to the keychain under `cf:<account-id>:<kind>` (ARCHITECTURE §6). An API token that
//! reaches several accounts is stored once per account, so each account can be removed
//! on its own and removal provably deletes everything for it.
//!
//! An OAuth sign-in is different: Cloudflare rotates its refresh token on every refresh
//! and refuses the old one (revoking the whole sign-in when it's reused), so the
//! accounts a sign-in reaches share one refresh token, `cf:<grant>:oauth`, and it goes
//! (revoked) with the last of them.

pub mod capabilities;
mod cert;
mod domains;
pub mod oauth;
mod template;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use cf_api::{ApiToken, Client};

/// A Cloudflare API client for one account (what [`Accounts::client`] returns).
pub type CloudClient = Client;
use rusqlite::params;
use serde::{Deserialize, Serialize};

pub use cert::{CertCredential, parse_cert_pem};
pub use domains::{Domain, DomainStatus};
pub use template::{TOKENS_PAGE, token_template_url};

use crate::{
    Secret,
    secrets::{SecretError, Secrets, spawn_blocking},
    store::{Store, StoreError},
};

use crate::text::{Text, UserText, english_display, msg};

/// Every table with rows about an account; they go with it. A test checks this against
/// the schema, so a new table can't be forgotten.
const ACCOUNT_TABLES: &[&str] = &[
    "local_tunnels",
    "dns_ownership",
    "access_ownership",
    "balanced_routes",
    "domain_shares",
    "inspected_routes",
    "activity",
    "incidents",
    // Their versions go with them (foreign key).
    "snapshots",
    "reservations_cache",
    "edge_rules",
    "service_tokens",
    "paused_routes",
    "route_schedules",
    "comments",
    "comment_subjects",
    "cloud_databases",
    "front_workers",
];

/// How an account was connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum CredentialKind {
    /// A user API token.
    ApiToken,
    /// OAuth sign-in (refresh token in the keychain).
    #[serde(rename = "oauth")]
    OAuth,
    /// Imported from `cloudflared tunnel login` (cert.pem); one zone only.
    CertPem,
}

impl CredentialKind {
    const ALL: [Self; 3] = [Self::ApiToken, Self::OAuth, Self::CertPem];

    fn as_str(self) -> &'static str {
        match self {
            Self::ApiToken => "apiToken",
            Self::OAuth => "oauth",
            Self::CertPem => "certPem",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == value)
    }
}

/// A connected Cloudflare account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// Cloudflare account id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// How it was connected.
    pub credential: CredentialKind,
    /// For cert.pem credentials: the only zone the credential works for.
    pub limited_zone: Option<String>,
    /// Its OAuth sign-in ended (Cloudflare refused the refresh token): only signing in
    /// again brings it back.
    #[serde(default)]
    pub signed_out: bool,
}

/// Errors from account operations. Messages are shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum AccountError {
    /// Cloudflare rejected the token.
    InvalidToken,
    /// The token works but can't see any account or zone.
    NoAccess,
    /// The cert.pem file couldn't be read or parsed.
    InvalidCert,
    /// Unknown account.
    NotFound,
    /// Cloudflare or the network failed.
    #[error(transparent)]
    Api(#[from] cf_api::Error),
    /// OAuth sign-in or refresh failed.
    #[error(transparent)]
    OAuth(#[from] oauth::OAuthError),
    /// The keychain failed.
    #[error(transparent)]
    Secret(#[from] SecretError),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl UserText for AccountError {
    fn text(&self) -> Text {
        match self {
            Self::Api(err) => err.text(),
            Self::OAuth(err) => err.text(),
            Self::Secret(err) => err.text(),
            Self::Store(err) => err.text(),
            Self::InvalidToken => msg::error::account::invalid_token(),
            Self::NoAccess => msg::error::account::no_access(),
            Self::InvalidCert => msg::error::account::invalid_cert(),
            Self::NotFound => msg::error::account::not_found(),
        }
    }
}

english_display!(AccountError);

fn secret_key(account: &str, kind: CredentialKind) -> String {
    format!("cf:{account}:{}", kind.as_str())
}

/// An OAuth access token, shared through the keychain with every Teitunnel process on
/// this computer (the app, the CLI, the MCP server): Cloudflare rotates the refresh
/// token on each refresh, so a process that refreshed on its own would invalidate the
/// others' access tokens.
#[derive(Clone, Serialize, Deserialize)]
struct HeldAccess {
    access: String,
    issued_ms: u64,
    expires_ms: u64,
    /// Which refresh token it came from (a hash): one from an older refresh token may
    /// have been revoked by the rotation.
    source: String,
}

impl HeldAccess {
    /// Used until 80% of its lifetime, so it never expires mid-request.
    fn fresh(&self, now_ms: u64) -> bool {
        let lifetime = self.expires_ms.saturating_sub(self.issued_ms);
        now_ms < self.issued_ms + lifetime / 5 * 4
    }
}

/// Access tokens this process holds, by OAuth grant.
type AccessCache = HashMap<String, HeldAccess>;

/// Keychain values this process read or wrote recently, by key, with when.
type RecentSecrets = HashMap<String, (Secret<String>, Instant)>;

/// How long a credential read from the keychain is used before it's read again (another
/// process may have replaced or removed it). Refreshing always reads it afresh.
const RECHECK: Duration = Duration::from_secs(60);

/// How long a permission check is reused by background work. Permissions rarely change,
/// and the person's own "check again" (and every sign-in) checks afresh.
const CHECKED_FOR: Duration = Duration::from_secs(15 * 60);

/// A newly issued access token, marked with the refresh token it goes with.
fn held(
    access: &Secret<String>,
    expires_in: std::time::Duration,
    refresh: &Secret<String>,
) -> HeldAccess {
    let issued_ms = now_ms();
    HeldAccess {
        access: access.expose().clone(),
        issued_ms,
        expires_ms: issued_ms
            .saturating_add(u64::try_from(expires_in.as_millis()).unwrap_or(u64::MAX)),
        source: fingerprint(refresh),
    }
}

/// Where the shared access token is kept.
fn shared_access_key(grant: &str) -> String {
    format!("cf:{grant}:oauth-access")
}

/// A new OAuth grant id: `g` and 16 random hex digits (never an account id, which is 32).
fn new_grant() -> Result<String, AccountError> {
    let mut bytes = [0u8; 8];
    getrandom::fill(&mut bytes).map_err(|_| oauth::OAuthError::Random)?;
    Ok(std::iter::once("g".to_owned())
        .chain(bytes.iter().map(|b| format!("{b:02x}")))
        .collect())
}

/// The grant an account's OAuth credential is under: its own id for older rows.
const GRANT: &str = "coalesce(oauth_grant, id)";

/// A short hash identifying a refresh token (never the token itself).
fn fingerprint(token: &Secret<String>) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(token.expose().as_bytes())[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Adds, lists and removes accounts. Cheap to clone.
#[derive(Clone)]
pub struct Accounts {
    store: Store,
    secrets: Secrets,
    api_base: Arc<str>,
    oauth: Option<oauth::OAuthConfig>,
    http: reqwest::Client,
    access: Arc<tokio::sync::Mutex<AccessCache>>,
    /// Grants whose refresh token Cloudflare refused, with that token's hash: they wait
    /// for a new sign-in instead of asking Cloudflare again on every call.
    signed_out: Arc<std::sync::Mutex<HashMap<String, String>>>,
    /// The last permission check of each account, with when: background checks reuse it
    /// instead of sending a dozen probes per account (and per domain) each time.
    checked: Arc<std::sync::Mutex<HashMap<String, (Instant, capabilities::Capabilities)>>>,
    /// So API calls don't each read the keychain (slow, and a prompt for an item this
    /// program isn't trusted with yet).
    recent: Arc<std::sync::Mutex<RecentSecrets>>,
}

impl std::fmt::Debug for Accounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accounts").finish_non_exhaustive()
    }
}

impl Accounts {
    /// Accounts against the production API (OAuth enabled if a client is registered).
    pub fn new(store: Store, secrets: Secrets) -> Self {
        Self::with_api_base(
            store,
            secrets,
            cf_api::API_BASE,
            oauth::OAuthConfig::cloudflare(),
        )
    }

    /// Accounts against another API base (tests).
    pub fn with_api_base(
        store: Store,
        secrets: Secrets,
        api_base: &str,
        oauth: Option<oauth::OAuthConfig>,
    ) -> Self {
        Self {
            store,
            secrets,
            api_base: api_base.into(),
            oauth,
            http: reqwest::Client::new(),
            access: Arc::default(),
            signed_out: Arc::default(),
            checked: Arc::default(),
            recent: Arc::default(),
        }
    }

    /// The OAuth client, if sign-in with Cloudflare is available.
    pub fn oauth(&self) -> Option<&oauth::OAuthConfig> {
        self.oauth.as_ref()
    }

    /// Where OAuth token requests go.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    fn client_with(&self, token: &Secret<String>) -> Result<Client, AccountError> {
        Ok(Client::with_base(
            &self.api_base,
            ApiToken::new(token.expose().clone()),
        )?)
    }

    /// Connected accounts, by name.
    ///
    /// # Errors
    /// The database failed.
    pub async fn list(&self) -> Result<Vec<Account>, AccountError> {
        let rows = self
            .store
            .call(|conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT id, name, credential, limited_zone, {GRANT} FROM accounts ORDER BY name COLLATE NOCASE"
                ))?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?, row.get::<_, String>(4)?))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await?;
        let signed_out = self.signed_out_lock();
        Ok(rows
            .into_iter()
            .filter_map(|(id, name, credential, limited_zone, grant)| {
                let credential = CredentialKind::parse(&credential)?;
                Some(Account {
                    signed_out: credential == CredentialKind::OAuth
                        && signed_out.contains_key(&grant),
                    id,
                    name,
                    credential,
                    limited_zone,
                })
            })
            .collect())
    }

    /// Verifies an API token and connects every account it can reach.
    ///
    /// # Errors
    /// [`AccountError::InvalidToken`] if Cloudflare rejects it, [`AccountError::NoAccess`]
    /// if it reaches nothing.
    pub async fn add_token(&self, token: Secret<String>) -> Result<Vec<Account>, AccountError> {
        let client = self.client_with(&token)?;
        let status = match client.verify_token().await {
            Ok(status) => status,
            // An account-owned token: it verifies under the account it belongs to.
            Err(err) if err.is_auth() => verify_account_owned(&client)
                .await
                .ok_or(AccountError::InvalidToken)?,
            Err(err) => return Err(err.into()),
        };
        if !status.is_active() {
            return Err(AccountError::InvalidToken);
        }
        let reachable = reachable_accounts(&client).await?;
        let before = self.grants_of(&reachable).await?;
        let added = self
            .save_all(reachable, CredentialKind::ApiToken, &token)
            .await?;
        self.forget_unused_grants(before).await;
        Ok(added)
    }

    async fn save_all(
        &self,
        reachable: Vec<(String, String)>,
        credential: CredentialKind,
        secret: &Secret<String>,
    ) -> Result<Vec<Account>, AccountError> {
        let mut added = Vec::new();
        for (id, name) in reachable {
            let account = Account {
                id,
                name,
                credential,
                limited_zone: None,
                signed_out: false,
            };
            self.save(&account, secret).await?;
            added.push(account);
        }
        Ok(added)
    }

    /// Connects every account an OAuth sign-in reaches. The refresh token goes to the
    /// keychain; the access token stays in memory.
    ///
    /// # Errors
    /// [`AccountError::NoAccess`] if the grant reaches nothing, or
    /// [`AccountError::OAuth`] if Cloudflare didn't issue a refresh token.
    pub async fn add_oauth(&self, tokens: oauth::TokenSet) -> Result<Vec<Account>, AccountError> {
        let refresh = tokens
            .refresh_token
            .ok_or_else(|| AccountError::OAuth(oauth::OAuthError::NoOfflineAccess))?;
        let client = self.client_with(&tokens.access_token)?;
        let reachable = reachable_accounts(&client).await?;
        let before = self.grants_of(&reachable).await?;
        // One refresh token for every account the sign-in reaches.
        let grant = new_grant()?;
        self.write(secret_key(&grant, CredentialKind::OAuth), refresh.clone())
            .await?;
        let mut added = Vec::new();
        for (id, name) in reachable {
            let account = Account {
                id,
                name,
                credential: CredentialKind::OAuth,
                limited_zone: None,
                signed_out: false,
            };
            self.save_row(&account, Some(&grant)).await?;
            added.push(account);
        }
        let held = held(&tokens.access_token, tokens.expires_in, &refresh);
        self.share(&grant, &held).await;
        self.access.lock().await.insert(grant, held);
        // The sign-ins these accounts had before, unless another account still uses one.
        self.forget_unused_grants(before).await;
        Ok(added)
    }

    /// The grant each of these accounts is under now (connected ones only).
    async fn grants_of(&self, accounts: &[(String, String)]) -> Result<Vec<String>, AccountError> {
        let ids: Vec<String> = accounts.iter().map(|(id, _)| id.clone()).collect();
        Ok(self
            .store
            .call(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {GRANT} FROM accounts WHERE id = ?1 AND credential = 'oauth'"
                ))?;
                let mut grants = Vec::new();
                for id in ids {
                    if let Ok(grant) = stmt.query_row(params![id], |row| row.get::<_, String>(0)) {
                        grants.push(grant);
                    }
                }
                Ok(grants)
            })
            .await?)
    }

    /// The grant an OAuth account's refresh token is under.
    async fn grant(&self, account_id: &str) -> String {
        let id = account_id.to_owned();
        self.store
            .call(move |conn| {
                Ok(conn.query_row(
                    &format!("SELECT {GRANT} FROM accounts WHERE id = ?1"),
                    params![id],
                    |row| row.get::<_, String>(0),
                )?)
            })
            .await
            .unwrap_or_else(|_| account_id.to_owned())
    }

    /// Revokes and deletes the refresh tokens of these grants that no OAuth account uses
    /// any more (best effort: a failure leaves an unused keychain item).
    async fn forget_unused_grants(&self, grants: Vec<String>) {
        for grant in grants {
            let check = grant.clone();
            let used = self
                .store
                .call(move |conn| {
                    Ok(conn.query_row(
                        &format!(
                            "SELECT count(*) FROM accounts WHERE credential = 'oauth' AND {GRANT} = ?1"
                        ),
                        params![check],
                        |row| row.get::<_, i64>(0),
                    )?)
                })
                .await
                .map_or(true, |count| count > 0);
            if used {
                continue;
            }
            let key = secret_key(&grant, CredentialKind::OAuth);
            if let (Some(config), Ok(refresh)) = (&self.oauth, self.secret(key.clone()).await) {
                oauth::revoke(&self.http, config, &refresh).await;
            }
            self.access.lock().await.remove(&grant);
            self.signed_out_lock().remove(&grant);
            let keys = vec![key, shared_access_key(&grant)];
            {
                let mut recent = self.recent_lock();
                for key in &keys {
                    recent.remove(key);
                }
            }
            let secrets = Arc::clone(&self.secrets);
            if let Err(err) =
                spawn_blocking(move || keys.iter().try_for_each(|key| secrets.delete(key))).await
            {
                tracing::warn!(%err, "couldn't delete an unused sign-in");
            }
        }
    }

    fn signed_out_lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.signed_out
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A valid OAuth access token for `account_id`, refreshed at 80% of its lifetime.
    /// Refreshes are serialised within the process, and processes share the token
    /// through the keychain, so one refresh serves every Teitunnel process and none
    /// invalidates another's token.
    async fn access_token(&self, account_id: &str) -> Result<Secret<String>, AccountError> {
        let grant = self.grant(account_id).await;
        let grant = grant.as_str();
        let mut cache = self.access.lock().await;
        let key = secret_key(grant, CredentialKind::OAuth);
        // The current refresh token's hash, from the database every process updates when
        // it refreshes: no keychain read per call.
        let current = match self.token_source(grant).await {
            Some(source) => source,
            None => fingerprint(&self.remembered(&key).await?),
        };
        let usable =
            |held: &HeldAccess, source: &str| held.source == source && held.fresh(now_ms());
        if let Some(held) = cache.get(grant).filter(|h| usable(h, &current)) {
            return Ok(Secret::new(held.access.clone()));
        }
        // Refreshing: read the refresh token afresh, another process may have rotated it.
        let refresh = self.secret(key.clone()).await?;
        let source = fingerprint(&refresh);
        let usable = |held: &HeldAccess| usable(held, &source);
        // Another process may have refreshed already.
        if let Some(held) = self.shared(grant).await.filter(|h| usable(h)) {
            cache.insert(grant.to_owned(), held.clone());
            return Ok(Secret::new(held.access));
        }
        // Cloudflare already refused this refresh token: asking again won't help.
        if self.signed_out_lock().get(grant) == Some(&source) {
            return Err(oauth::OAuthError::SignedOut.into());
        }
        let config = self.oauth.clone().ok_or(AccountError::NotFound)?;
        let tokens = match oauth::refresh(&self.http, &config, &refresh).await {
            Ok(tokens) => tokens,
            Err(err) => {
                // Refreshed by another process at the same moment: the refresh token we
                // sent was rotated. Use what that process stored.
                return match self.refreshed_elsewhere(grant, &source).await {
                    Some(held) => {
                        cache.insert(grant.to_owned(), held.clone());
                        Ok(Secret::new(held.access))
                    }
                    None => {
                        if matches!(err, oauth::OAuthError::SignedOut) {
                            tracing::warn!(
                                "Cloudflare ended an OAuth sign-in; waiting for a new one"
                            );
                            self.signed_out_lock().insert(grant.to_owned(), source);
                        }
                        Err(err.into())
                    }
                };
            }
        };
        let current = tokens.refresh_token.unwrap_or(refresh);
        self.write(key, current.clone()).await?;
        let held = held(&tokens.access_token, tokens.expires_in, &current);
        self.share(grant, &held).await;
        cache.insert(grant.to_owned(), held);
        Ok(tokens.access_token)
    }

    /// Whether an OAuth account's sign-in ended (Cloudflare refused its refresh token),
    /// as far as this process knows.
    pub async fn signed_out(&self, account_id: &str) -> bool {
        let grant = self.grant(account_id).await;
        self.signed_out_lock().contains_key(&grant)
    }

    /// A keychain item as last read or written, if that was within [`RECHECK`].
    async fn remembered(&self, key: &str) -> Result<Secret<String>, AccountError> {
        let recent = self
            .recent_lock()
            .get(key)
            .filter(|(_, at)| at.elapsed() < RECHECK)
            .map(|(value, _)| value.clone());
        match recent {
            Some(value) => Ok(value),
            None => self.secret(key.to_owned()).await,
        }
    }

    fn recent_lock(&self) -> std::sync::MutexGuard<'_, RecentSecrets> {
        self.recent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Stores a keychain item (and remembers it).
    async fn write(&self, key: String, value: Secret<String>) -> Result<(), AccountError> {
        let secrets = Arc::clone(&self.secrets);
        let (stored_key, stored) = (key.clone(), value.clone());
        spawn_blocking(move || secrets.set(&stored_key, &stored)).await?;
        self.recent_lock().insert(key, (value, Instant::now()));
        Ok(())
    }

    /// A keychain item read afresh, or [`AccountError::NotFound`].
    async fn secret(&self, key: String) -> Result<Secret<String>, AccountError> {
        let secrets = Arc::clone(&self.secrets);
        let read = key.clone();
        let value = spawn_blocking(move || secrets.get(&read)).await?;
        let mut recent = self.recent_lock();
        match value {
            Some(value) => {
                recent.insert(key, (value.clone(), Instant::now()));
                Ok(value)
            }
            None => {
                recent.remove(&key);
                Err(AccountError::NotFound)
            }
        }
    }

    /// The access token another process shared, if any.
    async fn shared(&self, grant: &str) -> Option<HeldAccess> {
        let raw = self.secret(shared_access_key(grant)).await.ok()?;
        serde_json::from_str(raw.expose()).ok()
    }

    /// Shares a new access token with the other processes (best effort: without it
    /// they refresh on their own).
    async fn share(&self, grant: &str, held: &HeldAccess) {
        let Ok(json) = serde_json::to_string(held) else {
            return;
        };
        if let Err(err) = self
            .write(shared_access_key(grant), Secret::new(json))
            .await
        {
            tracing::warn!(%err, "couldn't share the access token");
        }
        let (grant, source) = (grant.to_owned(), held.source.clone());
        if let Err(err) = self
            .store
            .call(move |conn| {
                Ok(conn.execute(
                    &format!("UPDATE accounts SET token_source = ?2 WHERE {GRANT} = ?1"),
                    params![grant, source],
                )?)
            })
            .await
        {
            tracing::warn!(%err, "couldn't record the access token's source");
        }
    }

    /// The hash of the refresh token the shared access token came from, as last recorded.
    async fn token_source(&self, grant: &str) -> Option<String> {
        let grant = grant.to_owned();
        self.store
            .call(move |conn| {
                Ok(conn.query_row(
                    &format!("SELECT token_source FROM accounts WHERE {GRANT} = ?1 LIMIT 1"),
                    params![grant],
                    |row| row.get::<_, Option<String>>(0),
                )?)
            })
            .await
            .ok()
            .flatten()
    }

    /// After a failed refresh: the token another process got with a newer refresh token
    /// (waiting briefly for it to be stored).
    async fn refreshed_elsewhere(&self, grant: &str, stale: &str) -> Option<HeldAccess> {
        for _ in 0..5 {
            let refresh = self
                .secret(secret_key(grant, CredentialKind::OAuth))
                .await
                .ok()?;
            let source = fingerprint(&refresh);
            if source != stale
                && let Some(held) = self.shared(grant).await
                && held.source == source
                && held.fresh(now_ms())
            {
                return Some(held);
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        None
    }

    /// Imports the credential from a `cloudflared tunnel login` cert.pem. The file is
    /// only read; the token is copied into the keychain.
    ///
    /// # Errors
    /// [`AccountError::InvalidCert`] if the file has no usable login.
    pub async fn import_cert(&self, pem: &str) -> Result<Account, AccountError> {
        let cert = parse_cert_pem(pem).ok_or(AccountError::InvalidCert)?;
        let client = self.client_with(&cert.api_token)?;
        let zone = client.zone(&cert.zone_id).await.map_err(|err| {
            if err.is_auth() {
                AccountError::InvalidCert
            } else {
                AccountError::Api(err)
            }
        })?;
        let name = if zone.account.name.is_empty() {
            zone.name.clone()
        } else {
            zone.account.name.clone()
        };
        let account = Account {
            id: cert.account_id.clone(),
            name,
            credential: CredentialKind::CertPem,
            limited_zone: Some(cert.zone_id.clone()),
            signed_out: false,
        };
        self.save(&account, &cert.api_token).await?;
        Ok(account)
    }

    async fn save(&self, account: &Account, secret: &Secret<String>) -> Result<(), AccountError> {
        self.write(secret_key(&account.id, account.credential), secret.clone())
            .await?;
        self.save_row(account, None).await
    }

    /// Records an account (an OAuth one with its grant).
    async fn save_row(&self, account: &Account, grant: Option<&str>) -> Result<(), AccountError> {
        self.checked_lock().remove(&account.id);
        let row = account.clone();
        let grant = grant.map(str::to_owned);
        let added_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO accounts (id, name, credential, limited_zone, added_at, oauth_grant)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name,
                        credential = excluded.credential, limited_zone = excluded.limited_zone,
                        oauth_grant = excluded.oauth_grant, token_source = NULL",
                    params![
                        row.id,
                        row.name,
                        row.credential.as_str(),
                        row.limited_zone,
                        i64::try_from(added_at).unwrap_or(i64::MAX),
                        grant
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Disconnects an account and deletes every credential stored for it. Its OAuth
    /// sign-in is revoked (best effort) once no other account uses it.
    ///
    /// # Errors
    /// [`AccountError::NotFound`], or keychain/database failures.
    pub async fn remove(&self, account_id: &str) -> Result<(), AccountError> {
        let grant = self.grant(account_id).await;
        let id = account_id.to_owned();
        let keys: Vec<String> = CredentialKind::ALL
            .iter()
            .map(|kind| secret_key(&id, *kind))
            .chain(std::iter::once(shared_access_key(&id)))
            .collect();
        let removed = self
            .store
            .call(move |conn| {
                let tx = conn.transaction()?;
                // Everything remembered for the account goes too.
                for table in ACCOUNT_TABLES {
                    tx.execute(
                        &format!("DELETE FROM {table} WHERE account_id = ?1"),
                        params![id],
                    )?;
                }
                let removed = tx.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
                tx.commit()?;
                Ok(removed)
            })
            .await?;
        self.checked_lock().remove(account_id);
        // Before its own items go: an older account's sign-in is stored under its id.
        self.forget_unused_grants(vec![grant]).await;
        {
            let mut recent = self.recent_lock();
            for key in &keys {
                recent.remove(key);
            }
        }
        let secrets = Arc::clone(&self.secrets);
        spawn_blocking(move || keys.iter().try_for_each(|key| secrets.delete(key))).await?;
        if removed == 0 {
            return Err(AccountError::NotFound);
        }
        Ok(())
    }

    /// Probes what the account's credential can do (read-only, no side effects).
    ///
    /// # Errors
    /// [`AccountError::NotFound`] if the account or its credential is missing.
    pub async fn capabilities(
        &self,
        account_id: &str,
    ) -> Result<capabilities::Capabilities, AccountError> {
        let account = self
            .list()
            .await?
            .into_iter()
            .find(|a| a.id == account_id)
            .ok_or(AccountError::NotFound)?;
        let client = self.client(account_id).await?;
        let found = capabilities::probe(&client, account_id, account.limited_zone.as_deref()).await;
        self.checked_lock()
            .insert(account_id.to_owned(), (Instant::now(), found.clone()));
        Ok(found)
    }

    /// What the account's credential can do, as checked within [`CHECKED_FOR`] (checked
    /// now otherwise). For background work; [`Self::capabilities`] always checks.
    ///
    /// # Errors
    /// As [`Self::capabilities`].
    pub async fn capabilities_recent(
        &self,
        account_id: &str,
    ) -> Result<capabilities::Capabilities, AccountError> {
        let recent = self
            .checked_lock()
            .get(account_id)
            .filter(|(at, _)| at.elapsed() < CHECKED_FOR)
            .map(|(_, found)| found.clone());
        match recent {
            Some(found) => Ok(found),
            None => self.capabilities(account_id).await,
        }
    }

    fn checked_lock(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<String, (Instant, capabilities::Capabilities)>> {
        self.checked
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Domains in an account, by name. One-zone credentials see only their zone.
    ///
    /// # Errors
    /// [`AccountError::NotFound`], or API failures.
    pub async fn domains(&self, account_id: &str) -> Result<Vec<Domain>, AccountError> {
        let account = self
            .list()
            .await?
            .into_iter()
            .find(|a| a.id == account_id)
            .ok_or(AccountError::NotFound)?;
        let client = self.client(account_id).await?;
        let zones = match account.limited_zone {
            Some(zone) => vec![client.zone(&zone).await?],
            None => client.zones(account_id).await?,
        };
        let mut domains: Vec<Domain> = zones.into_iter().map(Domain::from).collect();
        domains.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(domains)
    }

    /// An API client for a connected account.
    ///
    /// # Errors
    /// [`AccountError::NotFound`] if the account or its credential is missing.
    pub async fn client(&self, account_id: &str) -> Result<Client, AccountError> {
        let account = self
            .list()
            .await?
            .into_iter()
            .find(|a| a.id == account_id)
            .ok_or(AccountError::NotFound)?;
        if account.credential == CredentialKind::OAuth {
            let token = self.access_token(account_id).await?;
            return self.client_with(&token);
        }
        let token = self
            .remembered(&secret_key(&account.id, account.credential))
            .await?;
        self.client_with(&token)
    }
}

/// Accounts a credential can reach. Zone-scoped tokens may not list accounts, so fall
/// back to the owners of the zones they can see.
/// The status of an account-owned token, from the first account it can see that
/// verifies it; `None` if there's none (then it isn't a valid token of either kind).
async fn verify_account_owned(client: &Client) -> Option<cf_api::TokenStatus> {
    for (id, _) in reachable_accounts(client).await.ok()? {
        if let Ok(status) = client.verify_account_token(&id).await {
            return Some(status);
        }
    }
    None
}

async fn reachable_accounts(client: &Client) -> Result<Vec<(String, String)>, AccountError> {
    let mut reachable: Vec<(String, String)> = match client.accounts().await {
        Ok(accounts) => accounts.into_iter().map(|a| (a.id, a.name)).collect(),
        Err(err) if err.is_auth() => Vec::new(),
        Err(err) => return Err(err.into()),
    };
    if reachable.is_empty() {
        let zones = match client.all_zones().await {
            Ok(zones) => zones,
            Err(err) if err.is_auth() => Vec::new(),
            Err(err) => return Err(err.into()),
        };
        for zone in zones {
            if !reachable.iter().any(|(id, _)| *id == zone.account.id) {
                reachable.push((zone.account.id, zone.account.name));
            }
        }
    }
    if reachable.is_empty() {
        return Err(AccountError::NoAccess);
    }
    Ok(reachable)
}

/// Runs a blocking keychain call off the async runtime.
#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_string_contains, header, path},
    };

    use super::*;
    use crate::secrets::{MemoryStore, SecretStore};

    #[allow(clippy::needless_pass_by_value)]
    fn envelope(result: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true, "errors": [], "messages": [], "result": result,
            "result_info": {"page": 1, "per_page": 50, "total_pages": 1}
        }))
    }

    fn denied() -> ResponseTemplate {
        ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "success": false, "errors": [{"code": 10000, "message": "Authentication error"}], "messages": [], "result": null
        }))
    }

    async fn setup() -> (MockServer, Accounts, MemoryStore) {
        let server = MockServer::start().await;
        let secrets = MemoryStore::default();
        let oauth = oauth::OAuthConfig::with_base("client".into(), &server.uri());
        let accounts = Accounts::with_api_base(
            Store::open_in_memory().unwrap(),
            Arc::new(secrets.clone()),
            &server.uri(),
            Some(oauth),
        );
        (server, accounts, secrets)
    }

    fn tokens(access: &str, refresh: Option<&str>, expires_in: u64) -> oauth::TokenSet {
        oauth::TokenSet {
            access_token: Secret::new(access.into()),
            refresh_token: refresh.map(|r| Secret::new(r.into())),
            expires_in: std::time::Duration::from_secs(expires_in),
        }
    }

    /// Where an account's OAuth refresh token is kept.
    async fn refresh_key(accounts: &Accounts, account: &str) -> String {
        secret_key(&accounts.grant(account).await, CredentialKind::OAuth)
    }

    /// Makes the held access token (in this process and the shared one) stale.
    async fn expire(accounts: &Accounts, secrets: &MemoryStore, account: &str) {
        let grant = accounts.grant(account).await;
        let refresh = secrets
            .get(&secret_key(&grant, CredentialKind::OAuth))
            .unwrap()
            .unwrap();
        let stale = HeldAccess {
            access: "old".into(),
            issued_ms: 0,
            expires_ms: 1,
            source: fingerprint(&refresh),
        };
        accounts
            .access
            .lock()
            .await
            .insert(grant.clone(), stale.clone());
        accounts.share(&grant, &stale).await;
    }

    #[tokio::test]
    async fn processes_share_one_refresh_instead_of_invalidating_each_other() {
        let (server, app, secrets) = setup().await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        app.add_oauth(tokens("access-1", Some("refresh-1"), 3600))
            .await
            .unwrap();
        // The CLI or MCP server: another process with the same keychain and database.
        let cli = Accounts::with_api_base(
            app.store.clone(),
            Arc::new(secrets.clone()),
            &server.uri(),
            Some(oauth::OAuthConfig::with_base(
                "client".into(),
                &server.uri(),
            )),
        );
        assert_eq!(cli.access_token("a1").await.unwrap().expose(), "access-1");

        // The app's token ages out; it refreshes (rotating the refresh token) once.
        expire(&app, &secrets, "a1").await;
        Mock::given(path("/oauth2/token"))
            .and(body_string_contains("refresh_token=refresh-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(app.access_token("a1").await.unwrap().expose(), "access-2");
        // The CLI's access-1 came from the rotated refresh token: it takes the app's new
        // one instead of refreshing with a revoked refresh token.
        assert_eq!(cli.access_token("a1").await.unwrap().expose(), "access-2");
    }

    #[tokio::test]
    async fn a_refresh_that_lost_the_race_uses_the_winner() {
        let (server, app, secrets) = setup().await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        app.add_oauth(tokens("access-1", Some("refresh-1"), 3600))
            .await
            .unwrap();
        expire(&app, &secrets, "a1").await;
        // Another process refreshes with refresh-1 at the same moment and wins: ours is
        // refused, and its result lands in the keychain shortly after.
        Mock::given(path("/oauth2/token"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_json(serde_json::json!({ "error": "invalid_grant" }))
                    .set_delay(std::time::Duration::from_millis(50)),
            )
            .expect(1)
            .mount(&server)
            .await;
        let key = refresh_key(&app, "a1").await;
        let grant = app.grant("a1").await;
        let other = async {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let winner = Secret::new("refresh-2".to_owned());
            secrets.set(&key, &winner).unwrap();
            let theirs = held(
                &Secret::new("access-2".into()),
                std::time::Duration::from_secs(3600),
                &winner,
            );
            app.share(&grant, &theirs).await;
        };
        let (ours, ()) = tokio::join!(app.access_token("a1"), other);
        assert_eq!(ours.unwrap().expose(), "access-2");
    }

    #[tokio::test]
    async fn removing_an_account_forgets_everything_about_it() {
        let (_server, accounts, _) = setup().await;
        let tables: Vec<String> = accounts
            .store
            .call(|conn| {
                let mut statement = conn.prepare(
                    "SELECT m.name FROM sqlite_master m, pragma_table_info(m.name) c \
                     WHERE m.type = 'table' AND c.name = 'account_id' ORDER BY m.name",
                )?;
                let names = statement
                    .query_map([], |row| row.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                Ok(names)
            })
            .await
            .unwrap();
        let mut expected: Vec<&str> = ACCOUNT_TABLES.to_vec();
        expected.sort_unstable();
        assert_eq!(
            tables, expected,
            "every table keyed by account is cleared on removal"
        );
    }

    #[tokio::test]
    async fn oauth_accounts_refresh_and_revoke() {
        let (server, accounts, secrets) = setup().await;
        Mock::given(path("/accounts"))
            .and(header("authorization", "Bearer access-1"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        let added = accounts
            .add_oauth(tokens("access-1", Some("refresh-1"), 3600))
            .await
            .unwrap();
        assert_eq!(added[0].credential, CredentialKind::OAuth);
        let grant = accounts.grant("a1").await;
        let key = secret_key(&grant, CredentialKind::OAuth);
        assert_eq!(secrets.keys(), [key.clone(), shared_access_key(&grant)]);
        assert_eq!(secrets.get(&key).unwrap().unwrap().expose(), "refresh-1");

        // Fresh access token: no refresh needed.
        assert!(accounts.client("a1").await.is_ok());

        // Expire it (here and in the keychain): the next client refreshes once and stores
        // the rotated refresh token.
        expire(&accounts, &secrets, "a1").await;
        Mock::given(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        let (first, second) =
            tokio::join!(accounts.access_token("a1"), accounts.access_token("a1"));
        assert_eq!(first.unwrap().expose(), "access-2");
        assert_eq!(
            second.unwrap().expose(),
            "access-2",
            "single-flight: one refresh"
        );
        assert_eq!(secrets.get(&key).unwrap().unwrap().expose(), "refresh-2");

        Mock::given(path("/oauth2/revoke"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        accounts.remove("a1").await.unwrap();
        assert!(secrets.keys().is_empty());
    }

    /// One sign-in that reaches two accounts.
    async fn two_accounts_one_sign_in() -> (MockServer, Accounts, MemoryStore) {
        let (server, accounts, secrets) = setup().await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(serde_json::json!([
                {"id": "a1", "name": "Personal"}, {"id": "a2", "name": "Work"}
            ])))
            .mount(&server)
            .await;
        accounts
            .add_oauth(tokens("access-1", Some("refresh-1"), 3600))
            .await
            .unwrap();
        (server, accounts, secrets)
    }

    #[tokio::test]
    async fn accounts_from_one_sign_in_share_its_rotating_refresh_token() {
        let (server, accounts, secrets) = two_accounts_one_sign_in().await;
        assert_eq!(accounts.grant("a1").await, accounts.grant("a2").await);
        // One refresh token in the keychain, not a copy per account that the first
        // refresh would leave stale (and Cloudflare would refuse, ending the sign-in).
        assert_eq!(secrets.keys().len(), 2, "{:?}", secrets.keys());

        expire(&accounts, &secrets, "a1").await;
        Mock::given(path("/oauth2/token"))
            .and(body_string_contains("refresh_token=refresh-1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600
            })))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            accounts.access_token("a1").await.unwrap().expose(),
            "access-2"
        );
        // The other account uses the same new token: no second refresh with refresh-1.
        assert_eq!(
            accounts.access_token("a2").await.unwrap().expose(),
            "access-2"
        );
        assert_eq!(
            secrets
                .get(&refresh_key(&accounts, "a2").await)
                .unwrap()
                .unwrap()
                .expose(),
            "refresh-2"
        );
    }

    #[tokio::test]
    async fn disconnecting_one_account_keeps_the_others_signed_in() {
        let (server, accounts, secrets) = two_accounts_one_sign_in().await;
        Mock::given(path("/oauth2/revoke"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        accounts.remove("a1").await.unwrap();
        // Not revoked: Work still uses it.
        assert_eq!(
            accounts.access_token("a2").await.unwrap().expose(),
            "access-1"
        );
        assert_eq!(secrets.keys().len(), 2);
        // The last one revokes it (the one expected call) and deletes it.
        accounts.remove("a2").await.unwrap();
        assert!(secrets.keys().is_empty(), "{:?}", secrets.keys());
    }

    #[tokio::test]
    async fn an_ended_sign_in_isnt_retried_until_signing_in_again() {
        let (server, accounts, secrets) = two_accounts_one_sign_in().await;
        expire(&accounts, &secrets, "a1").await;
        Mock::given(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant", "error_description": "The refresh token is malformed or not valid."
            })))
            .expect(1)
            .mount(&server)
            .await;
        for account in ["a1", "a2", "a1"] {
            assert!(matches!(
                accounts.access_token(account).await,
                Err(AccountError::OAuth(oauth::OAuthError::SignedOut))
            ));
        }
        assert!(accounts.signed_out("a2").await);
        assert!(accounts.list().await.unwrap().iter().all(|a| a.signed_out));

        // Signing in again works at once.
        accounts
            .add_oauth(tokens("access-3", Some("refresh-3"), 3600))
            .await
            .unwrap();
        assert!(!accounts.signed_out("a1").await);
        assert_eq!(
            accounts.access_token("a2").await.unwrap().expose(),
            "access-3"
        );
        assert_eq!(secrets.keys().len(), 2, "the ended sign-in is deleted");
    }

    #[tokio::test]
    async fn signing_in_again_moves_older_accounts_to_one_shared_token() {
        let (server, accounts, secrets) = setup().await;
        // As an earlier version stored them: a copy per account under its own id.
        for (id, name) in [("a1", "Personal"), ("a2", "Work")] {
            let account = Account {
                id: id.into(),
                name: name.into(),
                credential: CredentialKind::OAuth,
                limited_zone: None,
                signed_out: false,
            };
            accounts.save_row(&account, None).await.unwrap();
            secrets
                .set(&format!("cf:{id}:oauth"), &Secret::new("old".into()))
                .unwrap();
        }
        assert_eq!(accounts.grant("a1").await, "a1");
        Mock::given(path("/accounts"))
            .respond_with(envelope(serde_json::json!([
                {"id": "a1", "name": "Personal"}, {"id": "a2", "name": "Work"}
            ])))
            .mount(&server)
            .await;
        Mock::given(path("/oauth2/revoke"))
            .respond_with(ResponseTemplate::new(200))
            .expect(2)
            .mount(&server)
            .await;
        accounts
            .add_oauth(tokens("access-1", Some("refresh-1"), 3600))
            .await
            .unwrap();
        let grant = accounts.grant("a1").await;
        assert!(grant.starts_with('g'));
        assert_eq!(accounts.grant("a2").await, grant);
        let mut expected = vec![
            secret_key(&grant, CredentialKind::OAuth),
            shared_access_key(&grant),
        ];
        expected.sort();
        assert_eq!(secrets.keys(), expected);
    }

    #[tokio::test]
    async fn background_checks_reuse_a_recent_permission_check() {
        let (server, accounts, _) = setup().await;
        mount_verify(&server, true).await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        accounts.add_token(Secret::new("tok".into())).await.unwrap();
        Mock::given(path("/accounts/a1/cfd_tunnel"))
            .respond_with(envelope(serde_json::json!([])))
            .expect(2)
            .mount(&server)
            .await;
        // A background check probes once, then reuses it.
        for _ in 0..3 {
            accounts.capabilities_recent("a1").await.unwrap();
        }
        // The person's own check always asks Cloudflare.
        accounts.capabilities("a1").await.unwrap();
        accounts.capabilities_recent("a1").await.unwrap();
    }

    #[tokio::test]
    async fn oauth_without_offline_access_is_refused() {
        let (_server, accounts, secrets) = setup().await;
        let err = accounts
            .add_oauth(tokens("access", None, 3600))
            .await
            .unwrap_err();
        assert!(matches!(err, AccountError::OAuth(_)));
        assert!(secrets.keys().is_empty());
    }

    async fn mount_verify(server: &MockServer, active: bool) {
        Mock::given(path("/user/tokens/verify"))
            .respond_with(envelope(serde_json::json!({"id": "t1", "status": if active { "active" } else { "disabled" }})))
            .mount(server)
            .await;
    }

    /// A keychain that counts reads.
    #[derive(Debug, Default)]
    struct Counting {
        inner: MemoryStore,
        reads: std::sync::atomic::AtomicUsize,
    }

    impl SecretStore for Counting {
        fn set(&self, key: &str, value: &Secret<String>) -> Result<(), SecretError> {
            self.inner.set(key, value)
        }
        fn get(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
            self.reads.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.get(key)
        }
        fn delete(&self, key: &str) -> Result<(), SecretError> {
            self.inner.delete(key)
        }
    }

    #[tokio::test]
    async fn api_calls_dont_each_read_the_keychain() {
        let server = MockServer::start().await;
        let keychain = Arc::new(Counting::default());
        let accounts = Accounts::with_api_base(
            Store::open_in_memory().unwrap(),
            keychain.clone(),
            &server.uri(),
            Some(oauth::OAuthConfig::with_base(
                "client".into(),
                &server.uri(),
            )),
        );
        mount_verify(&server, true).await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        accounts.add_token(Secret::new("tok".into())).await.unwrap();
        let reads = || keychain.reads.load(std::sync::atomic::Ordering::SeqCst);
        let before = reads();
        for _ in 0..20 {
            accounts.client("a1").await.unwrap();
        }
        assert_eq!(reads(), before, "the token was just stored: no read needed");
    }

    #[tokio::test]
    async fn oauth_calls_dont_each_read_the_keychain() {
        let server = MockServer::start().await;
        let keychain = Arc::new(Counting::default());
        let accounts = Accounts::with_api_base(
            Store::open_in_memory().unwrap(),
            keychain.clone(),
            &server.uri(),
            Some(oauth::OAuthConfig::with_base(
                "client".into(),
                &server.uri(),
            )),
        );
        Mock::given(path("/accounts"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        accounts
            .add_oauth(tokens("access", Some("refresh"), 3600))
            .await
            .unwrap();
        let before = keychain.reads.load(std::sync::atomic::Ordering::SeqCst);
        for _ in 0..20 {
            assert_eq!(
                accounts.access_token("a1").await.unwrap().expose(),
                "access"
            );
        }
        assert_eq!(
            keychain.reads.load(std::sync::atomic::Ordering::SeqCst),
            before
        );
    }

    #[tokio::test]
    async fn token_connects_every_reachable_account() {
        let (server, accounts, secrets) = setup().await;
        mount_verify(&server, true).await;
        Mock::given(path("/accounts"))
            .and(header("authorization", "Bearer tok"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a2", "name": "Work"}, {"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        let added = accounts.add_token(Secret::new("tok".into())).await.unwrap();
        assert_eq!(added.len(), 2);
        let listed = accounts.list().await.unwrap();
        assert_eq!(
            listed.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(),
            ["Personal", "Work"]
        );
        assert_eq!(secrets.keys(), ["cf:a1:apiToken", "cf:a2:apiToken"]);
        assert!(accounts.client("a1").await.is_ok());
    }

    #[tokio::test]
    async fn account_owned_tokens_verify_under_their_account() {
        let (server, accounts, secrets) = setup().await;
        // The user endpoint doesn't know account-owned tokens (code 1000).
        Mock::given(path("/user/tokens/verify"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "success": false, "errors": [{"code": 1000, "message": "Invalid API Token"}],
                "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}]),
            ))
            .mount(&server)
            .await;
        Mock::given(path("/accounts/a1/tokens/verify"))
            .respond_with(envelope(
                serde_json::json!({"id": "t1", "status": "active"}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let added = accounts
            .add_token(Secret::new("acct".into()))
            .await
            .unwrap();
        assert_eq!(added[0].id, "a1");
        assert_eq!(secrets.keys(), ["cf:a1:apiToken"]);
    }

    #[tokio::test]
    async fn rejected_or_disabled_tokens_are_invalid() {
        let (server, accounts, secrets) = setup().await;
        Mock::given(path("/user/tokens/verify"))
            .respond_with(denied())
            .mount(&server)
            .await;
        assert!(matches!(
            accounts.add_token(Secret::new("bad".into())).await,
            Err(AccountError::InvalidToken)
        ));

        let (server, accounts, _) = setup().await;
        mount_verify(&server, false).await;
        assert!(matches!(
            accounts.add_token(Secret::new("off".into())).await,
            Err(AccountError::InvalidToken)
        ));
        assert!(
            secrets.keys().is_empty(),
            "nothing stored for rejected tokens"
        );
    }

    #[tokio::test]
    async fn zone_scoped_tokens_derive_accounts_from_zones() {
        let (server, accounts, _) = setup().await;
        mount_verify(&server, true).await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(path("/zones"))
            .respond_with(envelope(serde_json::json!([
                {"id": "z1", "name": "xyz.com", "status": "active", "account": {"id": "a1", "name": "Personal"}},
                {"id": "z2", "name": "yx.com", "status": "active", "account": {"id": "a1", "name": "Personal"}}
            ])))
            .mount(&server)
            .await;
        let added = accounts.add_token(Secret::new("tok".into())).await.unwrap();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].id, "a1");
    }

    #[tokio::test]
    async fn tokens_that_reach_nothing_are_refused() {
        let (server, accounts, secrets) = setup().await;
        mount_verify(&server, true).await;
        Mock::given(path("/accounts"))
            .respond_with(denied())
            .mount(&server)
            .await;
        Mock::given(path("/zones"))
            .respond_with(envelope(serde_json::json!([])))
            .mount(&server)
            .await;
        assert!(matches!(
            accounts.add_token(Secret::new("tok".into())).await,
            Err(AccountError::NoAccess)
        ));
        assert!(secrets.keys().is_empty());
    }

    #[tokio::test]
    async fn removing_an_account_deletes_all_of_its_secrets() {
        let (server, accounts, secrets) = setup().await;
        mount_verify(&server, true).await;
        Mock::given(path("/accounts"))
            .respond_with(envelope(
                serde_json::json!([{"id": "a1", "name": "Personal"}, {"id": "a2", "name": "Work"}]),
            ))
            .mount(&server)
            .await;
        accounts.add_token(Secret::new("tok".into())).await.unwrap();
        // A stray OAuth refresh token for the same account must go too.
        secrets
            .set("cf:a1:oauth", &Secret::new("refresh".into()))
            .unwrap();
        let local = crate::engine::Local::new(accounts.store.clone());
        local.set_machine_tunnel("a1", "t1", "Mac").await.unwrap();
        local.log("a1", "Add", "applied", &[], None).await.unwrap();

        accounts.remove("a1").await.unwrap();
        assert_eq!(secrets.keys(), ["cf:a2:apiToken"]);
        assert_eq!(local.machine_tunnel("a1").await.unwrap(), None);
        assert!(local.activity("a1", 5).await.unwrap().is_empty());
        assert_eq!(accounts.list().await.unwrap().len(), 1);
        assert!(matches!(
            accounts.remove("a1").await,
            Err(AccountError::NotFound)
        ));
        assert!(matches!(
            accounts.client("a1").await,
            Err(AccountError::NotFound)
        ));
    }

    #[tokio::test]
    async fn imports_cert_pem_as_a_limited_account() {
        let (server, accounts, secrets) = setup().await;
        Mock::given(path("/zones/z1"))
            .and(header("authorization", "Bearer cert-token"))
            .respond_with(envelope(serde_json::json!(
                {"id": "z1", "name": "xyz.com", "status": "active", "account": {"id": "a1", "name": "Personal"}}
            )))
            .mount(&server)
            .await;
        let account = accounts
            .import_cert(&cert::sample_pem("z1", "a1", "cert-token"))
            .await
            .unwrap();
        assert_eq!(account.credential, CredentialKind::CertPem);
        assert_eq!(account.limited_zone.as_deref(), Some("z1"));
        assert_eq!(secrets.keys(), ["cf:a1:certPem"]);
        assert!(matches!(
            accounts.import_cert("nonsense").await,
            Err(AccountError::InvalidCert)
        ));
    }
}
