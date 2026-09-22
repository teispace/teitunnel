//! Cloudflare accounts: which credentials Teitunnel holds and for which accounts.
//!
//! SQLite keeps only metadata (id, name, credential kind). The credential itself goes
//! to the keychain under `cf:<account-id>:<kind>` (ARCHITECTURE §6). A token that
//! reaches several accounts is stored once per account, so each account can be removed
//! on its own and removal provably deletes everything for it.

pub mod capabilities;
mod cert;

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use cf_api::{ApiToken, Client};
use rusqlite::params;
use serde::{Deserialize, Serialize};

pub use cert::{CertCredential, parse_cert_pem};

use crate::{
    Secret,
    secrets::{SecretError, Secrets},
    store::{Store, StoreError},
};

/// How an account was connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum CredentialKind {
    /// A user API token.
    ApiToken,
    /// OAuth sign-in (refresh token in the keychain).
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
    #[error(
        "Cloudflare didn't accept this token. Check that you copied all of it and that it hasn't expired."
    )]
    InvalidToken,
    /// The token works but can't see any account or zone.
    #[error(
        "This token can't access any Cloudflare account or domain. Create it with the Teitunnel template."
    )]
    NoAccess,
    /// The cert.pem file couldn't be read or parsed.
    #[error(
        "That cert.pem doesn't contain a Cloudflare login. Run `cloudflared tunnel login` again, or use an API token."
    )]
    InvalidCert,
    /// Unknown account.
    #[error("That account isn't connected.")]
    NotFound,
    /// Cloudflare or the network failed.
    #[error(transparent)]
    Api(#[from] cf_api::Error),
    /// The keychain failed.
    #[error(transparent)]
    Secret(#[from] SecretError),
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn secret_key(account: &str, kind: CredentialKind) -> String {
    format!("cf:{account}:{}", kind.as_str())
}

/// Adds, lists and removes accounts. Cheap to clone.
#[derive(Clone)]
pub struct Accounts {
    store: Store,
    secrets: Secrets,
    api_base: Arc<str>,
}

impl std::fmt::Debug for Accounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Accounts").finish_non_exhaustive()
    }
}

impl Accounts {
    /// Accounts against the production API.
    pub fn new(store: Store, secrets: Secrets) -> Self {
        Self::with_api_base(store, secrets, cf_api::API_BASE)
    }

    /// Accounts against another API base (tests).
    pub fn with_api_base(store: Store, secrets: Secrets, api_base: &str) -> Self {
        Self {
            store,
            secrets,
            api_base: api_base.into(),
        }
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
        match client.verify_token().await {
            Ok(status) if status.is_active() => {}
            Ok(_) => return Err(AccountError::InvalidToken),
            Err(err) if err.is_auth() => return Err(AccountError::InvalidToken),
            Err(err) => return Err(err.into()),
        }
        let mut reachable: Vec<(String, String)> = match client.accounts().await {
            Ok(accounts) => accounts.into_iter().map(|a| (a.id, a.name)).collect(),
            Err(err) if err.is_auth() => Vec::new(),
            Err(err) => return Err(err.into()),
        };
        if reachable.is_empty() {
            // Zone-scoped tokens may not list accounts; derive them from their zones.
            let zones = client.all_zones().await.or_else(|err| {
                if err.is_auth() {
                    Ok(Vec::new())
                } else {
                    Err(err)
                }
            })?;
            for zone in zones {
                if !reachable.iter().any(|(id, _)| *id == zone.account.id) {
                    reachable.push((zone.account.id, zone.account.name));
                }
            }
        }
        if reachable.is_empty() {
            return Err(AccountError::NoAccess);
        }
        let mut added = Vec::new();
        for (id, name) in reachable {
            let account = Account {
                id,
                name,
                credential: CredentialKind::ApiToken,
                limited_zone: None,
            };
            self.save(&account, &token).await?;
            added.push(account);
        }
        Ok(added)
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

    /// Disconnects an account and deletes every credential stored for it.
    ///
    /// # Errors
    /// [`AccountError::NotFound`], or keychain/database failures.
    pub async fn remove(&self, account_id: &str) -> Result<(), AccountError> {
        let id = account_id.to_owned();
        let secrets = Arc::clone(&self.secrets);
        let keys: Vec<String> = CredentialKind::ALL
            .iter()
            .map(|kind| secret_key(&id, *kind))
            .collect();
        spawn_blocking(move || keys.iter().try_for_each(|key| secrets.delete(key))).await?;
        let removed = self
            .store
            .call(move |conn| Ok(conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])?))
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
        let secrets = Arc::clone(&self.secrets);
        let key = secret_key(&account.id, account.credential);
        let token = spawn_blocking(move || secrets.get(&key))
            .await?
            .ok_or(AccountError::NotFound)?;
        self.client_with(&token)
    }
}

/// Runs a blocking keychain call off the async runtime.
async fn spawn_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, SecretError> + Send + 'static,
) -> Result<T, SecretError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| SecretError::Keychain(err.to_string()))?
}

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
        let accounts = Accounts::with_api_base(
            Store::open_in_memory().unwrap(),
            Arc::new(secrets.clone()),
            &server.uri(),
        );
        (server, accounts, secrets)
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

        accounts.remove("a1").await.unwrap();
        assert_eq!(secrets.keys(), ["cf:a2:apiToken"]);
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
