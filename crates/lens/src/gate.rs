//! Protection enforced by Lens before a request is forwarded (M12-04): IP rules,
//! user-agent blocking, and sign-in by password page, secret link or HTTP basic auth,
//! with a bypass list for paths such as webhooks.
//!
//! Order: IP rules, then user agents (these apply to every path), then the bypass list,
//! then sign-in (any configured method admits the visitor).
//!
//! Sessions are a signed cookie: `v1.<expiry>.<hmac>` where the HMAC-SHA256 covers the
//! tap id, the expiry and a fingerprint of the current credentials, under a random key
//! owned by the [`crate::Lens`] instance. Changing a password or link invalidates every
//! session; restarting Lens (a new key) does too.

use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use http::{HeaderMap, HeaderValue, header};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::{LensError, PathPattern, Secret, util::ct_eq};

/// The reserved path of the sign-in form's POST.
pub const LOGIN_PATH: &str = "/__teitunnel/login";
/// Session cookie name when the cookie is `Secure` (a `__Host-` cookie can't be set by
/// another subdomain or path).
const COOKIE_SECURE: &str = "__Host-teitunnel";
/// Session cookie name when it isn't (plain-http local testing).
const COOKIE_PLAIN: &str = "teitunnel_session";
/// Failed passwords allowed per IP per window.
const MAX_FAILURES: u32 = 10;
/// The failure window.
const FAILURE_WINDOW: Duration = Duration::from_secs(10 * 60);
/// IPs tracked by the limiter at most (memory bound).
const MAX_TRACKED: usize = 10_000;

/// A password for the sign-in page, stored as an Argon2id hash (PHC string).
#[derive(Clone, PartialEq, Eq)]
pub struct PasswordGate {
    phc: Secret<String>,
}

impl std::fmt::Debug for PasswordGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PasswordGate([redacted])")
    }
}

impl PasswordGate {
    /// Hashes `password` with Argon2id and a random salt. Slow on purpose (~tens of
    /// milliseconds); call it off the async runtime's hot path.
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] for an empty password, [`LensError::PasswordHash`]
    /// if hashing fails.
    pub fn new(password: &str) -> Result<Self, LensError> {
        if password.is_empty() {
            return Err(LensError::InvalidConfig(
                "the password can't be empty".into(),
            ));
        }
        let salt = crate::util::random_bytes::<16>()?;
        let hash = Argon2::default()
            .hash_password_with_salt(password.as_bytes(), &salt)
            .map_err(|err| LensError::PasswordHash(err.to_string()))?;
        Ok(Self {
            phc: Secret::new(hash.to_string()),
        })
    }

    /// Uses an existing Argon2 PHC hash (e.g. one kept in the keychain).
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] when `phc` isn't a valid PHC string.
    pub fn from_hash(phc: &str) -> Result<Self, LensError> {
        PasswordHash::new(phc)
            .map_err(|err| LensError::InvalidConfig(format!("invalid password hash: {err}")))?;
        Ok(Self {
            phc: Secret::new(phc.to_owned()),
        })
    }

    /// The PHC hash, to store it. It's not the password, but treat it as sensitive.
    pub fn hash(&self) -> &Secret<String> {
        &self.phc
    }

    /// Checks a password (constant-time inside Argon2). Slow; run on a blocking thread.
    pub fn verify(&self, password: &str) -> bool {
        Argon2::default()
            .verify_password(password.as_bytes(), self.phc.expose().as_str())
            .is_ok()
    }
}

/// A secret link: `https://host/any/path?key=<token>` signs the visitor in.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretLink {
    token: Secret<String>,
}

impl std::fmt::Debug for SecretLink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SecretLink([redacted])")
    }
}

impl SecretLink {
    /// Uses `token` (at least 16 characters, URL-safe).
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] for short or non-URL-safe tokens.
    pub fn new(token: &str) -> Result<Self, LensError> {
        let ok = token.len() >= 16
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~'));
        if !ok {
            return Err(LensError::InvalidConfig(
                "a link token needs at least 16 URL-safe characters".into(),
            ));
        }
        Ok(Self {
            token: Secret::new(token.to_owned()),
        })
    }

    /// A new random token (192 bits, base64url).
    ///
    /// # Errors
    /// [`LensError::Random`] if the OS random source fails.
    pub fn generate() -> Result<Self, LensError> {
        let bytes = crate::util::random_bytes::<24>()?;
        Self::new(&URL_SAFE_NO_PAD.encode(bytes))
    }

    /// The token, to build the link to share.
    pub fn token(&self) -> &Secret<String> {
        &self.token
    }

    fn matches(&self, candidate: &str) -> bool {
        let a = ring::digest::digest(&ring::digest::SHA256, self.token.expose().as_bytes());
        let b = ring::digest::digest(&ring::digest::SHA256, candidate.as_bytes());
        ct_eq(a.as_ref(), b.as_ref())
    }
}

