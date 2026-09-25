//! OAuth 2.0 Authorization Code + PKCE (S256) with a loopback redirect (M2-04).
//!
//! Cloudflare requires an exact redirect match and rejects custom URL schemes, so the
//! app registers `http://127.0.0.1:{53682,53683,53684}/callback` and listens on the
//! first free one, on 127.0.0.1 only, for exactly one callback.

use std::{net::Ipv4Addr, time::Duration};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use crate::Secret;

use crate::text::{Text, UserText, english_display, msg, msg::oauth_page as page};

/// Redirect ports registered with the OAuth client (docs/research/cloudflare.md).
pub const REDIRECT_PORTS: [u16; 3] = [53682, 53683, 53684];
/// How long to wait for the browser to come back.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// The public client "Teitunnel" in the Teispace Cloudflare account (registered
/// 2026-09-24, research/cloudflare.md); `TEITUNNEL_OAUTH_CLIENT_ID` overrides it for tests.
/// A client id isn't a secret: PKCE protects the flow.
const CLIENT_ID: Option<&str> = Some("57fe3059e8fc6db30d9e3e07e50e94ea");

/// Scopes requested at sign-in, as Cloudflare names them (the client's scope list,
/// checked 2026-09-25). The first four are required by the client; the rest are
/// optional, so one consent screen covers every feature, people can decline any of them,
/// and capability probing shows what's missing with an in-place fix.
///
/// Only scopes registered on the client may be asked for: an unregistered one fails the
/// sign-in.
const SCOPES: &[&str] = &[
    // Required: tunnels, route DNS records, domains, and finding the accounts.
    "argotunnel.write",
    "dns.write",
    "zone.read",
    "account-settings.read",
    // A refresh token, so the sign-in lasts.
    "offline_access",
    // Optional: require a login. Access apps and policies live on the account
    // (`access-app`, `access-policy`); login methods and the team domain are
    // `access-acct`; `zone-access` covers zone-level apps.
    "access-app.write",
    "access-policy.write",
    "access-acct.write",
    "zone-access.write",
    // Optional: edge protection (custom and rate limiting rules, header rules, service
    // tokens).
    "zone-waf.write",
    "zone-transform-rules.write",
    "access-service-token.write",
    // Optional: Snapshots and offline pages (Workers and their routes), comments and
    // webhook inboxes (D1).
    "workers-scripts.write",
    "workers-routes.write",
    "d1.write",
    // Optional: traffic charts.
    "analytics.read",
    "account-analytics.read",
    // Optional: private networks, and the Doctor's WARP checks (read only).
    "teams-networks.write",
    "teams.read",
    // Optional: load balancing across machines.
    "load-balancers.write",
    "load-balancing-monitors-and-pools.write",
];

/// Errors from the OAuth flow. Messages are shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum OAuthError {
    /// No redirect port is free.
    NoPort,
    /// The browser didn't come back in time.
    Timeout,
    /// The user declined, or Cloudflare reported an error.
    Denied(String),
    /// The callback didn't match this sign-in (possible forgery).
    StateMismatch,
    /// The token endpoint failed.
    Exchange(String),
    /// Cloudflare issued no refresh token, so the sign-in can't last.
    NoOfflineAccess,
    /// The callback carried no authorization code.
    NoCode,
    /// Randomness unavailable (should never happen).
    Random,
}

impl UserText for OAuthError {
    fn text(&self) -> Text {
        match self {
            Self::NoPort => {
                msg::error::oauth::no_port(REDIRECT_PORTS.map(|p| p.to_string()).join(", "))
            }
            Self::Timeout => msg::error::oauth::timeout(),
            Self::Denied(reason) => msg::error::oauth::denied(reason),
            Self::StateMismatch => msg::error::oauth::state_mismatch(),
            Self::Exchange(detail) => msg::error::oauth::exchange(detail),
            Self::NoOfflineAccess => msg::error::oauth::no_offline_access(),
            Self::NoCode => msg::error::oauth::no_code(),
            Self::Random => msg::error::oauth::random(),
        }
    }
}

english_display!(OAuthError);

/// Endpoints and client settings.
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    /// Public client id.
    pub client_id: String,
    /// Authorization endpoint.
    pub authorize_url: String,
    /// Token endpoint.
    pub token_url: String,
    /// Revocation endpoint.
    pub revoke_url: String,
    /// Requested scopes.
    pub scopes: Vec<String>,
    /// Loopback ports to try, in order.
    pub ports: Vec<u16>,
}

