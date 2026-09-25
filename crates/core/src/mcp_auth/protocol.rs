//! The pure parts of the authorization server: metadata documents, PKCE, redirect URI
//! rules, client documents and registrations, forms and OAuth errors.

use std::net::IpAddr;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// Where the endpoints live on the shared hostname (Lens's reserved prefix, so they
/// never shadow the MCP server's own paths).
pub(crate) const PREFIX: &str = "/__teitunnel/oauth";
pub(crate) const AUTHORIZE: &str = "/__teitunnel/oauth/authorize";
pub(crate) const WAIT: &str = "/__teitunnel/oauth/wait";
pub(crate) const TOKEN: &str = "/__teitunnel/oauth/token";
pub(crate) const REGISTER: &str = "/__teitunnel/oauth/register";
pub(crate) const REVOKE: &str = "/__teitunnel/oauth/revoke";
pub(crate) const RESOURCE_METADATA: &str = "/.well-known/oauth-protected-resource";
pub(crate) const SERVER_METADATA: &str = "/.well-known/oauth-authorization-server";
pub(crate) const OPENID_METADATA: &str = "/.well-known/openid-configuration";

/// Longest client name kept.
const MAX_NAME: usize = 100;
/// Most redirect URIs a client may list.
const MAX_REDIRECTS: usize = 10;

/// The issuer for `host`: its HTTPS origin (the tap's public address, never the
/// request's `Host` header).
pub(crate) fn issuer(host: &str) -> String {
    format!("https://{host}")
}

/// RFC 9728 metadata for the MCP server at `resource`.
pub(crate) fn resource_metadata(host: &str, resource: &str, name: &str) -> Value {
    json!({
        "resource": resource,
        "authorization_servers": [issuer(host)],
        "bearer_methods_supported": ["header"],
        "resource_name": name,
    })
}

/// RFC 8414 metadata (also served as OpenID discovery for clients that try it first).
pub(crate) fn server_metadata(host: &str) -> Value {
    let base = issuer(host);
    json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}{AUTHORIZE}"),
        "token_endpoint": format!("{base}{TOKEN}"),
        "registration_endpoint": format!("{base}{REGISTER}"),
        "revocation_endpoint": format!("{base}{REVOKE}"),
        "response_types_supported": ["code"],
        "response_modes_supported": ["query"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "revocation_endpoint_auth_methods_supported": ["none", "client_secret_post", "client_secret_basic"],
        "client_id_metadata_document_supported": true,
        "authorization_response_iss_parameter_supported": true,
    })
}

/// A hash to store for a high-entropy secret (tokens, codes): SHA-256, URL-safe base64.
pub(crate) fn hash(secret: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(secret.as_bytes()))
}

/// A new random secret: `prefix` and 32 random bytes.
pub(crate) fn random(prefix: &str) -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(format!("{prefix}{}", URL_SAFE_NO_PAD.encode(bytes)))
}

/// A short code the person compares between the browser and the app (no look-alikes).
pub(crate) fn short_code() -> Result<String, getrandom::Error> {
    const ALPHABET: &[u8] = b"ACDEFHJKMNPRTUVWXY3479";
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes)?;
    Ok(bytes
        .iter()
        .map(|b| char::from(ALPHABET[usize::from(*b) % ALPHABET.len()]))
        .collect())
}

/// Whether `verifier` answers `challenge` (S256, RFC 7636 §4.6). Verifiers must be
/// 43–128 unreserved characters.
pub(crate) fn pkce_ok(verifier: &str, challenge: &str) -> bool {
    let valid = (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b));
    valid && URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) == challenge
}