/// HTTP basic credentials (for scripts and machine callers).
#[derive(Clone, PartialEq, Eq)]
pub struct BasicAuth {
    user: String,
    password: Secret<String>,
}

impl std::fmt::Debug for BasicAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BasicAuth")
            .field("user", &self.user)
            .field("password", &self.password)
            .finish()
    }
}

impl BasicAuth {
    /// Credentials `user:password`.
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] for an empty password or a user containing `:`.
    pub fn new(user: &str, password: &str) -> Result<Self, LensError> {
        if password.is_empty() || user.contains(':') {
            return Err(LensError::InvalidConfig(
                "basic auth needs a user without ':' and a password".into(),
            ));
        }
        Ok(Self {
            user: user.to_owned(),
            password: Secret::new(password.to_owned()),
        })
    }

    fn matches(&self, header_value: &str) -> bool {
        let Some(encoded) = header_value
            .strip_prefix("Basic ")
            .or_else(|| header_value.strip_prefix("basic "))
        else {
            return false;
        };
        let Ok(decoded) = STANDARD.decode(encoded.trim()) else {
            return false;
        };
        let expected = format!("{}:{}", self.user, self.password.expose());
        let a = ring::digest::digest(&ring::digest::SHA256, expected.as_bytes());
        let b = ring::digest::digest(&ring::digest::SHA256, &decoded);
        ct_eq(a.as_ref(), b.as_ref())
    }
}

/// Built-in user-agent block lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AgentPreset {
    /// AI training and answer crawlers (GPTBot, ClaudeBot, CCBot, PerplexityBot…).
    AiCrawlers,
    /// Search engine crawlers (Googlebot, Bingbot…).
    SearchEngines,
    /// SEO and marketing crawlers (AhrefsBot, SemrushBot…).
    SeoCrawlers,
}

impl AgentPreset {
    /// Lowercase substrings matched against the `User-Agent`.
    pub fn patterns(self) -> &'static [&'static str] {
        match self {
            Self::AiCrawlers => &[
                "gptbot",
                "chatgpt-user",
                "oai-searchbot",
                "claudebot",
                "claude-web",
                "claude-searchbot",
                "claude-user",
                "anthropic-ai",
                "ccbot",
                "google-extended",
                "perplexitybot",
                "perplexity-user",
                "bytespider",
                "amazonbot",
                "applebot-extended",
                "meta-externalagent",
                "meta-externalfetcher",
                "facebookbot",
                "cohere-ai",
                "diffbot",
                "imagesiftbot",
                "omgilibot",
                "youbot",
                "ai2bot",
                "timpibot",
                "mistralai-user",
            ],
            Self::SearchEngines => &[
                "googlebot",
                "bingbot",
                "duckduckbot",
                "yandexbot",
                "baiduspider",
                "slurp",
                "applebot",
                "petalbot",
            ],
            Self::SeoCrawlers => &[
                "ahrefsbot",
                "semrushbot",
                "mj12bot",
                "dotbot",
                "blexbot",
                "rogerbot",
                "dataforseobot",
                "serpstatbot",
            ],
        }
    }
}

/// Protection settings for a tap. The default protects nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gates {
    /// Sign in with a password page.
    pub password: Option<PasswordGate>,
    /// Sign in with a secret link.
    pub secret_link: Option<SecretLink>,
    /// Accept HTTP basic credentials (also the only sign-in when alone: browsers show
    /// their own prompt).
    pub basic: Option<BasicAuth>,
    /// Only these networks may connect (empty: everyone).
    pub ip_allow: Vec<IpNet>,
    /// These networks may not connect.
    pub ip_deny: Vec<IpNet>,
    /// Blocked user-agent lists.
    pub agent_presets: Vec<AgentPreset>,
    /// Extra blocked user-agent substrings (case-insensitive).
    pub agent_patterns: Vec<String>,
    /// Paths that skip sign-in (IP and user-agent rules still apply), e.g. `/webhooks/*`.
    pub bypass: Vec<PathPattern>,
    /// How long a sign-in lasts.
    pub session_ttl: Duration,
    /// Mark the session cookie `Secure` (visitors come over HTTPS through Cloudflare).
    pub secure_cookie: bool,
}