impl OAuthConfig {
    /// Cloudflare's endpoints with the registered client, if there is one.
    pub fn cloudflare() -> Option<Self> {
        let client_id = std::env::var("TEITUNNEL_OAUTH_CLIENT_ID")
            .ok()
            .or(CLIENT_ID.map(str::to_owned))?;
        Some(Self::with_base(client_id, "https://dash.cloudflare.com"))
    }

    /// Endpoints under `base` (tests use a mock server).
    pub fn with_base(client_id: String, base: &str) -> Self {
        Self {
            client_id,
            authorize_url: format!("{base}/oauth2/auth"),
            token_url: format!("{base}/oauth2/token"),
            revoke_url: format!("{base}/oauth2/revoke"),
            scopes: SCOPES.iter().map(|s| (*s).to_owned()).collect(),
            ports: REDIRECT_PORTS.to_vec(),
        }
    }
}

fn random_token(bytes: usize) -> Result<String, OAuthError> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|_| OAuthError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// S256 challenge for a PKCE verifier.
pub fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

fn form_encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A started sign-in, waiting for the browser.
#[derive(Debug)]
pub struct PendingLogin {
    /// The URL to open in the browser.
    pub authorize_url: String,
    redirect_uri: String,
    state: String,
    verifier: Secret<String>,
    listener: TcpListener,
}

/// What the browser brought back.
#[derive(Debug)]
pub struct AuthCode {
    code: Secret<String>,
    verifier: Secret<String>,
    redirect_uri: String,
}

/// Tokens from the token endpoint.
#[derive(Debug)]
pub struct TokenSet {
    /// Short-lived access token.
    pub access_token: Secret<String>,
    /// Long-lived refresh token (with `offline_access`).
    pub refresh_token: Option<Secret<String>>,
    /// Access token lifetime.
    pub expires_in: Duration,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct TokenError {
    #[serde(default)]
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// Starts a sign-in: binds a loopback port and builds the authorize URL.
///
/// # Errors
/// [`OAuthError::NoPort`] if every registered port is busy.
pub async fn start(config: &OAuthConfig) -> Result<PendingLogin, OAuthError> {
    let mut bound = None;
    for port in &config.ports {
        if let Ok(listener) = TcpListener::bind((Ipv4Addr::LOCALHOST, *port)).await {
            bound = Some((listener, *port));
            break;
        }
    }
    let (listener, port) = bound.ok_or(OAuthError::NoPort)?;
    let redirect_uri = format!("http://127.0.0.1:{port}/callback");
    let verifier = random_token(48)?; // 64 characters
    let state = random_token(24)?;
    let authorize_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        config.authorize_url,
        form_encode(&config.client_id),
        form_encode(&redirect_uri),
        form_encode(&config.scopes.join(" ")),
        state,
        challenge(&verifier),
    );
    Ok(PendingLogin {
        authorize_url,
        redirect_uri,
        state,
        verifier: Secret::new(verifier),
        listener,
    })
}

const PAGE: &str = r#"<!doctype html><html lang="LANG"><head><meta charset="utf-8"><title>Teitunnel</title><style>
:root{color-scheme:light dark}body{font:15px -apple-system,system-ui,sans-serif;display:grid;place-items:center;height:100vh;margin:0;color:light-dark(#1d1d1f,#f5f5f7);background:light-dark(#fff,#1e1e1e)}
main{text-align:center;max-width:22rem}h1{font-size:17px;font-weight:600;margin:0 0 6px}p{margin:0;opacity:.6}
</style></head><body><main><h1>TITLE</h1><p>BODY</p></main></body></html>"#;

/// Escapes text for HTML (translations are text, never markup).
fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn respond(stream: &mut TcpStream, status: &str, title: Text, body: Text) {
    let page = PAGE
        .replace("LANG", &escape(&crate::text::language()))
        .replace("TITLE", &escape(&title.to_string()))
        .replace("BODY", &escape(&body.to_string()));
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}",
        page.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key).then(|| percent_decode(v))
    })
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let decoded = (bytes[i] == b'%' && i + 2 < bytes.len())
            .then(|| std::str::from_utf8(&bytes[i + 1..i + 3]).ok())
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[i], decoded) {
            (_, Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (b'+', None) => {
                out.push(b' ');
                i += 1;
            }
            (byte, None) => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl PendingLogin {
    /// Waits for the browser to hit `/callback` (other paths get 404), up to `timeout`.
    ///
    /// # Errors
    /// Timeout, a denied consent, or a state mismatch.
    pub async fn wait(self, timeout: Duration) -> Result<AuthCode, OAuthError> {
        tokio::time::timeout(timeout, self.accept())
            .await
            .map_err(|_| OAuthError::Timeout)?
    }

    async fn accept(self) -> Result<AuthCode, OAuthError> {
        loop {
            let Ok((mut stream, _)) = self.listener.accept().await else {
                continue;
            };
            let mut buf = vec![0u8; 8192];
            let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..read]);
            let target = request.split_whitespace().nth(1).unwrap_or("/");
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            if path != "/callback" {
                respond(
                    &mut stream,
                    "404 Not Found",
                    page::not_found(),
                    msg::raw(""),
                )
                .await;
                continue;
            }
            if query_param(query, "state").as_deref() != Some(self.state.as_str()) {
                respond(
                    &mut stream,
                    "400 Bad Request",
                    page::mismatch(),
                    page::return_and_retry(),
                )
                .await;
                return Err(OAuthError::StateMismatch);
            }
            if let Some(error) = query_param(query, "error") {
                respond(&mut stream, "200 OK", page::cancelled(), page::close_tab()).await;
                return Err(OAuthError::Denied(
                    query_param(query, "error_description").unwrap_or(error),
                ));
            }
            let Some(code) = query_param(query, "code").filter(|c| !c.is_empty()) else {
                respond(
                    &mut stream,
                    "400 Bad Request",
                    page::incomplete(),
                    page::return_and_retry(),
                )
                .await;
                return Err(OAuthError::NoCode);
            };
            respond(&mut stream, "200 OK", page::signed_in(), page::close_tab()).await;
            return Ok(AuthCode {
                code: Secret::new(code),
                verifier: self.verifier,
                redirect_uri: self.redirect_uri,
            });
        }
    }
}

