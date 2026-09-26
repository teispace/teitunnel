//! Zone rulesets (the Ruleset Engine): custom rules, rate limiting and header rules live
//! in each phase's entry point ruleset.
//!
//! Teitunnel changes rules one at a time (add, change or delete by id) and never
//! replaces a whole ruleset, so rules someone else made are never touched. Shapes per
//! the [Rulesets API](https://developers.cloudflare.com/ruleset-engine/rulesets-api/).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Client, Result};

/// Custom rules (block, challenge).
pub const PHASE_CUSTOM: &str = "http_request_firewall_custom";
/// Rate limiting rules.
pub const PHASE_RATE_LIMIT: &str = "http_ratelimit";
/// Request header rules (Transform Rules).
pub const PHASE_REQUEST_HEADERS: &str = "http_request_late_transform";
/// Response header rules (Transform Rules).
pub const PHASE_RESPONSE_HEADERS: &str = "http_response_headers_transform";
/// URL rewrites (Transform Rules): read only, they count towards the same quota.
pub const PHASE_URL_REWRITE: &str = "http_request_transform";
/// Cache Rules (<https://developers.cloudflare.com/cache/how-to/cache-rules/create-api/>).
pub const PHASE_CACHE: &str = "http_request_cache_settings";

/// A rate limiting rule's counter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimit {
    /// What requests are counted by (`cf.colo.id` is always first).
    pub characteristics: Vec<String>,
    /// Counting period, in seconds.
    pub period: u32,
    /// Requests allowed per period.
    pub requests_per_period: u32,
    /// How long the action lasts, in seconds.
    pub mitigation_timeout: u32,
}

/// A rule as read.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Rule {
    /// Rule id.
    pub id: String,
    /// What it does: `block`, `managed_challenge`, `rewrite`, …
    #[serde(default)]
    pub action: String,
    /// When it applies (Rules language).
    #[serde(default)]
    pub expression: String,
    /// Free text; Teitunnel's rules carry a `teitunnel:` marker here.
    #[serde(default)]
    pub description: String,
    /// Whether it's on.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// The action's settings (headers for `rewrite`), kept as they are.
    #[serde(default)]
    pub action_parameters: Option<Value>,
    /// Rate limiting settings.
    #[serde(default)]
    pub ratelimit: Option<RateLimit>,
}

fn enabled() -> bool {
    true
}

/// A rule to create, or the whole definition of one to change (Cloudflare replaces
/// every field on a change).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRule {
    /// What it does.
    pub action: String,
    /// When it applies.
    pub expression: String,
    /// Description (Teitunnel's marker).
    pub description: String,
    /// Whether it's on.
    pub enabled: bool,
    /// The action's settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_parameters: Option<Value>,
    /// Rate limiting settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ratelimit: Option<RateLimit>,
}

impl Rule {
    /// This rule as a definition that recreates it.
    pub fn to_new(&self) -> NewRule {
        NewRule {
            action: self.action.clone(),
            expression: self.expression.clone(),
            description: self.description.clone(),
            enabled: self.enabled,
            action_parameters: self.action_parameters.clone(),
            ratelimit: self.ratelimit.clone(),
        }
    }
}

/// A phase's entry point ruleset.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Ruleset {
    /// Ruleset id.
    pub id: String,
    /// Its phase.
    #[serde(default)]
    pub phase: String,
    /// Its rules, in order.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

fn entrypoint_path(zone: &str, phase: &str) -> String {
    format!(
        "/zones/{}/rulesets/phases/{}/entrypoint",
        crate::encode(zone),
        crate::encode(phase)
    )
}

fn rules_path(zone: &str, ruleset: &str) -> String {
    format!(
        "/zones/{}/rulesets/{}/rules",
        crate::encode(zone),
        crate::encode(ruleset)
    )
}

/// The rule whose description is `description`, last one first (a rule Teitunnel just
/// added is at the end, or where it asked).
fn find(ruleset: &Ruleset, description: &str) -> Result<Rule> {
    ruleset
        .rules
        .iter()
        .rev()
        .find(|r| r.description == description)
        .cloned()
        .ok_or_else(|| {
            crate::Error::Decode(serde::de::Error::custom(
                "the ruleset doesn't contain the rule just written",
            ))
        })
}

