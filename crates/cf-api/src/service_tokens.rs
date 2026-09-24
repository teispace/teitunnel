//! Access service tokens: credentials for machines (CI, scripts, other servers) to pass
//! an Access application with `CF-Access-Client-Id` and `CF-Access-Client-Secret`
//! headers. Cloudflare shows a token's secret only when it's created or rotated.

use std::fmt;

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Client, Result};

/// A service token as listed (never with its secret).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ServiceToken {
    /// Token id (what Access policies refer to).
    pub id: String,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// The `CF-Access-Client-Id` value.
    #[serde(default)]
    pub client_id: String,
    /// When it stops working (RFC 3339).
    #[serde(default)]
    pub expires_at: Option<String>,
    /// When it was created (RFC 3339).
    #[serde(default)]
    pub created_at: Option<String>,
}

/// A token with its secret, right after it was created or rotated.
#[derive(Clone, PartialEq, Eq, Deserialize)]
pub struct IssuedServiceToken {
    /// The token.
    #[serde(flatten)]
    pub token: ServiceToken,
    /// The `CF-Access-Client-Secret` value: shown once, never stored by Teitunnel.
    pub client_secret: String,
}

impl fmt::Debug for IssuedServiceToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedServiceToken")
            .field("token", &self.token)
            .field("client_secret", &"[redacted]")
            .finish()
    }
}

/// An Access rule that lets in the service token `id`.
pub fn service_token_rule(id: &str) -> Value {
    json!({ "service_token": { "token_id": id } })
}

/// The service token id a rule lets in, if it's a service token rule.
pub fn rule_service_token(rule: &Value) -> Option<&str> {
    rule.pointer("/service_token/token_id")?.as_str()
}

fn tokens_path(account: &str) -> String {
    format!("/accounts/{}/access/service_tokens", crate::encode(account))
}

impl Client {
    /// The account's service tokens.
    ///
    /// # Errors
    /// API or network errors (403 without Access: Service Tokens).
    pub async fn service_tokens(&self, account: &str) -> Result<Vec<ServiceToken>> {
        self.get_all(&tokens_path(account)).await
    }

    /// Creates a service token valid for `duration` (e.g. `8760h`).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn create_service_token(
        &self,
        account: &str,
        name: &str,
        duration: &str,
    ) -> Result<IssuedServiceToken> {
        self.post(
            &tokens_path(account),
            &json!({ "name": name, "duration": duration }),
        )
        .await
    }

    /// Gives a token a new secret (the client id stays; the old secret stops working).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn rotate_service_token(
        &self,
        account: &str,
        id: &str,
    ) -> Result<IssuedServiceToken> {
        self.post(
            &format!("{}/{}/rotate", tokens_path(account), crate::encode(id)),
            &json!({}),
        )
        .await
    }

    /// Deletes a token (a missing one counts as deleted).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_service_token(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", tokens_path(account), crate::encode(id)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::ApiToken;

    fn ok(result: &Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result,
            "result_info": {"page": 1, "per_page": 50, "total_pages": 1}
        }))
    }

    #[tokio::test]
    async fn creates_lists_rotates_and_deletes() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/service_tokens"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["name"], "Teitunnel · api.xyz.com · CI");
                assert_eq!(body["duration"], "8760h");
                ok(&json!({
                    "id": "tok1", "name": body["name"], "client_id": "abc.access",
                    "client_secret": "s3cret", "expires_at": "2027-09-24T00:00:00Z"
                }))
            })
            .expect(1)
            .mount(&server)
            .await;
        let issued = client
            .create_service_token("a1", "Teitunnel · api.xyz.com · CI", "8760h")
            .await
            .unwrap();
        assert_eq!(issued.token.client_id, "abc.access");
        assert_eq!(issued.client_secret, "s3cret");
        assert!(
            !format!("{issued:?}").contains("s3cret"),
            "the secret never reaches logs"
        );

        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/service_tokens"))
            .respond_with(ok(&json!([
                {"id": "tok1", "name": "Teitunnel · api.xyz.com · CI", "client_id": "abc.access",
                 "expires_at": "2027-09-24T00:00:00Z", "duration": "8760h"}
            ])))
            .mount(&server)
            .await;
        let listed = client.service_tokens("a1").await.unwrap();
        assert_eq!(listed[0].id, "tok1");

        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/service_tokens/tok1/rotate"))
            .respond_with(ok(&json!({
                "id": "tok1", "client_id": "abc.access", "client_secret": "n3w"
            })))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            client
                .rotate_service_token("a1", "tok1")
                .await
                .unwrap()
                .client_secret,
            "n3w"
        );

        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/access/service_tokens/tok1"))
            .respond_with(ok(&json!({"id": "tok1"})))
            .expect(1)
            .mount(&server)
            .await;
        client.delete_service_token("a1", "tok1").await.unwrap();
    }

    #[tokio::test]
    async fn a_missing_permission_is_an_auth_error() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/service_tokens"))
            .respond_with(ResponseTemplate::new(403).set_body_json(json!({
                "success": false, "errors": [{"code": 10000, "message": "Authentication error"}],
                "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        assert!(client.service_tokens("a1").await.unwrap_err().is_auth());
    }

    #[test]
    fn service_token_rules_round_trip() {
        assert_eq!(
            rule_service_token(&service_token_rule("tok1")),
            Some("tok1")
        );
        assert_eq!(rule_service_token(&json!({"email": {"email": "x"}})), None);
    }
}
