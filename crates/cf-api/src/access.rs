//! Cloudflare Access (Zero Trust) endpoints: the self-hosted applications that put a
//! login in front of a hostname, the organization they need, and login methods.
//!
//! Policy rules stay raw JSON, so applications made elsewhere, with rule types
//! Teitunnel doesn't model, are read and written back unchanged. Shapes per Cloudflare's
//! OpenAPI schema (`cloudflare/api-schemas`, checked 2026-09-23).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Client, Result};

/// An Access application.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AccessApp {
    /// Application id.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// The protected hostname and optional path, e.g. `app.xyz.com/admin`.
    #[serde(default)]
    pub domain: String,
    /// `self_hosted`, `saas`, …
    #[serde(rename = "type", default)]
    pub kind: String,
    /// How long a login lasts, e.g. `24h`.
    #[serde(default)]
    pub session_duration: Option<String>,
    /// Policies, in order of precedence.
    #[serde(default)]
    pub policies: Vec<AccessPolicy>,
}

/// A policy of an application.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccessPolicy {
    /// Policy id (absent when creating one inline).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Name.
    pub name: String,
    /// `allow`, `deny`, `bypass`, `non_identity`.
    pub decision: String,
    /// Who it applies to: any rule matching is enough.
    #[serde(default)]
    pub include: Vec<Value>,
    /// Order among the application's policies.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precedence: Option<u32>,
}

/// A rule matching one email address.
pub fn email_rule(email: &str) -> Value {
    json!({ "email": { "email": email } })
}

/// A rule matching every address at an email domain.
pub fn email_domain_rule(domain: &str) -> Value {
    json!({ "email_domain": { "domain": domain } })
}

/// The email a rule matches, if it's an email rule.
pub fn rule_email(rule: &Value) -> Option<&str> {
    rule.pointer("/email/email")?.as_str()
}

/// The email domain a rule matches, if it's an email-domain rule.
pub fn rule_email_domain(rule: &Value) -> Option<&str> {
    rule.pointer("/email_domain/domain")?.as_str()
}

/// An application to create or replace.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NewAccessApp {
    /// Display name.
    pub name: String,
    /// The protected hostname and optional path.
    pub domain: String,
    /// Always `self_hosted` for tunnel routes.
    #[serde(rename = "type")]
    pub kind: String,
    /// How long a login lasts.
    pub session_duration: String,
    /// Hidden from the App Launcher (a route, not an app people browse to).
    pub app_launcher_visible: bool,
    /// Inline policies.
    pub policies: Vec<AccessPolicy>,
}

/// The account's Zero Trust organization.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AccessOrganization {
    /// Team name.
    #[serde(default)]
    pub name: String,
    /// Login domain, e.g. `myteam.cloudflareaccess.com`.
    #[serde(default)]
    pub auth_domain: String,
}

/// A login method (identity provider).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct IdentityProvider {
    /// Id.
    pub id: String,
    /// Display name.
    #[serde(default)]
    pub name: String,
    /// `onetimepin`, `google`, `github`, …
    #[serde(rename = "type", default)]
    pub kind: String,
}

fn apps_path(account: &str) -> String {
    format!("/accounts/{}/access/apps", crate::encode(account))
}

