//! Cloudflare accounts: which credentials Teitunnel holds and for which accounts.
//!
//! SQLite keeps only metadata (id, name, credential kind). The credential itself goes
//! to the keychain under `cf:<account-id>:<kind>` (ARCHITECTURE §6). A token that
//! reaches several accounts is stored once per account, so each account can be removed
//! on its own and removal provably deletes everything for it.

pub mod capabilities;
mod cert;
mod domains;
pub mod oauth;
mod template;

use std::{
    collections::HashMap,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use tokio::time::Instant;

use cf_api::{ApiToken, Client};
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
    "activity",
    // Their versions go with them (foreign key).
    "snapshots",
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

/// Cached OAuth access tokens: (token, issued, expires).
type AccessCache = HashMap<String, (Secret<String>, Instant, Instant)>;

/// Adds, lists and removes accounts. Cheap to clone.
#[derive(Clone)]
pub struct Accounts {
    store: Store,
    secrets: Secrets,
    api_base: Arc<str>,
    oauth: Option<oauth::OAuthConfig>,
    http: reqwest::Client,
    access: Arc<tokio::sync::Mutex<AccessCache>>,
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
        Ok(self
            .store
            .call(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, name, credential, limited_zone FROM accounts ORDER BY name COLLATE NOCASE",
                )?;
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, Option<String>>(3)?))
                })?;
                let mut accounts = Vec::new();
                for row in rows {
                    let (id, name, credential, limited_zone) = row?;
                    if let Some(credential) = CredentialKind::parse(&credential) {
                        accounts.push(Account { id, name, credential, limited_zone });
                    }
                }
                Ok(accounts)
            })
            .await?)
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
        self.save_all(reachable, CredentialKind::ApiToken, &token)
            .await
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
        let added = self
            .save_all(reachable, CredentialKind::OAuth, &refresh)
            .await?;
        let expires = Instant::now() + tokens.expires_in;
        let mut cache = self.access.lock().await;
        for account in &added {
            cache.insert(
                account.id.clone(),
                (tokens.access_token.clone(), Instant::now(), expires),
            );
        }
        Ok(added)
    }

    /// A valid OAuth access token for `account_id`, refreshed at 80% of its lifetime.
    /// Refreshes are serialised, so concurrent callers never race to rotate the token.
    async fn access_token(&self, account_id: &str) -> Result<Secret<String>, AccountError> {
        let mut cache = self.access.lock().await;
        if let Some((token, issued, expires)) = cache.get(account_id) {
            let lifetime = expires.saturating_duration_since(*issued);
            if issued.elapsed() < lifetime.mul_f64(0.8) {
                return Ok(token.clone());
            }
        }
        let config = self.oauth.clone().ok_or(AccountError::NotFound)?;
        let secrets = Arc::clone(&self.secrets);
        let key = secret_key(account_id, CredentialKind::OAuth);
        let refresh = spawn_blocking(move || secrets.get(&key))
            .await?
            .ok_or(AccountError::NotFound)?;
        let tokens = oauth::refresh(&self.http, &config, &refresh).await?;
        if let Some(rotated) = tokens.refresh_token {
            let secrets = Arc::clone(&self.secrets);
            let key = secret_key(account_id, CredentialKind::OAuth);
            spawn_blocking(move || secrets.set(&key, &rotated)).await?;
        }
        let now = Instant::now();
        cache.insert(
            account_id.to_owned(),
            (tokens.access_token.clone(), now, now + tokens.expires_in),
        );
        Ok(tokens.access_token)
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
        };
        self.save(&account, &cert.api_token).await?;
        Ok(account)
    }

    async fn save(&self, account: &Account, secret: &Secret<String>) -> Result<(), AccountError> {
        let secrets = Arc::clone(&self.secrets);
        let key = secret_key(&account.id, account.credential);
        let secret = secret.clone();
        spawn_blocking(move || secrets.set(&key, &secret)).await?;
        let row = account.clone();
        let added_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO accounts (id, name, credential, limited_zone, added_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(id) DO UPDATE SET name = excluded.name,
                        credential = excluded.credential, limited_zone = excluded.limited_zone",
                    params![
                        row.id,
                        row.name,
                        row.credential.as_str(),
                        row.limited_zone,
                        i64::try_from(added_at).unwrap_or(i64::MAX)
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Disconnects an account and deletes every credential stored for it (OAuth grants
    /// are revoked first, best effort).
    ///
    /// # Errors
    /// [`AccountError::NotFound`], or keychain/database failures.
    pub async fn remove(&self, account_id: &str) -> Result<(), AccountError> {
        if let Some(config) = &self.oauth {
            let secrets = Arc::clone(&self.secrets);
            let key = secret_key(account_id, CredentialKind::OAuth);
            if let Ok(Some(refresh)) = spawn_blocking(move || secrets.get(&key)).await {
                oauth::revoke(&self.http, config, &refresh).await;
            }
        }
        self.access.lock().await.remove(account_id);
        let id = account_id.to_owned();
        let secrets = Arc::clone(&self.secrets);
        let keys: Vec<String> = CredentialKind::ALL
            .iter()
            .map(|kind| secret_key(&id, *kind))
            .collect();
        spawn_blocking(move || keys.iter().try_for_each(|key| secrets.delete(key))).await?;
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
        if removed == 0 {
            Err(AccountError::NotFound)
        } else {
            Ok(())
        }
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
        Ok(capabilities::probe(&client, account_id, account.limited_zone.as_deref()).await)
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
        let secrets = Arc::clone(&self.secrets);
        let key = secret_key(&account.id, account.credential);
        let token = spawn_blocking(move || secrets.get(&key))
            .await?
            .ok_or(AccountError::NotFound)?;
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
        matchers::{header, path},
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
        assert_eq!(secrets.keys(), ["cf:a1:oauth"]);
        assert_eq!(
            secrets.get("cf:a1:oauth").unwrap().unwrap().expose(),
            "refresh-1"
        );

        // Fresh access token: no refresh needed.
        assert!(accounts.client("a1").await.is_ok());

        // Expire it: the next client refreshes once and stores the rotated refresh token.
        accounts.access.lock().await.insert(
            "a1".into(),
            (
                Secret::new("old".into()),
                Instant::now() - std::time::Duration::from_secs(100),
                Instant::now(),
            ),
        );
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
        assert_eq!(
            secrets.get("cf:a1:oauth").unwrap().unwrap().expose(),
            "refresh-2"
        );

        Mock::given(path("/oauth2/revoke"))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        accounts.remove("a1").await.unwrap();
        assert!(secrets.keys().is_empty());
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