impl Default for Gates {
    fn default() -> Self {
        Self {
            password: None,
            secret_link: None,
            basic: None,
            ip_allow: Vec::new(),
            ip_deny: Vec::new(),
            agent_presets: Vec::new(),
            agent_patterns: Vec::new(),
            bypass: Vec::new(),
            session_ttl: Duration::from_secs(7 * 24 * 3_600),
            secure_cookie: true,
        }
    }
}

impl Gates {
    /// Whether any sign-in method is configured.
    pub fn requires_sign_in(&self) -> bool {
        self.password.is_some() || self.secret_link.is_some() || self.basic.is_some()
    }

    /// Whether anything at all is enforced.
    pub fn is_active(&self) -> bool {
        self.requires_sign_in()
            || !self.ip_allow.is_empty()
            || !self.ip_deny.is_empty()
            || !self.agent_presets.is_empty()
            || !self.agent_patterns.is_empty()
    }

    /// A digest of the current credentials: sessions signed under other credentials
    /// stop working. Never leaves the process.
    pub(crate) fn fingerprint(&self) -> [u8; 32] {
        let mut ctx = ring::digest::Context::new(&ring::digest::SHA256);
        if let Some(password) = &self.password {
            ctx.update(b"p");
            ctx.update(password.phc.expose().as_bytes());
        }
        if let Some(link) = &self.secret_link {
            ctx.update(b"l");
            ctx.update(link.token.expose().as_bytes());
        }
        if let Some(basic) = &self.basic {
            ctx.update(b"b");
            ctx.update(basic.user.as_bytes());
            ctx.update(b":");
            ctx.update(basic.password.expose().as_bytes());
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(ctx.finish().as_ref());
        out
    }

    /// IP rules: `Some(outcome)` when the address is refused.
    pub(crate) fn check_ip(&self, ip: IpAddr) -> Option<crate::GateOutcome> {
        let ip = canonical_ip(ip);
        if self.ip_deny.iter().any(|net| net.contains(&ip)) {
            return Some(crate::GateOutcome::IpDenied);
        }
        if !self.ip_allow.is_empty() && !self.ip_allow.iter().any(|net| net.contains(&ip)) {
            return Some(crate::GateOutcome::IpNotAllowed);
        }
        None
    }

    /// User-agent rules: whether this agent is blocked.
    pub(crate) fn agent_blocked(&self, user_agent: &str) -> bool {
        if self.agent_presets.is_empty() && self.agent_patterns.is_empty() {
            return false;
        }
        let ua = user_agent.to_ascii_lowercase();
        self.agent_presets
            .iter()
            .flat_map(|preset| preset.patterns().iter().copied())
            .any(|pattern| ua.contains(pattern))
            || self
                .agent_patterns
                .iter()
                .any(|pattern| !pattern.is_empty() && ua.contains(&pattern.to_ascii_lowercase()))
    }

    pub(crate) fn bypassed(&self, path: &str) -> bool {
        self.bypass.iter().any(|pattern| pattern.matches(path))
    }

    pub(crate) fn basic_ok(&self, headers: &HeaderMap) -> bool {
        let Some(basic) = &self.basic else {
            return false;
        };
        headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| basic.matches(value))
    }

    pub(crate) fn link_ok(&self, key: &str) -> bool {
        self.secret_link
            .as_ref()
            .is_some_and(|link| link.matches(key))
    }

    pub(crate) fn cookie_name(&self) -> &'static str {
        if self.secure_cookie {
            COOKIE_SECURE
        } else {
            COOKIE_PLAIN
        }
    }
}

/// IPv4-mapped IPv6 addresses (`::ffff:1.2.3.4`) compare as IPv4.
fn canonical_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// Signs and checks session cookies.
pub(crate) struct Sessions {
    key: ring::hmac::Key,
}

impl std::fmt::Debug for Sessions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sessions([key redacted])")
    }
}

