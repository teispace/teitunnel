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
    /// Couldn't tell (network error, outage).
    Unknown,
}

impl Access {
    fn from_result<T>(result: &Result<T, Error>) -> Self {
        match result {
            Ok(_) => Self::Allowed,
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
        Access::from_result(&result)
    }

    /// Probes write permission on a collection by PATCHing a non-existent member.
    pub async fn probe_write(&self, member_path: &str) -> Access {
        let result = self
            .patch::<serde_json::Value>(member_path, &serde_json::json!({}))
            .await;
        Access::from_result(&result)
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
}