async fn token_request(
    http: &reqwest::Client,
    config: &OAuthConfig,
    form: &[(&str, &str)],
) -> Result<TokenSet, OAuthError> {
    let body: String = form
        .iter()
        .map(|(k, v)| format!("{k}={}", form_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let response = http
        .post(&config.token_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|err| OAuthError::Exchange(err.without_url().to_string()))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .map_err(|err| OAuthError::Exchange(err.to_string()))?;
    if !status.is_success() {
        let detail = serde_json::from_slice::<TokenError>(&bytes)
            .map(|e| e.error_description.unwrap_or(e.error))
            .unwrap_or_else(|_| format!("HTTP {}", status.as_u16()));
        return Err(OAuthError::Exchange(detail));
    }
    let tokens: TokenResponse =
        serde_json::from_slice(&bytes).map_err(|err| OAuthError::Exchange(err.to_string()))?;
    Ok(TokenSet {
        access_token: Secret::new(tokens.access_token),
        refresh_token: tokens.refresh_token.map(Secret::new),
        expires_in: Duration::from_secs(tokens.expires_in.unwrap_or(3600)),
    })
}

/// Exchanges the authorization code (with the PKCE verifier) for tokens.
///
/// # Errors
/// [`OAuthError::Exchange`] with Cloudflare's reason.
pub async fn exchange(
    http: &reqwest::Client,
    config: &OAuthConfig,
    code: AuthCode,
) -> Result<TokenSet, OAuthError> {
    token_request(
        http,
        config,
        &[
            ("grant_type", "authorization_code"),
            ("code", code.code.expose()),
            ("redirect_uri", &code.redirect_uri),
            ("client_id", &config.client_id),
            ("code_verifier", code.verifier.expose()),
        ],
    )
    .await
}

/// Gets a new access token (and possibly a rotated refresh token).
///
/// # Errors
/// [`OAuthError::Exchange`] (e.g. the refresh token was revoked).
pub async fn refresh(
    http: &reqwest::Client,
    config: &OAuthConfig,
    refresh_token: &Secret<String>,
) -> Result<TokenSet, OAuthError> {
    token_request(
        http,
        config,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose()),
            ("client_id", &config.client_id),
        ],
    )
    .await
}