/// Whether a code challenge looks like S256 output (43 base64url characters).
pub(crate) fn challenge_ok(challenge: &str) -> bool {
    challenge.len() == 43
        && challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn loopback_host(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || bare.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// Whether a redirect URI is acceptable at all: HTTPS, or HTTP to this computer (native
/// apps), with no fragment or credentials.
pub(crate) fn redirect_allowed(uri: &str) -> bool {
    let Ok(parsed) = uri.parse::<http::Uri>() else {
        return false;
    };
    let Some(host) = parsed.host() else {
        return false;
    };
    if uri.contains('#') || parsed.authority().is_some_and(|a| a.as_str().contains('@')) {
        return false;
    }
    match parsed.scheme_str() {
        Some("https") => true,
        Some("http") => loopback_host(host),
        _ => false,
    }
}

/// Whether `asked` is one of the `registered` redirect URIs: exactly, except that a
/// loopback URI may use any port (RFC 8252 §7.3, OAuth 2.1 §8.4.2).
pub(crate) fn redirect_matches(registered: &[String], asked: &str) -> bool {
    if registered.iter().any(|r| r == asked) {
        return true;
    }
    let Ok(asked_uri) = asked.parse::<http::Uri>() else {
        return false;
    };
    if asked_uri.scheme_str() != Some("http") || !asked_uri.host().is_some_and(loopback_host) {
        return false;
    }
    registered.iter().any(|r| {
        r.parse::<http::Uri>().is_ok_and(|reg| {
            reg.scheme_str() == Some("http")
                && reg.host() == asked_uri.host()
                && reg.path_and_query() == asked_uri.path_and_query()
        })
    })
}

/// The host a redirect goes to, shown to the person.
pub(crate) fn redirect_host(uri: &str) -> String {
    uri.parse::<http::Uri>()
        .ok()
        .and_then(|u| u.host().map(str::to_owned))
        .unwrap_or_default()
}

/// Whether the redirect only goes to this computer (anyone's app could claim it).
pub(crate) fn redirect_is_loopback(uri: &str) -> bool {
    uri.parse::<http::Uri>()
        .ok()
        .and_then(|u| u.host().map(loopback_host))
        .unwrap_or(false)
}

/// Whether `resource` (RFC 8707) names this server: its origin, with or without a path.
pub(crate) fn resource_matches(host: &str, resource: &str) -> bool {
    let Ok(uri) = resource.parse::<http::Uri>() else {
        return false;
    };
    uri.scheme_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("https"))
        && uri.host().is_some_and(|h| h.eq_ignore_ascii_case(host))
        && uri.port_u16().is_none_or(|p| p == 443)
        && !resource.contains('#')
}

/// Whether `client_id` is a Client ID Metadata Document URL: HTTPS with a path.
pub(crate) fn is_document_url(client_id: &str) -> bool {
    client_id.parse::<http::Uri>().is_ok_and(|uri| {
        uri.scheme_str() == Some("https")
            && uri.host().is_some()
            && uri.path() != "/"
            && !uri.path().is_empty()
            && !client_id.contains('#')
            && !uri.authority().is_some_and(|a| a.as_str().contains('@'))
    })
}

/// A client, as the server knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Client {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) redirect_uris: Vec<String>,
    /// For clients registered with a secret.
    pub(crate) secret_hash: Option<String>,
    /// The host that published its metadata (Client ID Metadata Documents).
    pub(crate) published_by: Option<String>,
}

/// Why a client can't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClientError(pub(crate) String);

fn clean_name(name: Option<&str>, fallback: &str) -> String {
    let name: String = name
        .unwrap_or(fallback)
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME)
        .collect();
    let name = name.trim();
    if name.is_empty() {
        fallback.to_owned()
    } else {
        name.to_owned()
    }
}

fn redirect_list(value: Option<&Value>) -> Result<Vec<String>, ClientError> {
    let uris: Vec<String> = value
        .and_then(Value::as_array)
        .ok_or_else(|| ClientError("redirect_uris is required".into()))?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ClientError("redirect_uris must be strings".into()))
        })
        .collect::<Result<_, _>>()?;
    if uris.is_empty() || uris.len() > MAX_REDIRECTS {
        return Err(ClientError(format!(
            "list between 1 and {MAX_REDIRECTS} redirect_uris"
        )));
    }
    if let Some(bad) = uris.iter().find(|u| !redirect_allowed(u)) {
        return Err(ClientError(format!(
            "redirect URI {bad} must use HTTPS, or HTTP to localhost"
        )));
    }
    Ok(uris)
}

/// A Client ID Metadata Document fetched from `url`, checked.
pub(crate) fn client_from_document(url: &str, document: &Value) -> Result<Client, ClientError> {
    if document.get("client_id").and_then(Value::as_str) != Some(url) {
        return Err(ClientError(
            "the document's client_id isn't its own URL".into(),
        ));
    }
    match document
        .get("token_endpoint_auth_method")
        .and_then(Value::as_str)
    {
        None | Some("none") => {}
        Some(other) => {
            return Err(ClientError(format!(
                "token_endpoint_auth_method {other} isn't supported (use none)"
            )));
        }
    }
    let host = url
        .parse::<http::Uri>()
        .ok()
        .and_then(|u| u.host().map(str::to_ascii_lowercase));
    Ok(Client {
        id: url.to_owned(),
        name: clean_name(
            document.get("client_name").and_then(Value::as_str),
            host.as_deref().unwrap_or("An MCP client"),
        ),
        redirect_uris: redirect_list(document.get("redirect_uris"))?,
        secret_hash: None,
        published_by: host,
    })
}

