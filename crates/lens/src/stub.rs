//! Stubs: canned responses for matching requests, always or only while the upstream
//! is unreachable (keeps webhook senders happy while a dev server restarts).

use http::{HeaderName, HeaderValue, Method, Response, StatusCode};
use serde::{Deserialize, Serialize};

use crate::{LensBody, LensError, PathPattern, body::full};

/// When a stub answers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum StubMode {
    /// Every matching request, without contacting the upstream.
    Always,
    /// Only when the upstream can't be reached (refused, timeout, DNS, TLS).
    #[default]
    WhenUnreachable,
}

/// A canned response for requests matching a method and path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct StubRule {
    /// Method to match (case-insensitive); `None` matches any.
    pub method: Option<String>,
    /// Path to match.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub path: PathPattern,
    /// When to answer.
    pub mode: StubMode,
    /// Response status.
    pub status: u16,
    /// Response headers.
    pub headers: Vec<(String, String)>,
    /// Response body (text; use a `Content-Type` header to describe it).
    pub body: String,
}

impl StubRule {
    /// A stub answering `path` with `status` and `body`, for any method, when the
    /// upstream is unreachable.
    pub fn new(path: PathPattern, status: u16, body: impl Into<String>) -> Self {
        Self {
            method: None,
            path,
            mode: StubMode::default(),
            status,
            headers: Vec::new(),
            body: body.into(),
        }
    }

    /// Whether the rule applies to `method` and `path`.
    pub fn matches(&self, method: &Method, path: &str) -> bool {
        self.method
            .as_deref()
            .is_none_or(|m| m.eq_ignore_ascii_case(method.as_str()))
            && self.path.matches(path)
    }

    /// Checks status and headers.
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] for a status outside 200–599 or invalid headers.
    pub fn validate(&self) -> Result<(), LensError> {
        if !(200..=599).contains(&self.status) {
            return Err(LensError::InvalidConfig(format!(
                "stub status {} must be between 200 and 599",
                self.status
            )));
        }
        for (name, value) in &self.headers {
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| LensError::InvalidConfig(format!("invalid header name {name:?}")))?;
            HeaderValue::from_str(value).map_err(|_| {
                LensError::InvalidConfig(format!("invalid value for header {name:?}"))
            })?;
        }
        Ok(())
    }

    /// Builds the response (headers that don't parse are skipped; see `validate`).
    pub(crate) fn response(&self) -> Response<LensBody> {
        let mut response = Response::new(full(self.body.clone()));
        *response.status_mut() = StatusCode::from_u16(self.status).unwrap_or(StatusCode::OK);
        let headers = response.headers_mut();
        for (name, value) in &self.headers {
            if let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(name.as_bytes()),
                HeaderValue::from_str(value),
            ) {
                headers.append(name, value);
            }
        }
        headers.insert("x-teitunnel-stub", HeaderValue::from_static("1"));
        response
    }
}

/// The first rule of `mode` matching the request.
pub(crate) fn find<'a>(
    rules: &'a [StubRule],
    mode: StubMode,
    method: &Method,
    path: &str,
) -> Option<(usize, &'a StubRule)> {
    rules
        .iter()
        .enumerate()
        .find(|(_, rule)| rule.mode == mode && rule.matches(method, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(method: Option<&str>, path: &str, mode: StubMode) -> StubRule {
        StubRule {
            method: method.map(str::to_owned),
            mode,
            headers: vec![("content-type".into(), "application/json".into())],
            ..StubRule::new(PathPattern::parse(path).unwrap(), 200, "{\"ok\":true}")
        }
    }

    #[test]
    fn matching_and_order() {
        let rules = vec![
            rule(Some("post"), "/webhooks/*", StubMode::WhenUnreachable),
            rule(None, "/health", StubMode::Always),
        ];
        assert_eq!(
            find(
                &rules,
                StubMode::WhenUnreachable,
                &Method::POST,
                "/webhooks/stripe"
            )
            .map(|(i, _)| i),
            Some(0)
        );
        assert!(
            find(
                &rules,
                StubMode::WhenUnreachable,
                &Method::GET,
                "/webhooks/stripe"
            )
            .is_none()
        );
        assert!(find(&rules, StubMode::Always, &Method::POST, "/webhooks/stripe").is_none());
        assert_eq!(
            find(&rules, StubMode::Always, &Method::GET, "/health").map(|(i, _)| i),
            Some(1)
        );
    }

    #[test]
    fn response_and_validation() {
        let stub = rule(None, "/x", StubMode::Always);
        stub.validate().unwrap();
        let response = stub.response();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(response.headers()["x-teitunnel-stub"], "1");

        let bad_status = StubRule {
            status: 99,
            ..stub.clone()
        };
        assert!(bad_status.validate().is_err());
        let bad_header = StubRule {
            headers: vec![("bad name".into(), "v".into())],
            ..stub
        };
        assert!(bad_header.validate().is_err());
    }

    #[test]
    fn serde_round_trip() {
        let stub = rule(Some("GET"), "re:^/a", StubMode::Always);
        let json = serde_json::to_string(&stub).unwrap();
        let back: StubRule = serde_json::from_str(&json).unwrap();
        assert_eq!(back, stub);
    }
}