/// Revokes a token (best effort; sign-out proceeds regardless).
pub async fn revoke(http: &reqwest::Client, config: &OAuthConfig, token: &Secret<String>) {
    let body = format!(
        "token={}&client_id={}",
        form_encode(token.expose()),
        form_encode(&config.client_id)
    );
    if let Err(err) = http
        .post(&config.revoke_url)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
    {
        tracing::warn!(error = %err.without_url(), "token revocation failed");
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    fn free_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    }

    fn config(base: &str, ports: Vec<u16>) -> OAuthConfig {
        OAuthConfig {
            ports,
            ..OAuthConfig::with_base("teitunnel-test".into(), base)
        }
    }

    fn param(url: &str, key: &str) -> String {
        query_param(url.split_once('?').unwrap().1, key).unwrap()
    }

    /// Plays the browser: follows the redirect back to the loopback server.
    async fn browser(url: String) -> u16 {
        reqwest::get(url).await.unwrap().status().as_u16()
    }

    #[tokio::test]
    async fn full_flow_with_pkce() {
        let server = MockServer::start().await;
        let config = config(&server.uri(), vec![free_port()]);
        let login = start(&config).await.unwrap();
        let url = login.authorize_url.clone();
        assert!(url.starts_with(&format!("{}/oauth2/auth?response_type=code", server.uri())));
        assert_eq!(param(&url, "code_challenge_method"), "S256");
        let challenge_sent = param(&url, "code_challenge");
        let redirect = param(&url, "redirect_uri");
        assert!(redirect.starts_with("http://127.0.0.1:") && redirect.ends_with("/callback"));

        let waiting = tokio::spawn(login.wait(Duration::from_secs(5)));
        // Unrelated requests (favicon) don't end the sign-in.
        assert_eq!(
            browser(redirect.replace("/callback", "/favicon.ico")).await,
            404
        );
        let status = browser(format!(
            "{redirect}?code=the-code&state={}",
            param(&url, "state")
        ))
        .await;
        assert_eq!(status, 200);
        let code = waiting.await.unwrap().unwrap();

        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(move |req: &Request| {
                let body = String::from_utf8_lossy(&req.body).into_owned();
                let verifier = query_param(&body, "code_verifier").unwrap_or_default();
                assert_eq!(challenge(&verifier), challenge_sent, "verifier matches the challenge");
                assert_eq!(query_param(&body, "code").as_deref(), Some("the-code"));
                assert_eq!(query_param(&body, "grant_type").as_deref(), Some("authorization_code"));
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "access_token": "access", "refresh_token": "refresh", "expires_in": 3600, "token_type": "bearer"
                }))
            })
            .mount(&server)
            .await;
        let tokens = exchange(&reqwest::Client::new(), &config, code)
            .await
            .unwrap();
        assert_eq!(tokens.access_token.expose(), "access");
        assert_eq!(tokens.refresh_token.unwrap().expose(), "refresh");
        assert_eq!(tokens.expires_in, Duration::from_secs(3600));
    }

    #[tokio::test]
    async fn rejects_a_forged_state() {
        let config = config("http://unused", vec![free_port()]);
        let login = start(&config).await.unwrap();
        let redirect = param(&login.authorize_url, "redirect_uri");
        let waiting = tokio::spawn(login.wait(Duration::from_secs(5)));
        assert_eq!(
            browser(format!("{redirect}?code=x&state=forged")).await,
            400
        );
        assert!(matches!(
            waiting.await.unwrap(),
            Err(OAuthError::StateMismatch)
        ));
    }

    #[tokio::test]
    async fn reports_denied_consent() {
        let config = config("http://unused", vec![free_port()]);
        let login = start(&config).await.unwrap();
        let (redirect, state) = (
            param(&login.authorize_url, "redirect_uri"),
            param(&login.authorize_url, "state"),
        );
        let waiting = tokio::spawn(login.wait(Duration::from_secs(5)));
        browser(format!(
            "{redirect}?error=access_denied&error_description=User+said+no&state={state}"
        ))
        .await;
        match waiting.await.unwrap() {
            Err(OAuthError::Denied(reason)) => assert_eq!(reason, "User said no"),
            other => panic!("expected denial, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn times_out() {
        let config = config("http://unused", vec![free_port()]);
        let login = start(&config).await.unwrap();
        assert!(matches!(
            login.wait(Duration::from_millis(100)).await,
            Err(OAuthError::Timeout)
        ));
    }

    #[tokio::test]
    async fn falls_back_to_the_next_free_port() {
        let busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let busy_port = busy.local_addr().unwrap().port();
        let free = free_port();
        let login = start(&config("http://unused", vec![busy_port, free]))
            .await
            .unwrap();
        assert!(param(&login.authorize_url, "redirect_uri").contains(&format!(":{free}/")));
        drop(busy);

        let only_busy = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = only_busy.local_addr().unwrap().port();
        assert!(matches!(
            start(&config("http://unused", vec![port])).await,
            Err(OAuthError::NoPort)
        ));
    }

    #[tokio::test]
    async fn refresh_errors_carry_cloudflares_reason() {
        let server = MockServer::start().await;
        Mock::given(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "error": "invalid_grant", "error_description": "refresh token revoked"
            })))
            .mount(&server)
            .await;
        let err = refresh(
            &reqwest::Client::new(),
            &config(&server.uri(), vec![]),
            &Secret::new("r".into()),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("refresh token revoked"));
    }

    #[test]
    fn challenge_matches_rfc7636_example() {
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn decodes_query_values() {
        assert_eq!(percent_decode("a%20b+c%2Fd%"), "a b c/d%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
