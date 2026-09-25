//! Permission probing without side effects.
//!
//! API tokens don't expose their permissions to themselves, so Teitunnel asks
//! Cloudflare directly: reads are probed with a one-item list, and writes by PATCHing an
//! id that can't exist (all zeros). Cloudflare checks authorization first, so 403 /
//! code 10000 means "not allowed" and 404 means "allowed, just no such object". Nothing
//! can be created or modified by a probe.

use crate::{Client, Error};

/// The id used for write probes; no real object has it.
pub const NIL_ID: &str = "00000000000000000000000000000000";
/// Tunnel ids are UUIDs.
pub const NIL_UUID: &str = "00000000-0000-0000-0000-000000000000";

/// What a probe learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// The credential may do it.
    Allowed,
    /// The credential may not.
    Denied,
    /// The product isn't turned on for the account (Zero Trust), whatever the credential.
    NotEnabled,
    /// Couldn't tell (network error, outage).
    Unknown,
}

impl Access {
    fn from_result<T>(result: &Result<T, Error>) -> Self {
        match result {
            Ok(_) => Self::Allowed,
            Err(err) if err.is_not_enabled() => Self::NotEnabled,
            Err(err) if err.is_auth() => Self::Denied,
            // Allowed but the object doesn't exist / the request is otherwise invalid.
            Err(err) if matches!(err.status(), Some(400 | 404 | 405 | 409)) => Self::Allowed,
            Err(_) => Self::Unknown,
        }
    }
}

impl Client {
    /// Probes a read endpoint (fetches at most one item).
    pub async fn probe_read(&self, path: &str) -> Access {
        let separator = if path.contains('?') { '&' } else { '?' };
        let result = self
            .get::<serde_json::Value>(&format!("{path}{separator}per_page=1"))
            .await;
        Self::logged(path, "read", &result)
    }

    /// Probes write permission on a collection by PATCHing a non-existent member.
    pub async fn probe_write(&self, member_path: &str) -> Access {
        let result = self
            .patch::<serde_json::Value>(member_path, &serde_json::json!({}))
            .await;
        Self::logged(member_path, "write", &result)
    }

    /// The probe's answer; a denial is logged with Cloudflare's reason (never a
    /// credential), so "why can't I…" can be answered from the log.
    fn logged<T>(path: &str, kind: &str, result: &Result<T, Error>) -> Access {
        let access = Access::from_result(result);
        if access != Access::Allowed
            && let Err(err) = result
        {
            tracing::info!(path, kind, ?access, %err, "permission probe");
        }
        access
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::ApiToken;

    fn err(status: u16, code: u32) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(serde_json::json!({
            "success": false, "errors": [{"code": code, "message": "x"}], "messages": [], "result": null
        }))
    }

    #[tokio::test]
    async fn interprets_statuses() {
        let server = MockServer::start().await;
        Mock::given(method("PATCH"))
            .and(path("/allowed"))
            .respond_with(err(404, 1003))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path("/denied"))
            .respond_with(err(403, 10000))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/list"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"success":true,"errors":[],"messages":[],"result":[]}"#),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/broken"))
            .respond_with(ResponseTemplate::new(418))
            .mount(&server)
            .await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        assert_eq!(client.probe_write("/allowed").await, Access::Allowed);
        assert_eq!(client.probe_write("/denied").await, Access::Denied);
        assert_eq!(client.probe_read("/list").await, Access::Allowed);
        assert_eq!(client.probe_read("/broken").await, Access::Unknown);
    }

    #[tokio::test]
    async fn zero_trust_turned_off_is_not_a_denial() {
        let server = MockServer::start().await;
        // Cloudflare's answer on an account without Zero Trust (seen 2026-09-25).
        let not_enabled = ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "success": false,
            "errors": [{"code": 9999, "message": "access.api.error.not_enabled: Access is not enabled. Visit the Access dashboard at https://dash.cloudflare.com/ and click the 'Enable Access' button."}],
            "messages": [], "result": null
        }));
        for endpoint in ["service_tokens", "organizations"] {
            Mock::given(method("GET"))
                .and(path(format!("/accounts/a1/access/{endpoint}")))
                .respond_with(not_enabled.clone())
                .mount(&server)
                .await;
        }
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        assert_eq!(
            client
                .probe_read("/accounts/a1/access/service_tokens")
                .await,
            Access::NotEnabled
        );
        // Planning a login then says to set up Zero Trust, not to add a permission.
        assert!(client.access_organization("a1").await.unwrap().is_none());
    }
}
