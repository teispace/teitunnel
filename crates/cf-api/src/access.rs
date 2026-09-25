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
    /// An account-level policy that applications refer to by id (as read).
    #[serde(default, skip_serializing)]
    pub reusable: bool,
    /// How many applications use it (reusable policies, as read).
    #[serde(default, skip_serializing)]
    pub app_count: Option<u32>,
}

/// Prefix of the names Teitunnel gives its applications and their policies; a reusable
/// policy with it is Teitunnel's to delete once no application uses it.
pub const TEITUNNEL_PREFIX: &str = "Teitunnel · ";

/// An application as sent: `destinations` (Cloudflare's replacement for
/// `self_hosted_domains`) alongside `domain`, and its policies by reference.
#[derive(Serialize)]
struct AppBody<'a> {
    name: &'a str,
    domain: &'a str,
    destinations: [Value; 1],
    #[serde(rename = "type")]
    kind: &'a str,
    session_duration: &'a str,
    app_launcher_visible: bool,
    policies: Vec<Value>,
}

impl<'a> AppBody<'a> {
    fn new(app: &'a NewAccessApp, policies: &[String]) -> Self {
        Self {
            name: &app.name,
            domain: &app.domain,
            destinations: [json!({ "type": "public", "uri": app.domain })],
            kind: &app.kind,
            session_duration: &app.session_duration,
            app_launcher_visible: app.app_launcher_visible,
            policies: policies
                .iter()
                .zip(1u32..)
                .map(|(id, precedence)| json!({ "id": id, "precedence": precedence }))
                .collect(),
        }
    }
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

fn policies_path(account: &str) -> String {
    format!("/accounts/{}/access/policies", crate::encode(account))
}

/// Teitunnel's reusable policies on `app` that nothing else uses, except `keep`.
fn unused_ours(app: &AccessApp, keep: &[String]) -> Vec<String> {
    app.policies
        .iter()
        .filter(|p| p.reusable && p.name.starts_with(TEITUNNEL_PREFIX))
        .filter(|p| p.app_count.is_none_or(|count| count <= 1))
        .filter_map(|p| p.id.clone())
        .filter(|id| !keep.contains(id))
        .collect()
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

    /// One application.
    ///
    /// # Errors
    /// API or network errors (404 when it's gone).
    pub async fn access_app(&self, account: &str, id: &str) -> Result<AccessApp> {
        self.get(&format!("{}/{}", apps_path(account), crate::encode(id)))
            .await
    }

    /// Creates an application with its policies as reusable policies (Cloudflare's model;
    /// the dashboard no longer makes application-scoped ones). If the application can't
    /// be created, the policies made for it are deleted again.
    ///
    /// # Errors
    /// API errors, e.g. when the account has no Zero Trust organization.
    pub async fn create_access_app(&self, account: &str, app: &NewAccessApp) -> Result<AccessApp> {
        let policies = self.create_policies(account, app).await?;
        let created = self
            .post(
                &apps_path(account),
                &serde_json::to_value(AppBody::new(app, &policies))?,
            )
            .await;
        if created.is_err() {
            self.delete_policies(account, &policies).await;
        }
        created
    }

    /// Replaces an application: new reusable policies, then the application pointing at
    /// them, then Teitunnel's policies it no longer uses are deleted.
    ///
    /// # Errors
    /// API or network errors; the application is unchanged then.
    pub async fn update_access_app(
        &self,
        account: &str,
        id: &str,
        app: &NewAccessApp,
    ) -> Result<AccessApp> {
        let before = self.access_app(account, id).await?;
        let policies = self.create_policies(account, app).await?;
        let updated: Result<AccessApp> = self
            .put(
                &format!("{}/{}", apps_path(account), crate::encode(id)),
                &serde_json::to_value(AppBody::new(app, &policies))?,
            )
            .await;
        match updated {
            Ok(updated) => {
                self.delete_policies(account, &unused_ours(&before, &policies))
                    .await;
                Ok(updated)
            }
            Err(err) => {
                self.delete_policies(account, &policies).await;
                Err(err)
            }
        }
    }

    /// Deletes an application (a missing one counts as deleted), then Teitunnel's
    /// reusable policies that only it used.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_access_app(&self, account: &str, id: &str) -> Result<()> {
        let before = match self.access_app(account, id).await {
            Ok(app) => Some(app),
            Err(err) if err.status() == Some(404) => None,
            Err(err) => return Err(err),
        };
        self.delete(&format!("{}/{}", apps_path(account), crate::encode(id)))
            .await?;
        if let Some(before) = before {
            self.delete_policies(account, &unused_ours(&before, &[]))
                .await;
        }
        Ok(())
    }