impl Client {
    /// Access applications for exactly this domain (hostname and optional path).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn access_apps_for(&self, account: &str, domain: &str) -> Result<Vec<AccessApp>> {
        self.get_all(&format!(
            "{}?domain={}&exact=true",
            apps_path(account),
            crate::encode_query(domain)
        ))
        .await
    }

    /// Creates an application. Not retried on server errors (no duplicates).
    ///
    /// # Errors
    /// API errors, e.g. when the account has no Zero Trust organization.
    pub async fn create_access_app(&self, account: &str, app: &NewAccessApp) -> Result<AccessApp> {
        self.post(&apps_path(account), &serde_json::to_value(app)?)
            .await
    }

    /// Replaces an application.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn update_access_app(
        &self,
        account: &str,
        id: &str,
        app: &NewAccessApp,
    ) -> Result<AccessApp> {
        self.put(
            &format!("{}/{}", apps_path(account), crate::encode(id)),
            &serde_json::to_value(app)?,
        )
        .await
    }

    /// Deletes an application (a missing one counts as deleted).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_access_app(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", apps_path(account), crate::encode(id)))
            .await
    }

    /// The account's Zero Trust organization; `None` when Zero Trust isn't set up.
    ///
    /// # Errors
    /// API or network errors other than "not set up".
    pub async fn access_organization(&self, account: &str) -> Result<Option<AccessOrganization>> {
        match self
            .get(&format!(
                "/accounts/{}/access/organizations",
                crate::encode(account)
            ))
            .await
        {
            Ok(org) => Ok(Some(org)),
            Err(err) if err.status() == Some(404) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// The account's login methods.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn identity_providers(&self, account: &str) -> Result<Vec<IdentityProvider>> {
        self.get_all(&format!(
            "/accounts/{}/access/identity_providers",
            crate::encode(account)
        ))
        .await
    }

    /// Adds One-time PIN (a code sent to the visitor's email) as a login method.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn create_one_time_pin(&self, account: &str) -> Result<IdentityProvider> {
        self.post(
            &format!(
                "/accounts/{}/access/identity_providers",
                crate::encode(account)
            ),
            &json!({ "type": "onetimepin", "name": "One-time PIN", "config": {} }),
        )
        .await
    }

    /// Removes a login method.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_identity_provider(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!(
            "/accounts/{}/access/identity_providers/{}",
            crate::encode(account),
            crate::encode(id)
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path, query_param},
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
    async fn manages_applications() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/apps"))
            .and(query_param("domain", "app.xyz.com"))
            .and(query_param("exact", "true"))
            .respond_with(ok(&json!([{
                "id": "app1", "name": "Teitunnel · app.xyz.com", "domain": "app.xyz.com",
                "type": "self_hosted", "session_duration": "24h",
                "policies": [{"id": "p1", "name": "Allowed", "decision": "allow", "precedence": 1,
                              "include": [{"email": {"email": "me@xyz.com"}}, {"ip": {"ip": "10.0.0.0/8"}}]}]
            }])))
            .mount(&server)
            .await;
        let apps = client.access_apps_for("a1", "app.xyz.com").await.unwrap();
        assert_eq!(apps[0].id, "app1");
        let include = &apps[0].policies[0].include;
        assert_eq!(rule_email(&include[0]), Some("me@xyz.com"));
        assert_eq!(
            rule_email(&include[1]),
            None,
            "unmodelled rules are kept as they are"
        );

        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/apps"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["type"], "self_hosted");
                assert_eq!(body["app_launcher_visible"], false);
                assert_eq!(body["policies"][0]["decision"], "allow");
                assert_eq!(
                    body["policies"][0]["include"][1],
                    json!({"email_domain": {"domain": "xyz.com"}})
                );
                assert!(body["policies"][0].get("id").is_none());
                ok(&json!({"id": "app2", "domain": body["domain"], "type": "self_hosted"}))
            })
            .mount(&server)
            .await;
        let app = NewAccessApp {
            name: "Teitunnel · api.xyz.com".into(),
            domain: "api.xyz.com".into(),
            kind: "self_hosted".into(),
            session_duration: "24h".into(),
            app_launcher_visible: false,
            policies: vec![AccessPolicy {
                id: None,
                name: "Allowed".into(),
                decision: "allow".into(),
                include: vec![email_rule("me@xyz.com"), email_domain_rule("xyz.com")],
                precedence: Some(1),
            }],
        };
        assert_eq!(
            client.create_access_app("a1", &app).await.unwrap().id,
            "app2"
        );

        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/access/apps/app2"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        client.delete_access_app("a1", "app2").await.unwrap();
    }

    #[tokio::test]
    async fn knows_when_zero_trust_isnt_set_up() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/organizations"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "success": false, "errors": [{"code": 9999, "message": "not found"}], "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        assert_eq!(client.access_organization("a1").await.unwrap(), None);

        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/identity_providers"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["type"], "onetimepin");
                ok(&json!({"id": "idp1", "name": "One-time PIN", "type": "onetimepin"}))
            })
            .mount(&server)
            .await;
        assert_eq!(
            client.create_one_time_pin("a1").await.unwrap().kind,
            "onetimepin"
        );
    }
}
