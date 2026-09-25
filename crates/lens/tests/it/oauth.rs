//! OAuth in front of a tap: the provider answers its paths, every other request needs a
//! valid token, and no token reaches the service.

use std::sync::Arc;

use http::{HeaderValue, Method, Response, StatusCode};
use lens::{BearerToken, HandlerFuture, OAuthProvider, ReservedRequest, Responder, Upstream};

use crate::support::*;

#[derive(Debug)]
struct Stub;

impl OAuthProvider for Stub {
    fn handles(&self, path: &str) -> bool {
        path.starts_with("/.well-known/oauth-") || path.starts_with("/oauth/")
    }

    fn handle(&self, request: ReservedRequest) -> HandlerFuture {
        Box::pin(async move {
            let body = format!(
                "{} {} {}",
                request.method,
                request.uri.path(),
                String::from_utf8_lossy(&request.body)
            );
            let mut response = Response::new(lens::full(body));
            *response.status_mut() = StatusCode::OK;
            response
        })
    }

    fn valid(&self, token: &str) -> bool {
        token == "good-access-token"
    }

    fn challenge(&self) -> HeaderValue {
        HeaderValue::from_static(
            "Bearer resource_metadata=\"https://mcp.test/.well-known/oauth-protected-resource\"",
        )
    }
}

/// An origin that says which Authorization header it got.
async fn origin_seeing_auth() -> Origin {
    origin(|request: http::Request<hyper::body::Incoming>| async move {
        let auth = request
            .headers()
            .get("authorization")
            .map_or("none", |v| v.to_str().unwrap())
            .to_owned();
        text_response(200, format!("auth={auth}"))
    })
    .await
}

fn with_token(path: &str, token: &str) -> http::Request<lens::LensBody> {
    request(Method::POST, path)
        .header("authorization", format!("Bearer {token}"))
        .body(body("{}"))
        .unwrap()
}

#[tokio::test]
async fn only_valid_tokens_get_through_and_never_reach_the_service() {
    let origin = origin_seeing_auth().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.oauth = Some(Arc::new(Stub));
        config.gates.bearer = vec![BearerToken::new("static-token-for-clis-0001").unwrap()];
    })
    .await;

    let none = get(tap.addr, "/mcp").await;
    assert_eq!(none.status, StatusCode::UNAUTHORIZED);
    let challenge = none.headers["www-authenticate"].to_str().unwrap();
    assert!(
        challenge.contains("resource_metadata=\"https://mcp.test/"),
        "{challenge}"
    );
    assert!(!challenge.contains("invalid_token"));
    assert_eq!(
        none.headers["access-control-expose-headers"],
        "WWW-Authenticate"
    );
    let exchange = exchange_for(&lens, "/mcp").await;
    assert_eq!(
        exchange.responder,
        Responder::Gate {
            reason: lens::GateOutcome::OAuthRequired
        }
    );

    let wrong = fetch(tap.addr, with_token("/mcp", "stolen")).await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert!(
        wrong.headers["www-authenticate"]
            .to_str()
            .unwrap()
            .ends_with("error=\"invalid_token\"")
    );

    let good = fetch(tap.addr, with_token("/mcp", "good-access-token")).await;
    assert_eq!(good.status, StatusCode::OK);
    assert_eq!(good.text(), "auth=none", "no token passes through");
    let cli = fetch(tap.addr, with_token("/mcp", "static-token-for-clis-0001")).await;
    assert_eq!(cli.text(), "auth=none");

    // A browser's preflight carries no token.
    let preflight = fetch(
        tap.addr,
        request(Method::OPTIONS, "/mcp")
            .header("origin", "https://inspector.test")
            .header("access-control-request-method", "POST")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(preflight.status, StatusCode::OK);
}

#[tokio::test]
async fn the_providers_paths_are_answered_without_keeping_their_secrets() {
    let origin = origin_seeing_auth().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.oauth = Some(Arc::new(Stub));
    })
    .await;
    let reply = fetch(
        tap.addr,
        request(Method::POST, "/oauth/token")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body("grant_type=authorization_code&code=secret-code"))
            .unwrap(),
    )
    .await;
    assert_eq!(
        reply.text(),
        "POST /oauth/token grant_type=authorization_code&code=secret-code"
    );
    let metadata = get(tap.addr, "/.well-known/oauth-protected-resource").await;
    assert_eq!(metadata.status, StatusCode::OK);

    let exchange = exchange_for(&lens, "/oauth/token").await;
    assert!(exchange.request.body.data.is_empty(), "the code isn't kept");
    assert_eq!(exchange.request.body.size, 46);
    let response = exchange.response.as_ref().unwrap();
    assert!(response.body.data.is_empty(), "nor what was answered");
    assert_eq!(response.body.size, 64);
}