    /// Creates `app`'s policies as reusable ones named after it, in order. On a failure
    /// the ones already made are deleted.
    async fn create_policies(&self, account: &str, app: &NewAccessApp) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        for policy in &app.policies {
            let body = json!({
                "name": format!("{} · {}", app.name, policy.name),
                "decision": policy.decision,
                "include": policy.include,
            });
            match self
                .post::<AccessPolicy>(&policies_path(account), &body)
                .await
                .and_then(|created| {
                    created
                        .id
                        .ok_or_else(|| serde::de::Error::custom("the policy has no id"))
                        .map_err(crate::Error::Decode)
                }) {
                Ok(id) => ids.push(id),
                Err(err) => {
                    self.delete_policies(account, &ids).await;
                    return Err(err);
                }
            }
        }
        Ok(ids)
    }

    /// Best effort: a policy left behind is harmless (and named after its application).
    async fn delete_policies(&self, account: &str, ids: &[String]) {
        for id in ids {
            let path = format!("{}/{}", policies_path(account), crate::encode(id));
            if let Err(err) = self.delete(&path).await {
                tracing::warn!(error = %err, "couldn't delete an Access policy");
            }
        }
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
            Err(err) if err.status() == Some(404) || err.is_not_enabled() => Ok(None),
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
            .and(path("/accounts/a1/access/policies"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["name"], "Teitunnel · api.xyz.com · Allowed");
                assert_eq!(body["decision"], "allow");
                assert_eq!(
                    body["include"][1],
                    json!({"email_domain": {"domain": "xyz.com"}})
                );
                ok(&json!({"id": "pol2", "name": body["name"], "decision": "allow", "reusable": true}))
            })
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/apps"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["type"], "self_hosted");
                assert_eq!(body["app_launcher_visible"], false);
                assert_eq!(
                    body["destinations"],
                    json!([{"type": "public", "uri": "api.xyz.com"}])
                );
                assert_eq!(body["policies"], json!([{"id": "pol2", "precedence": 1}]));
                ok(&json!({"id": "app2", "domain": body["domain"], "type": "self_hosted"}))
            })
            .mount(&server)
            .await;
        assert_eq!(
            client.create_access_app("a1", &new_app()).await.unwrap().id,
            "app2"
        );

        // Deleting takes Teitunnel's policy along, not someone else's or a shared one.
        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/apps/app2"))
            .respond_with(ok(&json!({
                "id": "app2", "name": "Teitunnel · api.xyz.com", "domain": "api.xyz.com",
                "policies": [
                    {"id": "pol2", "name": "Teitunnel · api.xyz.com · Allowed", "decision": "allow",
                     "reusable": true, "app_count": 1},
                    {"id": "shared", "name": "Teitunnel · team", "decision": "allow",
                     "reusable": true, "app_count": 3},
                    {"id": "theirs", "name": "Admins", "decision": "allow",
                     "reusable": true, "app_count": 1}
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/access/apps/app2"))
            .respond_with(ok(&json!({"id": "app2"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/access/policies/pol2"))
            .respond_with(ok(&json!({"id": "pol2"})))
            .expect(1)
            .mount(&server)
            .await;
        client.delete_access_app("a1", "app2").await.unwrap();
    }

    fn new_app() -> NewAccessApp {
        NewAccessApp {
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
                reusable: false,
                app_count: None,
            }],
        }
    }

    #[tokio::test]
    async fn a_failed_create_leaves_no_policy_behind() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/policies"))
            .respond_with(ok(&json!({"id": "pol9", "name": "x", "decision": "allow"})))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/apps"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "success": false, "errors": [{"code": 12130, "message": "domain taken"}],
                "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/access/policies/pol9"))
            .respond_with(ok(&json!({"id": "pol9"})))
            .expect(1)
            .mount(&server)
            .await;
        assert!(client.create_access_app("a1", &new_app()).await.is_err());
    }

    #[tokio::test]
    async fn an_update_swaps_in_new_policies_and_drops_the_old_ones() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        // An application made by 0.1 (policy inside the app) or by this version.
        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/apps/app2"))
            .respond_with(ok(&json!({
                "id": "app2", "name": "Teitunnel · api.xyz.com", "domain": "api.xyz.com",
                "policies": [
                    {"id": "old", "name": "Teitunnel · api.xyz.com · Allowed", "decision": "allow",
                     "reusable": true, "app_count": 1},
                    {"id": "inline", "name": "Allowed people", "decision": "allow"}
                ]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/access/policies"))
            .respond_with(ok(&json!({"id": "new", "name": "x", "decision": "allow"})))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/accounts/a1/access/apps/app2"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["policies"], json!([{"id": "new", "precedence": 1}]));
                ok(&json!({"id": "app2", "domain": "api.xyz.com"}))
            })
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/access/policies/old"))
            .respond_with(ok(&json!({"id": "old"})))
            .expect(1)
            .mount(&server)
            .await;
        client
            .update_access_app("a1", "app2", &new_app())
            .await
            .unwrap();
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
