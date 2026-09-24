//! Typed endpoints. Models keep only the fields Teitunnel uses and tolerate the rest,
//! so additive API changes never break decoding.

use serde::{Deserialize, Serialize};

use crate::{Client, Result};

/// `GET /user/tokens/verify` result.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TokenStatus {
    /// Token id (not the secret).
    pub id: String,
    /// `active`, `disabled` or `expired`.
    pub status: String,
    /// Expiry, if the token has one (RFC 3339).
    #[serde(default)]
    pub expires_on: Option<String>,
}

impl TokenStatus {
    /// Whether the token can be used now.
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}

/// A Cloudflare account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Account id.
    pub id: String,
    /// Display name.
    pub name: String,
}

/// A reference to a zone's owner account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountRef {
    /// Account id.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
}

/// A zone's plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Plan name, e.g. "Free Website".
    #[serde(default)]
    pub name: String,
}

/// Zone activation state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ZoneStatus {
    /// Serving through Cloudflare.
    Active,
    /// Waiting for the registrar to point nameservers at Cloudflare.
    Pending,
    /// Being set up.
    Initializing,
    /// Nameservers moved away from Cloudflare.
    Moved,
    /// Anything newer we don't know about.
    #[serde(other)]
    Unknown,
}

/// A zone (domain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Zone {
    /// Zone id.
    pub id: String,
    /// Domain name, e.g. `example.com`.
    pub name: String,
    /// Activation state.
    pub status: ZoneStatus,
    /// Whether Cloudflare's proxy is paused for the zone.
    #[serde(default)]
    pub paused: bool,
    /// `full` (Cloudflare DNS) or `partial` (CNAME setup).
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    /// Cloudflare nameservers assigned to the zone.
    #[serde(default)]
    pub name_servers: Vec<String>,
    /// Nameservers found at the registrar before moving to Cloudflare.
    #[serde(default)]
    pub original_name_servers: Option<Vec<String>>,
    /// Owner account.
    pub account: AccountRef,
    /// Plan.
    #[serde(default)]
    pub plan: Option<Plan>,
}

impl Client {
    /// Checks the token is valid and active.
    ///
    /// # Errors
    /// API or network errors (401/403 mean the token was rejected).
    pub async fn verify_token(&self) -> Result<TokenStatus> {
        self.get("/user/tokens/verify").await
    }

    /// Checks an account-owned token, which only verifies under its account (the user
    /// endpoint answers "Invalid API Token" for it).
    ///
    /// # Errors
    /// API or network errors (401/403 mean the token was rejected).
    pub async fn verify_account_token(&self, account_id: &str) -> Result<TokenStatus> {
        self.get(&format!("/accounts/{}/tokens/verify", encode(account_id)))
            .await
    }

    /// Accounts this credential can access.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn accounts(&self) -> Result<Vec<Account>> {
        self.get_all("/accounts").await
    }

    /// Zones in `account_id`.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn zones(&self, account_id: &str) -> Result<Vec<Zone>> {
        self.get_all(&format!("/zones?account.id={}", encode(account_id)))
            .await
    }

    /// Every zone the credential can see, across accounts.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn all_zones(&self) -> Result<Vec<Zone>> {
        self.get_all("/zones").await
    }

    /// One zone.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn zone(&self, zone_id: &str) -> Result<Zone> {
        self.get(&format!("/zones/{}", encode(zone_id))).await
    }
}

/// Percent-encodes an id for use in a path or query (ids are hex, but never trust input).
pub(crate) fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{path, query_param},
    };

    use super::*;
    use crate::ApiToken;

    const ZONES: &str = include_str!("../fixtures/zones.json");

    async fn client(server: &MockServer) -> Client {
        Client::with_base(&server.uri(), ApiToken::new("t")).unwrap()
    }

    #[tokio::test]
    async fn verifies_tokens() {
        let server = MockServer::start().await;
        Mock::given(path("/user/tokens/verify"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"success":true,"errors":[],"messages":[{"code":10000,"message":"This API Token is valid and active"}],"result":{"id":"ed17574386854bf78a67040be0a770b0","status":"active"}}"#,
            ))
            .mount(&server)
            .await;
        let status = client(&server).await.verify_token().await.unwrap();
        assert!(status.is_active());
    }

    #[tokio::test]
    async fn lists_zones_with_status_and_nameservers() {
        let server = MockServer::start().await;
        Mock::given(path("/zones"))
            .and(query_param("account.id", "acc1"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ZONES))
            .mount(&server)
            .await;
        let zones = client(&server).await.zones("acc1").await.unwrap();
        assert_eq!(zones.len(), 2);
        assert_eq!(zones[0].status, ZoneStatus::Active);
        assert_eq!(zones[1].status, ZoneStatus::Pending);
        assert_eq!(
            zones[1].name_servers,
            ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"]
        );
        assert_eq!(zones[0].plan.as_ref().unwrap().name, "Free Website");
    }

    #[test]
    fn unknown_zone_states_decode() {
        let status: ZoneStatus = serde_json::from_str(r#""deactivated""#).unwrap();
        assert_eq!(status, ZoneStatus::Unknown);
    }

    #[test]
    fn encodes_ids() {
        assert_eq!(
            encode("023e105f4ecef8ad9ca31a8372d0c353"),
            "023e105f4ecef8ad9ca31a8372d0c353"
        );
        assert_eq!(encode("a/../b?x"), "a%2F%2E%2E%2Fb%3Fx");
    }
}