/// What a client asks for when registering (RFC 7591).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Registration {
    pub(crate) name: String,
    pub(crate) redirect_uris: Vec<String>,
    /// `none`, `client_secret_post` or `client_secret_basic`.
    pub(crate) auth_method: String,
}

/// Checks a registration request. Clients that don't say how they authenticate are
/// treated as public (`none`), which is what MCP clients are.
pub(crate) fn registration(body: &Value) -> Result<Registration, ClientError> {
    if !body.is_object() {
        return Err(ClientError("the registration must be a JSON object".into()));
    }
    let auth_method = match body
        .get("token_endpoint_auth_method")
        .and_then(Value::as_str)
    {
        None => "none",
        Some(m @ ("none" | "client_secret_post" | "client_secret_basic")) => m,
        Some(other) => {
            return Err(ClientError(format!(
                "token_endpoint_auth_method {other} isn't supported"
            )));
        }
    }
    .to_owned();
    if let Some(grants) = body.get("grant_types").and_then(Value::as_array)
        && grants
            .iter()
            .any(|g| !matches!(g.as_str(), Some("authorization_code" | "refresh_token")))
    {
        return Err(ClientError(
            "only authorization_code and refresh_token grants are supported".into(),
        ));
    }
    if let Some(types) = body.get("response_types").and_then(Value::as_array)
        && types.iter().any(|t| t.as_str() != Some("code"))
    {
        return Err(ClientError(
            "only the code response type is supported".into(),
        ));
    }
    Ok(Registration {
        name: clean_name(
            body.get("client_name").and_then(Value::as_str),
            "An MCP client",
        ),
        redirect_uris: redirect_list(body.get("redirect_uris"))?,
        auth_method,
    })
}

/// The registration answer (RFC 7591 §3.2.1).
pub(crate) fn registered(
    client_id: &str,
    registration: &Registration,
    secret: Option<&str>,
    issued_at_secs: u64,
) -> Value {
    let mut answer = json!({
        "client_id": client_id,
        "client_id_issued_at": issued_at_secs,
        "client_name": registration.name,
        "redirect_uris": registration.redirect_uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": registration.auth_method,
    });
    if let Some(secret) = secret {
        answer["client_secret"] = json!(secret);
        answer["client_secret_expires_at"] = json!(0);
    }
    answer
}

/// Decodes an `application/x-www-form-urlencoded` body (or query), last value wins.
pub(crate) fn form(text: &str) -> std::collections::HashMap<String, String> {
    form_urlencoded::parse(text.as_bytes())
        .into_owned()
        .collect()
}

/// An OAuth error answer (RFC 6749 §5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct OAuthError {
    pub(crate) error: &'static str,
    pub(crate) error_description: String,
}

impl OAuthError {
    pub(crate) fn new(error: &'static str, description: impl Into<String>) -> Self {
        Self {
            error,
            error_description: description.into(),
        }
    }
}

