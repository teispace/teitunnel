//! The authorization server end to end, through the provider Lens calls.

use std::{
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use http::{HeaderMap, Method, StatusCode, Uri, header};
use lens::{OAuthProvider, ReservedRequest, TapId};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{clients::Fetch, *};

const CLIENT: &str = "https://client.test/oauth/metadata.json";
const REDIRECT: &str = "https://client.test/callback";
const VERIFIER: &str = "a-verifier-that-is-long-enough-for-pkce-0123456789";

fn challenge() -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(VERIFIER.as_bytes()))
}

struct Document;

impl Fetch for Document {
    fn fetch<'a>(
        &'a self,
        url: &'a str,
    ) -> Pin<Box<dyn Future<Output = super::clients::Fetched> + Send + 'a>> {
        let body = serde_json::to_vec(&json!({
            "client_id": url,
            "client_name": "Test Client",
            "redirect_uris": [REDIRECT, "http://127.0.0.1:3000/callback"],
            "token_endpoint_auth_method": "none",
        }))
        .unwrap();
        Box::pin(async move { Ok((body, None)) })
    }
}

/// Answers every consent with `allow`, and remembers what it was asked.
#[derive(Default)]
struct Person {
    allow: bool,
    asked: Mutex<Vec<ConsentRequest>>,
}

impl Approver for Person {
    fn approve(&self, request: ConsentRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> {
        self.asked.lock().unwrap().push(request);
        let allow = self.allow;
        Box::pin(async move { allow })
    }
}

async fn server(allow: bool) -> (McpAuth, Arc<Person>, Store) {
    let store = Store::open_in_memory().unwrap();
    let person = Arc::new(Person {
        allow,
        ..Person::default()
    });
    let auth = McpAuth::with_fetch(store.clone(), person.clone(), Arc::new(Document))
        .await
        .unwrap();
    (auth, person, store)
}

async fn call(
    provider: &Arc<dyn OAuthProvider>,
    method: Method,
    target: &str,
    body: &str,
    headers: HeaderMap,
) -> (StatusCode, HeaderMap, String) {
    let response = provider
        .handle(ReservedRequest {
            tap: TapId::new("t").unwrap(),
            method,
            uri: target.parse::<Uri>().unwrap(),
            headers,
            body: bytes::Bytes::from(body.to_owned()),
            client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)),
        })
        .await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = String::from_utf8(body_of(response).await.to_vec()).unwrap();
    (status, headers, body)
}

async fn get(provider: &Arc<dyn OAuthProvider>, target: &str) -> (StatusCode, HeaderMap, String) {
    call(provider, Method::GET, target, "", HeaderMap::new()).await
}