impl Sessions {
    pub(crate) fn new() -> Result<Self, LensError> {
        let key = crate::util::random_bytes::<32>()?;
        Ok(Self {
            key: ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &key),
        })
    }

    fn message(tap: &str, expiry: u64, fingerprint: &[u8; 32]) -> Vec<u8> {
        let mut message = Vec::with_capacity(tap.len() + 48);
        message.extend_from_slice(b"teitunnel-session-v1|");
        message.extend_from_slice(tap.as_bytes());
        message.push(b'|');
        message.extend_from_slice(expiry.to_string().as_bytes());
        message.push(b'|');
        message.extend_from_slice(fingerprint);
        message
    }

    /// A cookie value valid until `expiry` (Unix seconds).
    pub(crate) fn issue(&self, tap: &str, expiry: u64, fingerprint: &[u8; 32]) -> String {
        let tag = ring::hmac::sign(&self.key, &Self::message(tap, expiry, fingerprint));
        format!("v1.{expiry}.{}", URL_SAFE_NO_PAD.encode(tag.as_ref()))
    }

    /// Whether `value` is a valid, unexpired session for this tap and credentials.
    pub(crate) fn check(&self, value: &str, tap: &str, fingerprint: &[u8; 32], now: u64) -> bool {
        let mut parts = value.splitn(3, '.');
        let (Some("v1"), Some(expiry), Some(tag)) = (parts.next(), parts.next(), parts.next())
        else {
            return false;
        };
        let Ok(expiry) = expiry.parse::<u64>() else {
            return false;
        };
        if expiry <= now {
            return false;
        }
        let Ok(tag) = URL_SAFE_NO_PAD.decode(tag) else {
            return false;
        };
        // `hmac::verify` compares in constant time.
        ring::hmac::verify(&self.key, &Self::message(tap, expiry, fingerprint), &tag).is_ok()
    }

    /// The `Set-Cookie` header for a new session.
    pub(crate) fn set_cookie(
        &self,
        gates: &Gates,
        tap: &str,
        fingerprint: &[u8; 32],
        now: u64,
    ) -> Option<HeaderValue> {
        let ttl = gates.session_ttl.as_secs().max(60);
        let value = self.issue(tap, now + ttl, fingerprint);
        let secure = if gates.secure_cookie { "; Secure" } else { "" };
        HeaderValue::from_str(&format!(
            "{}={value}; Path=/; Max-Age={ttl}; HttpOnly; SameSite=Lax{secure}",
            gates.cookie_name()
        ))
        .ok()
    }
}

/// Finds a cookie's value in `Cookie` headers.
pub(crate) fn cookie_value<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

/// Removes one cookie from the `Cookie` headers (so Lens's session never reaches the
/// origin).
pub(crate) fn strip_cookie(headers: &mut HeaderMap, name: &str) {
    let values: Vec<String> = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .map(str::trim)
                .filter(|pair| pair.split_once('=').is_none_or(|(key, _)| key != name))
                .collect::<Vec<_>>()
                .join("; ")
        })
        .filter(|value| !value.is_empty())
        .collect();
    headers.remove(header::COOKIE);
    for value in values {
        if let Ok(value) = HeaderValue::from_str(&value) {
            headers.append(header::COOKIE, value);
        }
    }
}

/// Per-IP failed sign-in counter (bounded).
#[derive(Debug, Default)]
pub(crate) struct LoginLimiter {
    failures: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl LoginLimiter {
    /// Whether this IP has used up its attempts.
    pub(crate) fn limited(&self, ip: IpAddr) -> bool {
        let failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        failures.get(&ip).is_some_and(|(count, since)| {
            *count >= MAX_FAILURES && since.elapsed() < FAILURE_WINDOW
        })
    }

    pub(crate) fn failed(&self, ip: IpAddr) {
        let mut failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        if failures.len() >= MAX_TRACKED && !failures.contains_key(&ip) {
            failures.retain(|_, (_, since)| since.elapsed() < FAILURE_WINDOW);
            if failures.len() >= MAX_TRACKED {
                return;
            }
        }
        let entry = failures.entry(ip).or_insert((0, Instant::now()));
        if entry.1.elapsed() >= FAILURE_WINDOW {
            *entry = (0, Instant::now());
        }
        entry.0 += 1;
    }

    pub(crate) fn succeeded(&self, ip: IpAddr) {
        self.failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&ip);
    }

    /// Seconds until the window ends (for `Retry-After`).
    pub(crate) fn retry_after(&self, ip: IpAddr) -> u64 {
        let failures = self.failures.lock().unwrap_or_else(PoisonError::into_inner);
        failures.get(&ip).map_or(0, |(_, since)| {
            FAILURE_WINDOW
                .saturating_sub(since.elapsed())
                .as_secs()
                .max(1)
        })
    }
}