/// `uri` with `pairs` added to its query.
pub(crate) fn with_query(uri: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = uri.to_owned();
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    let query = serializer.finish();
    out.push(if uri.contains('?') { '&' } else { '?' });
    out.push_str(&query);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_accepts_only_the_matching_verifier() {
        // RFC 7636 appendix B.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";
        assert!(challenge_ok(challenge));
        assert!(pkce_ok(verifier, challenge));
        assert!(!pkce_ok(
            "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXx",
            challenge
        ));
        assert!(!pkce_ok("short", challenge));
        assert!(!challenge_ok("plain-challenge"));
    }

    #[test]
    fn redirects_must_be_https_or_this_computer() {
        assert!(redirect_allowed("https://claude.ai/api/mcp/auth_callback"));
        assert!(redirect_allowed("http://localhost:33418/callback"));
        assert!(redirect_allowed("http://127.0.0.1/cb"));
        assert!(redirect_allowed("http://[::1]:9000/cb"));
        for bad in [
            "http://evil.test/cb",
            "https://a.test/cb#frag",
            "https://user@a.test/cb",
            "javascript:alert(1)",
            "cursor://callback",
        ] {
            assert!(!redirect_allowed(bad), "{bad}");
        }
        let registered = vec![
            "https://claude.ai/api/mcp/auth_callback".to_owned(),
            "http://127.0.0.1:3000/callback".to_owned(),
        ];
        assert!(redirect_matches(
            &registered,
            "https://claude.ai/api/mcp/auth_callback"
        ));
        assert!(!redirect_matches(
            &registered,
            "https://claude.ai/api/mcp/auth_callback/"
        ));
        // Loopback redirects may use another port, never another path or host.
        assert!(redirect_matches(
            &registered,
            "http://127.0.0.1:51234/callback"
        ));
        assert!(!redirect_matches(
            &registered,
            "http://127.0.0.1:51234/other"
        ));
        assert!(!redirect_matches(
            &registered,
            "http://localhost:3000/callback"
        ));
    }

    #[test]
    fn a_resource_is_this_server_or_nothing() {
        assert!(resource_matches("mcp.xyz.com", "https://mcp.xyz.com"));
        assert!(resource_matches("mcp.xyz.com", "https://MCP.xyz.com/mcp"));
        assert!(resource_matches(
            "mcp.xyz.com",
            "https://mcp.xyz.com:443/mcp"
        ));
        assert!(!resource_matches(
            "mcp.xyz.com",
            "https://other.xyz.com/mcp"
        ));
        assert!(!resource_matches("mcp.xyz.com", "http://mcp.xyz.com/mcp"));
        assert!(!resource_matches("mcp.xyz.com", "https://mcp.xyz.com:8443"));
    }

    #[test]
    fn client_documents_must_describe_themselves() {
        let url = "https://claude.ai/oauth/mcp-oauth-client-metadata";
        assert!(is_document_url(url));
        assert!(!is_document_url("https://claude.ai/"));
        assert!(!is_document_url("http://claude.ai/client.json"));
        assert!(!is_document_url("tt_client_abc"));
        let document = json!({
            "client_id": url,
            "client_name": "Claude",
            "redirect_uris": ["https://claude.ai/api/mcp/auth_callback"],
            "token_endpoint_auth_method": "none"
        });
        let client = client_from_document(url, &document).unwrap();
        assert_eq!(client.name, "Claude");
        assert_eq!(client.published_by.as_deref(), Some("claude.ai"));
        let mut other = document.clone();
        other["client_id"] = json!("https://evil.test/client.json");
        assert!(client_from_document(url, &other).is_err());
        let mut jwt = document.clone();
        jwt["token_endpoint_auth_method"] = json!("private_key_jwt");
        assert!(client_from_document(url, &jwt).is_err());
        let mut plain = document;
        plain["redirect_uris"] = json!(["http://evil.test/cb"]);
        assert!(client_from_document(url, &plain).is_err());
    }

    #[test]
    fn registrations_are_public_clients_unless_they_ask() {
        let public = registration(&json!({
            "client_name": "Cursor\u{7}",
            "redirect_uris": ["http://127.0.0.1:53000/callback"],
        }))
        .unwrap();
        assert_eq!(public.auth_method, "none");
        assert_eq!(public.name, "Cursor");
        let secret = registration(&json!({
            "redirect_uris": ["https://chatgpt.com/connector_platform_oauth_redirect"],
            "token_endpoint_auth_method": "client_secret_post",
        }))
        .unwrap();
        assert_eq!(secret.name, "An MCP client");
        let answer = registered("tt_client_x", &secret, Some("s3"), 1);
        assert_eq!(answer["client_secret"], "s3");
        assert!(registration(&json!({ "redirect_uris": [] })).is_err());
        assert!(
            registration(
                &json!({ "redirect_uris": ["https://a.test/cb"], "grant_types": ["password"] })
            )
            .is_err()
        );
    }

    #[test]
    fn metadata_says_what_mcp_clients_look_for() {
        let server = server_metadata("mcp.xyz.com");
        assert_eq!(server["issuer"], "https://mcp.xyz.com");
        assert_eq!(server["code_challenge_methods_supported"], json!(["S256"]));
        assert_eq!(server["client_id_metadata_document_supported"], true);
        assert_eq!(
            server["authorization_response_iss_parameter_supported"],
            true
        );
        assert_eq!(
            server["token_endpoint"],
            "https://mcp.xyz.com/__teitunnel/oauth/token"
        );
        let resource = resource_metadata("mcp.xyz.com", "https://mcp.xyz.com/mcp", "docs");
        assert_eq!(
            resource["authorization_servers"],
            json!(["https://mcp.xyz.com"])
        );
        assert_eq!(
            with_query("https://a.test/cb?x=1", &[("code", "a b"), ("state", "s")]),
            "https://a.test/cb?x=1&code=a+b&state=s"
        );
        assert_eq!(short_code().unwrap().len(), 4);
        assert_eq!(hash("x").len(), 43);
    }
}