impl Client {
    /// A zone's entry point ruleset for `phase`; `None` when the zone has none yet.
    ///
    /// # Errors
    /// API or network errors (403 without the permission).
    pub async fn phase_entrypoint(&self, zone: &str, phase: &str) -> Result<Option<Ruleset>> {
        match self.get(&entrypoint_path(zone, phase)).await {
            Ok(ruleset) => Ok(Some(ruleset)),
            Err(err) if err.status() == Some(404) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Adds one rule to a phase: to its entry point ruleset `ruleset` (at 1-based
    /// `index`, or at the end), or, when the zone has none (`None`), by creating the
    /// entry point with just this rule. Returns the ruleset's id and the rule.
    ///
    /// # Errors
    /// API or network errors (e.g. the plan's quota is used up).
    pub async fn create_rule(
        &self,
        zone: &str,
        phase: &str,
        ruleset: Option<&str>,
        rule: &NewRule,
        index: Option<u32>,
    ) -> Result<(String, Rule)> {
        let written: Ruleset = match ruleset {
            Some(id) => {
                let mut body = serde_json::to_value(rule)?;
                if let (Some(index), Some(object)) = (index, body.as_object_mut()) {
                    object.insert("position".into(), json!({ "index": index }));
                }
                self.post(&rules_path(zone, id), &body).await?
            }
            None => {
                self.post(
                    &format!("/zones/{}/rulesets", crate::encode(zone)),
                    &json!({
                        "name": "default",
                        "kind": "zone",
                        "phase": phase,
                        "rules": [rule],
                    }),
                )
                .await?
            }
        };
        let created = find(&written, &rule.description)?;
        Ok((written.id, created))
    }

    /// Replaces one rule's definition (every field, as Cloudflare requires).
    ///
    /// # Errors
    /// API or network errors (404 when the rule is gone).
    pub async fn update_rule(
        &self,
        zone: &str,
        ruleset: &str,
        rule_id: &str,
        rule: &NewRule,
    ) -> Result<Rule> {
        let written: Ruleset = self
            .patch(
                &format!("{}/{}", rules_path(zone, ruleset), crate::encode(rule_id)),
                &serde_json::to_value(rule)?,
            )
            .await?;
        written
            .rules
            .into_iter()
            .find(|r| r.id == rule_id)
            .ok_or_else(|| {
                crate::Error::Decode(serde::de::Error::custom("the changed rule is missing"))
            })
    }

    /// Deletes one rule (a missing one counts as deleted).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_rule(&self, zone: &str, ruleset: &str, rule_id: &str) -> Result<()> {
        self.delete(&format!(
            "{}/{}",
            rules_path(zone, ruleset),
            crate::encode(rule_id)
        ))
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
            "success": true, "errors": [], "messages": [], "result": result
        }))
    }

    fn rule() -> NewRule {
        NewRule {
            action: "managed_challenge".into(),
            expression: r#"(http.host eq "app.xyz.com") and (not cf.client.bot)"#.into(),
            description: "teitunnel:abc:challenge".into(),
            enabled: true,
            action_parameters: None,
            ratelimit: None,
        }
    }

    async fn client() -> (MockServer, Client) {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        (server, client)
    }

    #[tokio::test]
    async fn reads_an_entrypoint_or_none() {
        let (server, client) = client().await;
        Mock::given(method("GET"))
            .and(path(
                "/zones/z1/rulesets/phases/http_request_firewall_custom/entrypoint",
            ))
            .respond_with(ok(&json!({
                "id": "rs1", "phase": "http_request_firewall_custom",
                "rules": [
                    {"id": "r1", "action": "block", "expression": "ip.src eq 1.2.3.4",
                     "description": "theirs", "enabled": true, "version": "1"},
                    {"id": "r2", "action": "rewrite", "expression": "true",
                     "action_parameters": {"headers": {"X-A": {"operation": "remove"}}}}
                ]
            })))
            .mount(&server)
            .await;
        let found = client
            .phase_entrypoint("z1", PHASE_CUSTOM)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.id, "rs1");
        assert_eq!(found.rules[0].description, "theirs");
        assert!(found.rules[1].enabled, "enabled when absent");
        assert_eq!(
            found.rules[1].to_new().action_parameters,
            Some(json!({"headers": {"X-A": {"operation": "remove"}}}))
        );

        Mock::given(method("GET"))
            .and(path("/zones/z1/rulesets/phases/http_ratelimit/entrypoint"))
            .respond_with(ResponseTemplate::new(404).set_body_json(json!({
                "success": false, "errors": [{"code": 10003, "message": "not found"}],
                "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        assert_eq!(
            client
                .phase_entrypoint("z1", PHASE_RATE_LIMIT)
                .await
                .unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn adds_a_rule_without_touching_the_others() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/zones/z1/rulesets/rs1/rules"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["description"], "teitunnel:abc:challenge");
                assert_eq!(body["position"], json!({"index": 2}));
                assert!(body.get("ratelimit").is_none());
                ok(&json!({"id": "rs1", "rules": [
                    {"id": "r1", "action": "block", "expression": "x", "description": "theirs"},
                    {"id": "new", "action": body["action"], "expression": body["expression"],
                     "description": body["description"]}
                ]}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let (ruleset, created) = client
            .create_rule("z1", PHASE_CUSTOM, Some("rs1"), &rule(), Some(2))
            .await
            .unwrap();
        assert_eq!((ruleset.as_str(), created.id.as_str()), ("rs1", "new"));
    }

    #[tokio::test]
    async fn creates_the_entrypoint_with_only_our_rule() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/zones/z1/rulesets"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["kind"], "zone");
                assert_eq!(body["phase"], "http_ratelimit");
                assert_eq!(body["rules"].as_array().unwrap().len(), 1);
                assert_eq!(
                    body["rules"][0]["ratelimit"]["characteristics"],
                    json!(["cf.colo.id", "ip.src"])
                );
                ok(&json!({"id": "rs9", "rules": [
                    {"id": "r9", "action": "block", "expression": body["rules"][0]["expression"],
                     "description": body["rules"][0]["description"],
                     "ratelimit": body["rules"][0]["ratelimit"]}
                ]}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let limit = NewRule {
            action: "block".into(),
            expression: r#"(http.host in {"a.xyz.com"})"#.into(),
            description: "teitunnel:ratelimit".into(),
            enabled: true,
            action_parameters: None,
            ratelimit: Some(RateLimit {
                characteristics: vec!["cf.colo.id".into(), "ip.src".into()],
                period: 60,
                requests_per_period: 30,
                mitigation_timeout: 60,
            }),
        };
        let (ruleset, created) = client
            .create_rule("z1", PHASE_RATE_LIMIT, None, &limit, None)
            .await
            .unwrap();
        assert_eq!(ruleset, "rs9");
        assert_eq!(created.ratelimit.unwrap().requests_per_period, 30);
    }

    #[tokio::test]
    async fn changes_and_deletes_one_rule_by_id() {
        let (server, client) = client().await;
        Mock::given(method("PATCH"))
            .and(path("/zones/z1/rulesets/rs1/rules/r2"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                // The whole definition travels, as Cloudflare requires.
                assert_eq!(body["enabled"], true);
                assert_eq!(body["action"], "managed_challenge");
                ok(&json!({"id": "rs1", "rules": [
                    {"id": "r1", "action": "block", "expression": "x"},
                    {"id": "r2", "action": "managed_challenge", "expression": body["expression"],
                     "description": body["description"]}
                ]}))
            })
            .expect(1)
            .mount(&server)
            .await;
        let changed = client
            .update_rule("z1", "rs1", "r2", &rule())
            .await
            .unwrap();
        assert_eq!(changed.id, "r2");

        Mock::given(method("DELETE"))
            .and(path("/zones/z1/rulesets/rs1/rules/r2"))
            .respond_with(ok(&json!({"id": "rs1", "rules": []})))
            .expect(1)
            .mount(&server)
            .await;
        client.delete_rule("z1", "rs1", "r2").await.unwrap();
    }

    #[tokio::test]
    async fn reports_cloudflares_refusal() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/zones/z1/rulesets/rs1/rules"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "success": false,
                "errors": [{"code": 20217, "message": "exceeded maximum number of rules"}],
                "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        let err = client
            .create_rule("z1", PHASE_CUSTOM, Some("rs1"), &rule(), None)
            .await
            .unwrap_err();
        assert_eq!(err.status(), Some(400));
    }
}