/// A local redirect target: must be a path on this site (no `//host`, no scheme).
pub(crate) fn safe_next(next: &str) -> String {
    let ok = next.starts_with('/')
        && !next.starts_with("//")
        && !next.starts_with("/\\")
        && !next.contains(['\r', '\n'])
        && !next.starts_with(LOGIN_PATH);
    if ok { next.to_owned() } else { "/".to_owned() }
}

/// Parses an `application/x-www-form-urlencoded` body.
pub(crate) fn parse_form(body: &[u8]) -> HashMap<String, String> {
    body.split(|&b| b == b'&')
        .filter_map(|pair| {
            let mut parts = pair.splitn(2, |&b| b == b'=');
            let key = parts.next()?;
            let value = parts.next().unwrap_or_default();
            let decode = |raw: &[u8]| {
                let plus: Vec<u8> = raw
                    .iter()
                    .map(|&b| if b == b'+' { b' ' } else { b })
                    .collect();
                percent_encoding::percent_decode(&plus)
                    .decode_utf8_lossy()
                    .into_owned()
            };
            Some((decode(key), decode(value)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn password_hash_and_verify() {
        let gate = PasswordGate::new("correct horse").unwrap();
        assert!(gate.verify("correct horse"));
        assert!(!gate.verify("wrong"));
        assert!(gate.hash().expose().starts_with("$argon2id$"));
        let again = PasswordGate::from_hash(gate.hash().expose()).unwrap();
        assert!(again.verify("correct horse"));
        assert!(PasswordGate::from_hash("nope").is_err());
        assert!(PasswordGate::new("").is_err());
        assert!(!format!("{gate:?}").contains("argon2"));
    }

    #[test]
    fn links_and_basic() {
        assert!(SecretLink::new("short").is_err());
        assert!(SecretLink::new("has spaces in it!!").is_err());
        let link = SecretLink::generate().unwrap();
        assert!(link.matches(link.token().expose()));
        assert!(!link.matches("wrong"));
        assert!(!format!("{link:?}").contains(link.token().expose()));

        let basic = BasicAuth::new("ada", "lovelace").unwrap();
        let header = format!("Basic {}", STANDARD.encode("ada:lovelace"));
        assert!(basic.matches(&header));
        assert!(!basic.matches(&format!("Basic {}", STANDARD.encode("ada:x"))));
        assert!(!basic.matches("Bearer x"));
        assert!(!basic.matches("Basic !!!"));
        assert!(BasicAuth::new("a:b", "c").is_err());
        assert!(!format!("{basic:?}").contains("lovelace"));
    }

    #[test]
    fn ip_rules() {
        let gates = Gates {
            ip_allow: vec![
                "10.0.0.0/8".parse().unwrap(),
                "2001:db8::/32".parse().unwrap(),
            ],
            ip_deny: vec!["10.0.0.66/32".parse().unwrap()],
            ..Gates::default()
        };
        assert_eq!(gates.check_ip("10.1.2.3".parse().unwrap()), None);
        assert_eq!(
            gates.check_ip("10.0.0.66".parse().unwrap()),
            Some(crate::GateOutcome::IpDenied)
        );
        assert_eq!(
            gates.check_ip("192.168.1.1".parse().unwrap()),
            Some(crate::GateOutcome::IpNotAllowed)
        );
        assert_eq!(gates.check_ip("::ffff:10.0.0.1".parse().unwrap()), None);
        assert_eq!(gates.check_ip("2001:db8::1".parse().unwrap()), None);
    }

    #[test]
    fn agents() {
        let gates = Gates {
            agent_presets: vec![AgentPreset::AiCrawlers],
            agent_patterns: vec!["EvilBot".into()],
            ..Gates::default()
        };
        assert!(
            gates.agent_blocked("Mozilla/5.0 (compatible; GPTBot/1.2; +https://openai.com/gptbot)")
        );
        assert!(gates.agent_blocked(
            "Mozilla/5.0 AppleWebKit/537.36 (KHTML, like Gecko; compatible; ClaudeBot/1.0)"
        ));
        assert!(gates.agent_blocked("evilbot/2"));
        assert!(!gates.agent_blocked("Mozilla/5.0 (Macintosh) Safari/605.1.15"));
        assert!(!Gates::default().agent_blocked("GPTBot"));
    }

    #[test]
    fn sessions_sign_and_expire() {
        let sessions = Sessions::new().unwrap();
        let fp = [7u8; 32];
        let cookie = sessions.issue("tap-1", 2_000, &fp);
        assert!(sessions.check(&cookie, "tap-1", &fp, 1_000));
        assert!(!sessions.check(&cookie, "tap-1", &fp, 2_000), "expired");
        assert!(!sessions.check(&cookie, "tap-2", &fp, 1_000), "other tap");
        assert!(
            !sessions.check(&cookie, "tap-1", &[8u8; 32], 1_000),
            "credentials changed"
        );
        let forged = cookie.replace("v1.2000", "v1.9999");
        assert!(!sessions.check(&forged, "tap-1", &fp, 1_000));
        let other = Sessions::new().unwrap();
        assert!(!other.check(&cookie, "tap-1", &fp, 1_000), "other key");
        assert!(!format!("{sessions:?}").contains("key:"));
    }

    #[test]
    fn set_cookie_attributes() {
        let sessions = Sessions::new().unwrap();
        let gates = Gates::default();
        let value = sessions.set_cookie(&gates, "t", &[0; 32], 100).unwrap();
        let text = value.to_str().unwrap();
        assert!(text.starts_with("__Host-teitunnel=v1."));
        for attribute in [
            "Path=/",
            "HttpOnly",
            "SameSite=Lax",
            "Secure",
            "Max-Age=604800",
        ] {
            assert!(text.contains(attribute), "{attribute}");
        }
        let plain = Gates {
            secure_cookie: false,
            ..Gates::default()
        };
        let value = sessions.set_cookie(&plain, "t", &[0; 32], 100).unwrap();
        assert!(!value.to_str().unwrap().contains("Secure"));
    }

    #[test]
    fn cookies_are_found_and_stripped() {
        let mut headers = HeaderMap::new();
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=1; __Host-teitunnel=v1.x.y"),
        );
        headers.append(header::COOKIE, HeaderValue::from_static("b=2"));
        assert_eq!(cookie_value(&headers, "__Host-teitunnel"), Some("v1.x.y"));
        assert_eq!(cookie_value(&headers, "b"), Some("2"));
        strip_cookie(&mut headers, "__Host-teitunnel");
        let all: Vec<&str> = headers
            .get_all(header::COOKIE)
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(all, vec!["a=1", "b=2"]);
        let mut only = HeaderMap::new();
        only.insert(
            header::COOKIE,
            HeaderValue::from_static("__Host-teitunnel=v"),
        );
        strip_cookie(&mut only, "__Host-teitunnel");
        assert!(!only.contains_key(header::COOKIE));
    }

    #[test]
    fn limiter_counts_per_ip() {
        let limiter = LoginLimiter::default();
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        for _ in 0..MAX_FAILURES {
            assert!(!limiter.limited(ip));
            limiter.failed(ip);
        }
        assert!(limiter.limited(ip));
        assert!(limiter.retry_after(ip) > 0);
        assert!(!limiter.limited("5.6.7.8".parse().unwrap()));
        limiter.succeeded(ip);
        assert!(!limiter.limited(ip));
    }

    #[test]
    fn next_must_be_local() {
        assert_eq!(safe_next("/dashboard?x=1"), "/dashboard?x=1");
        assert_eq!(safe_next("//evil.com"), "/");
        assert_eq!(safe_next("/\\evil.com"), "/");
        assert_eq!(safe_next("https://evil.com"), "/");
        assert_eq!(safe_next("/a\r\nSet-Cookie: x"), "/");
        assert_eq!(safe_next(LOGIN_PATH), "/");
    }

    #[test]
    fn forms() {
        let form = parse_form(b"password=a+b%26c&next=%2Fx%3Fy%3D1&flag");
        assert_eq!(form["password"], "a b&c");
        assert_eq!(form["next"], "/x?y=1");
        assert_eq!(form["flag"], "");
    }

    proptest! {
        #[test]
        fn cookie_checking_never_panics(value in any::<String>()) {
            let sessions = Sessions::new().unwrap();
            prop_assert!(!sessions.check(&value, "t", &[0; 32], 0));
        }

        #[test]
        fn form_parsing_never_panics(body in proptest::collection::vec(any::<u8>(), 0..512)) {
            let _ = parse_form(&body);
        }
    }
}