async fn post(
    provider: &Arc<dyn OAuthProvider>,
    target: &str,
    form: &[(&str, &str)],
) -> (StatusCode, Value) {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (k, v) in form {
        serializer.append_pair(k, v);
    }
    let (status, _, body) = call(
        provider,
        Method::POST,
        target,
        &serializer.finish(),
        HeaderMap::new(),
    )
    .await;
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

fn authorize_url(client: &str, redirect: &str, challenge: &str) -> String {
    protocol::with_query(
        protocol::AUTHORIZE,
        &[
            ("response_type", "code"),
            ("client_id", client),
            ("redirect_uri", redirect),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", "st4te"),
            ("resource", "https://mcp.xyz.com/mcp"),
        ],
    )
}

/// Authorizes and follows the waiting page until it redirects; returns the query.
async fn authorize(provider: &Arc<dyn OAuthProvider>, client: &str) -> HashMap<String, String> {
    let (status, _, page) = get(provider, &authorize_url(client, REDIRECT, &challenge())).await;
    assert_eq!(status, StatusCode::OK, "{page}");
    let request = page
        .split("request=")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap()
        .to_owned();
    for _ in 0..100 {
        let (status, headers, _) =
            get(provider, &format!("{}?request={request}", protocol::WAIT)).await;
        if status == StatusCode::SEE_OTHER {
            let location = headers[header::LOCATION].to_str().unwrap().to_owned();
            assert!(location.starts_with(REDIRECT), "{location}");
            return protocol::form(location.split_once('?').unwrap().1);
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("never answered");
}

async fn exchange(provider: &Arc<dyn OAuthProvider>, code: &str) -> (StatusCode, Value) {
    post(
        provider,
        protocol::TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("client_id", CLIENT),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
            ("resource", "https://mcp.xyz.com/mcp"),
        ],
    )
    .await
}

#[tokio::test]
async fn publishes_metadata_where_mcp_clients_look() {
    let (auth, _, _) = server(true).await;
    let provider = auth.provider("MCP.xyz.com", "/mcp", "docs");
    assert!(provider.handles("/.well-known/oauth-protected-resource/mcp"));
    assert!(provider.handles("/__teitunnel/oauth/token"));
    assert!(!provider.handles("/mcp"));
    assert!(
        !provider.handles("/oauth/token"),
        "the server's own paths stay its own"
    );
    assert_eq!(
        provider.challenge(),
        "Bearer resource_metadata=\"https://mcp.xyz.com/.well-known/oauth-protected-resource\""
    );
    for path in [
        "/.well-known/oauth-protected-resource",
        "/.well-known/oauth-protected-resource/mcp",
    ] {
        let (status, headers, body) = get(&provider, path).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        let metadata: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(metadata["resource"], "https://mcp.xyz.com/mcp");
    }
    for path in [
        "/.well-known/oauth-authorization-server",
        "/.well-known/openid-configuration",
    ] {
        let (_, _, body) = get(&provider, path).await;
        let metadata: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(metadata["issuer"], "https://mcp.xyz.com");
    }
    let (status, _, _) = call(
        &provider,
        Method::OPTIONS,
        protocol::TOKEN,
        "",
        HeaderMap::new(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_client_connects_once_the_person_approves() {
    let (auth, person, _) = server(true).await;
    let provider = auth.provider("mcp.xyz.com", "/mcp", "docs");
    let other = auth.provider("other.xyz.com", "/mcp", "other");
    let answer = authorize(&provider, CLIENT).await;
    assert_eq!(answer["state"], "st4te");
    assert_eq!(answer["iss"], "https://mcp.xyz.com");
    let asked = person.asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].client_name, "Test Client");
    assert_eq!(asked[0].published_by.as_deref(), Some("client.test"));
    assert_eq!(asked[0].redirect_host, "client.test");
    assert!(!asked[0].redirect_loopback);

    let (status, tokens) = exchange(&provider, &answer["code"]).await;
    assert_eq!(status, StatusCode::OK, "{tokens}");
    assert_eq!(tokens["token_type"], "Bearer");
    let access = tokens["access_token"].as_str().unwrap();
    assert!(provider.valid(access));
    assert!(!other.valid(access), "a token is for its own server only");
    assert!(!provider.valid("ttat_made-up"));

    let connections = connections(&auth.inner.store, Some("mcp.xyz.com"))
        .await
        .unwrap();
    assert_eq!(connections.len(), 1);
    assert_eq!(connections[0].client_name, "Test Client");

    // The same code again: someone intercepted it. The connection it made ends.
    let (status, error) = exchange(&provider, &answer["code"]).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
    assert!(!provider.valid(access));
    assert!(connections_of(&auth).await.is_empty());
}

async fn connections_of(auth: &McpAuth) -> Vec<McpConnection> {
    connections(&auth.inner.store, None).await.unwrap()
}

#[tokio::test]
async fn refuses_what_doesnt_match() {
    let (auth, _, _) = server(true).await;
    let provider = auth.provider("mcp.xyz.com", "/mcp", "docs");

    // A redirect the client didn't list is shown an error, never sent there.
    let (status, headers, page) = get(
        &provider,
        &authorize_url(CLIENT, "https://evil.test/cb", &challenge()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(headers.get(header::LOCATION).is_none());
    assert!(page.contains("doesn&#39;t match"));

    // Without PKCE the client hears why, at its own redirect.
    let (status, headers, _) = get(&provider, &authorize_url(CLIENT, REDIRECT, "plain")).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap();
    assert!(location.contains("error=invalid_request"), "{location}");
    assert!(location.contains("iss=https%3A%2F%2Fmcp.xyz.com"));

    // A wrong verifier.
    let answer = authorize(&provider, CLIENT).await;
    let (status, error) = post(
        &provider,
        protocol::TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("code", &answer["code"]),
            ("client_id", CLIENT),
            ("redirect_uri", REDIRECT),
            (
                "code_verifier",
                "another-verifier-that-is-long-enough-for-pkce-01",
            ),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");

    // A token for another server.
    let answer = authorize(&provider, CLIENT).await;
    let (_, error) = post(
        &provider,
        protocol::TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("code", &answer["code"]),
            ("client_id", CLIENT),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
            ("resource", "https://bank.example.com"),
        ],
    )
    .await;
    assert_eq!(error["error"], "invalid_target");
}

#[tokio::test]
async fn a_refusal_goes_back_to_the_client() {
    let (auth, _, _) = server(false).await;
    let provider = auth.provider("mcp.xyz.com", "/mcp", "docs");
    let answer = authorize(&provider, CLIENT).await;
    assert_eq!(answer["error"], "access_denied");
    assert!(!answer.contains_key("code"));
}

#[tokio::test]
async fn refresh_tokens_rotate_and_a_replay_ends_the_connection() {
    let (auth, _, store) = server(true).await;
    let provider = auth.provider("mcp.xyz.com", "/mcp", "docs");
    let answer = authorize(&provider, CLIENT).await;
    let (_, first) = exchange(&provider, &answer["code"]).await;
    let refresh = |token: String| {
        let provider = Arc::clone(&provider);
        async move {
            post(
                &provider,
                protocol::TOKEN,
                &[
                    ("grant_type", "refresh_token"),
                    ("refresh_token", &token),
                    ("client_id", CLIENT),
                ],
            )
            .await
        }
    };
    let (status, second) = refresh(first["refresh_token"].as_str().unwrap().to_owned()).await;
    assert_eq!(status, StatusCode::OK, "{second}");
    assert!(
        !provider.valid(first["access_token"].as_str().unwrap()),
        "only the newest works"
    );
    assert!(provider.valid(second["access_token"].as_str().unwrap()));

    // It survives a restart: a new server over the same database knows the token.
    let again = McpAuth::with_fetch(store, Arc::new(Person::default()), Arc::new(Document))
        .await
        .unwrap();
    assert!(
        again
            .provider("mcp.xyz.com", "/mcp", "docs")
            .valid(second["access_token"].as_str().unwrap())
    );

    // The first refresh token again: it leaked. Everything stops.
    let (status, error) = refresh(first["refresh_token"].as_str().unwrap().to_owned()).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(error["error"], "invalid_grant");
    assert!(!provider.valid(second["access_token"].as_str().unwrap()));
    let (_, error) = refresh(second["refresh_token"].as_str().unwrap().to_owned()).await;
    assert_eq!(error["error"], "invalid_grant");
}

#[tokio::test]
async fn registered_clients_with_a_secret_must_send_it() {
    let (auth, _, _) = server(true).await;
    let provider = auth.provider("mcp.xyz.com", "/mcp", "docs");
    let (status, _, body) = call(
        &provider,
        Method::POST,
        protocol::REGISTER,
        &json!({
            "client_name": "ChatGPT",
            "redirect_uris": [REDIRECT],
            "token_endpoint_auth_method": "client_secret_post",
        })
        .to_string(),
        HeaderMap::new(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let registered: Value = serde_json::from_str(&body).unwrap();
    let client_id = registered["client_id"].as_str().unwrap().to_owned();
    let secret = registered["client_secret"].as_str().unwrap().to_owned();
    assert!(client_id.starts_with("ttcl_"));

    let answer = authorize(&provider, &client_id).await;
    let (status, error) = post(
        &provider,
        protocol::TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("code", &answer["code"]),
            ("client_id", &client_id),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error["error"], "invalid_client");
    // The code was spent on the failed attempt; a fresh one with the secret works.
    let answer = authorize(&provider, &client_id).await;
    let (status, tokens) = post(
        &provider,
        protocol::TOKEN,
        &[
            ("grant_type", "authorization_code"),
            ("code", &answer["code"]),
            ("client_id", &client_id),
            ("redirect_uri", REDIRECT),
            ("code_verifier", VERIFIER),
            ("client_secret", &secret),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{tokens}");

    // Unknown clients are refused at the door.
    let (status, _, _) = get(
        &provider,
        &authorize_url("ttcl_unknown", REDIRECT, &challenge()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn connections_end_when_revoked_or_disconnected() {
    let (auth, _, store) = server(true).await;
    let provider = auth.provider("mcp.xyz.com", "/mcp", "docs");
    let answer = authorize(&provider, CLIENT).await;
    let (_, tokens) = exchange(&provider, &answer["code"]).await;
    let access = tokens["access_token"].as_str().unwrap().to_owned();
    let (status, _) = post(&provider, protocol::REVOKE, &[("token", &access)]).await;
    assert_eq!(status, StatusCode::OK);
    assert!(!provider.valid(&access));
    assert!(connections_of(&auth).await.is_empty());

    // Disconnected from the app, in another process: this one notices at its next check.
    let answer = authorize(&provider, CLIENT).await;
    let (_, tokens) = exchange(&provider, &answer["code"]).await;
    let access = tokens["access_token"].as_str().unwrap().to_owned();
    let id = connections_of(&auth).await[0].id.clone();
    assert!(disconnect(&store, &id).await.unwrap());
    assert!(provider.valid(&access), "until the next check");
    auth.inner.sync().await.unwrap();
    assert!(!provider.valid(&access));
}

#[tokio::test]
async fn waiting_requests_are_bounded() {
    #[derive(Default)]
    struct Never;
    impl Approver for Never {
        fn approve(&self, _: ConsentRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> {
            Box::pin(std::future::pending())
        }
    }
    let store = Store::open_in_memory().unwrap();
    let auth = McpAuth::with_fetch(store, Arc::new(Never), Arc::new(Document))
        .await
        .unwrap();
    let provider = auth.provider("mcp.xyz.com", "/mcp", "docs");
    for _ in 0..MAX_WAITING {
        let (status, _, _) = get(&provider, &authorize_url(CLIENT, REDIRECT, &challenge())).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _, page) = get(&provider, &authorize_url(CLIENT, REDIRECT, &challenge())).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert!(page.contains("waiting"));
}
