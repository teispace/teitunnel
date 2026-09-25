//! OAuth 2.1 in front of a tap (a shared MCP server): an [`OAuthProvider`] answers its own
//! paths (metadata, authorization, token, registration) and says which access tokens are
//! valid; Lens asks for a valid token everywhere else and never forwards it (tokens are
//! for the tap, not the service behind it).

use std::fmt;

use http::{HeaderMap, HeaderValue, Response, StatusCode, header};

use crate::{HandlerFuture, LensBody, ReservedRequest};

/// An OAuth authorization server for one tap. Implemented by the embedder (it keeps
/// clients and grants, and asks the person); Lens only routes and checks.
pub trait OAuthProvider: Send + Sync + fmt::Debug {
    /// Whether it answers `path` itself (`/.well-known/…`, its endpoints).
    fn handles(&self, path: &str) -> bool;
    /// Answers one of its paths. Bodies are at most 1 MiB and are never captured.
    fn handle(&self, request: ReservedRequest) -> HandlerFuture;
    /// Whether `token` is a valid, unexpired access token for this tap. Called on every
    /// request: it must not block.
    fn valid(&self, token: &str) -> bool;
    /// The `WWW-Authenticate` value for a request without a valid token, e.g.
    /// `Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource"`.
    fn challenge(&self) -> HeaderValue;
}

/// The bearer token of a request, if it sends one.
pub(crate) fn bearer(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.split_once(' ')?;
    scheme
        .eq_ignore_ascii_case("bearer")
        .then(|| token.trim())
        .filter(|token| !token.is_empty())
}

/// The answer to a request without a valid token (RFC 6750 §3, RFC 9728 §5.1).
pub(crate) fn unauthorized(provider: &dyn OAuthProvider, token_sent: bool) -> Response<LensBody> {
    let mut response = Response::new(crate::body::full(if token_sent {
        "{\"error\":\"invalid_token\",\"error_description\":\"The access token is invalid or expired.\"}\n"
    } else {
        "{\"error\":\"unauthorized\",\"error_description\":\"Sign in to use this server.\"}\n"
    }));
    *response.status_mut() = StatusCode::UNAUTHORIZED;
    let headers = response.headers_mut();
    let mut challenge = provider.challenge();
    if token_sent
        && let Ok(value) = challenge.to_str()
        && let Ok(with_error) = HeaderValue::from_str(&format!("{value}, error=\"invalid_token\""))
    {
        challenge = with_error;
    }
    headers.insert(header::WWW_AUTHENTICATE, challenge);
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    // Browser-based clients read the challenge to find the metadata.
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("WWW-Authenticate"),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_bearer_tokens_only() {
        let with = |value: &'static str| {
            let mut headers = HeaderMap::new();
            headers.insert(header::AUTHORIZATION, HeaderValue::from_static(value));
            headers
        };
        assert_eq!(bearer(&with("Bearer abc")), Some("abc"));
        assert_eq!(bearer(&with("bearer  abc ")), Some("abc"));
        assert_eq!(bearer(&with("Basic abc")), None);
        assert_eq!(bearer(&with("Bearer ")), None);
        assert_eq!(bearer(&HeaderMap::new()), None);
    }
}
