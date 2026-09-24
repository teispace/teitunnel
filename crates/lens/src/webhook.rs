//! Webhook signature verification and re-signing.
//!
//! Recognises the provider from headers, verifies with a secret the user supplies (kept
//! in the keychain by the embedder), and reports `Valid`, `Invalid`, `Expired` (a
//! correct signature with a timestamp outside the tolerance), `UnknownProvider` or
//! `NotEnoughData` (the captured body is truncated). Signature comparisons are
//! constant-time. [`resign`] recomputes signatures for replays where the provider's
//! scheme allows it.

use std::{fmt, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, HeaderValue};
use ring::hmac;
use serde::{Deserialize, Serialize};

use crate::{
    Exchange, Secret,
    capture::decode_body,
    util::{ct_eq, hex_decode, hex_encode},
};

/// Webhook senders Lens understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Provider {
    /// `Stripe-Signature: t=…,v1=…` (HMAC-SHA256 of `t.body`).
    Stripe,
    /// `X-Hub-Signature-256: sha256=…` (HMAC-SHA256 of the body).
    GitHub,
    /// `X-Slack-Signature: v0=…` with `X-Slack-Request-Timestamp`.
    Slack,
    /// `X-Shopify-Hmac-Sha256` (base64 HMAC-SHA256 of the body).
    Shopify,
    /// Standard Webhooks / Svix (Clerk, Resend, …): `webhook-id`, `webhook-timestamp`,
    /// `webhook-signature: v1,…` (or the `svix-` names).
    StandardWebhooks,
    /// `X-Twilio-Signature` (base64 HMAC-SHA1 of the URL and sorted form parameters).
    Twilio,
    /// `Linear-Signature` (hex HMAC-SHA256 of the body; `webhookTimestamp` in the body).
    Linear,
    /// `X-Signature-Ed25519` with `X-Signature-Timestamp` (Ed25519, public key).
    Discord,
}

impl Provider {
    /// All providers.
    pub const ALL: [Self; 8] = [
        Self::Stripe,
        Self::GitHub,
        Self::Slack,
        Self::Shopify,
        Self::StandardWebhooks,
        Self::Twilio,
        Self::Linear,
        Self::Discord,
    ];

    /// The timestamp tolerance the provider's documentation recommends.
    pub fn default_tolerance(self) -> Option<Duration> {
        match self {
            Self::Stripe | Self::Slack | Self::StandardWebhooks => Some(Duration::from_secs(300)),
            Self::Linear => Some(Duration::from_secs(60)),
            Self::GitHub | Self::Shopify | Self::Twilio | Self::Discord => None,
        }
    }

    /// Whether [`resign`] supports this provider.
    pub fn can_resign(self) -> bool {
        !matches!(self, Self::Twilio | Self::Discord)
    }
}

/// Recognises the provider from request headers.
pub fn detect(headers: &HeaderMap) -> Option<Provider> {
    let has = |name: &str| headers.contains_key(name);
    if has("stripe-signature") {
        Some(Provider::Stripe)
    } else if has("x-hub-signature-256") || has("x-hub-signature") {
        Some(Provider::GitHub)
    } else if has("x-slack-signature") {
        Some(Provider::Slack)
    } else if has("x-shopify-hmac-sha256") {
        Some(Provider::Shopify)
    } else if has("webhook-signature") || has("svix-signature") {
        Some(Provider::StandardWebhooks)
    } else if has("x-twilio-signature") {
        Some(Provider::Twilio)
    } else if has("linear-signature") {
        Some(Provider::Linear)
    } else if has("x-signature-ed25519") {
        Some(Provider::Discord)
    } else {
        None
    }
}

/// The secret to verify with: the signing secret, or Discord's application public key
/// (hex).
#[derive(Clone, PartialEq, Eq)]
pub struct WebhookSecret(Secret<String>);

impl WebhookSecret {
    /// Wraps a signing secret or public key as the provider's dashboard shows it
    /// (`whsec_…` for Stripe and Standard Webhooks, hex for Discord).
    pub fn new(value: impl Into<String>) -> Self {
        Self(Secret::new(value.into()))
    }

    fn raw(&self) -> &[u8] {
        self.0.expose().trim().as_bytes()
    }

    /// Standard Webhooks keys are base64 after an optional `whsec_` prefix.
    fn standard_key(&self) -> Option<Vec<u8>> {
        let text = self.0.expose().trim();
        STANDARD
            .decode(text.strip_prefix("whsec_").unwrap_or(text))
            .ok()
    }
}

impl fmt::Debug for WebhookSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("WebhookSecret([redacted])")
    }
}

/// The outcome of a verification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "result")]
pub enum Verification {
    /// The signature matches and the timestamp (if any) is recent.
    Valid,
    /// The signature doesn't match, or required headers are missing or malformed.
    Invalid {
        /// Why (English, technical).
        reason: String,
    },
    /// The signature matches but the timestamp is outside the tolerance (e.g. an old
    /// capture, or a replay without re-signing).
    Expired {
        /// The signed timestamp (Unix seconds).
        timestamp: u64,
        /// Its age in seconds (negative when in the future).
        age_secs: i64,
    },
    /// No known signature headers.
    UnknownProvider,
    /// The body wasn't captured in full, so it can't be checked.
    NotEnoughData {
        /// Why.
        reason: String,
    },
}

/// What a verification looks at.
#[derive(Debug, Clone, Copy)]
pub struct VerifyInput<'a> {
    /// Request headers.
    pub headers: &'a HeaderMap,
    /// The body exactly as received (decoded if the request used `Content-Encoding`).
    pub body: &'a [u8],
    /// Whether `body` is the whole body.
    pub body_complete: bool,
    /// The public URL the sender called (Twilio signs it), e.g.
    /// `https://app.example.com/sms?x=1`.
    pub url: &'a str,
    /// Current time (Unix seconds).
    pub now: u64,
    /// Timestamp tolerance; `None` uses the provider's default.
    pub tolerance: Option<Duration>,
}

fn invalid(reason: impl Into<String>) -> Verification {
    Verification::Invalid {
        reason: reason.into(),
    }
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
}

fn hmac_sha256(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA256, key);
    let mut ctx = hmac::Context::with_key(&key);
    for part in parts {
        ctx.update(part);
    }
    ctx.sign().as_ref().to_vec()
}

fn hmac_sha1(key: &[u8], parts: &[&[u8]]) -> Vec<u8> {
    let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
    let mut ctx = hmac::Context::with_key(&key);
    for part in parts {
        ctx.update(part);
    }
    ctx.sign().as_ref().to_vec()
}

/// Valid, or Expired when the timestamp is outside the tolerance.
fn check_time(timestamp: u64, now: u64, tolerance: Option<Duration>) -> Verification {
    let Some(tolerance) = tolerance else {
        return Verification::Valid;
    };
    let age = i64::try_from(now).unwrap_or(i64::MAX) - i64::try_from(timestamp).unwrap_or(i64::MAX);
    if age.unsigned_abs() > tolerance.as_secs() {
        Verification::Expired {
            timestamp,
            age_secs: age,
        }
    } else {
        Verification::Valid
    }
}

/// Verifies a request against `provider`'s scheme.
pub fn verify(provider: Provider, input: &VerifyInput<'_>, secret: &WebhookSecret) -> Verification {
    if !input.body_complete {
        return Verification::NotEnoughData {
            reason: "the captured body is truncated; raise the capture limit to verify".into(),
        };
    }
    let tolerance = input.tolerance.or_else(|| provider.default_tolerance());
    let headers = input.headers;
    let body = input.body;
    match provider {
        Provider::Stripe => {
            let Some(value) = header(headers, "stripe-signature") else {
                return invalid("the Stripe-Signature header is missing");
            };
            let mut timestamp = None;
            let mut candidates = Vec::new();
            for part in value.split(',') {
                match part.trim().split_once('=') {
                    Some(("t", t)) => timestamp = t.parse::<u64>().ok(),
                    Some(("v1", sig)) => candidates.extend(hex_decode(sig)),
                    _ => {}
                }
            }
            let Some(timestamp) = timestamp else {
                return invalid("Stripe-Signature has no timestamp");
            };
            let expected = hmac_sha256(
                secret.raw(),
                &[timestamp.to_string().as_bytes(), b".", body],
            );
            if candidates.iter().any(|c| ct_eq(c, &expected)) {
                check_time(timestamp, input.now, tolerance)
            } else {
                invalid("no v1 signature matches")
            }
        }
        Provider::GitHub => {
            if let Some(value) = header(headers, "x-hub-signature-256") {
                let Some(sig) = value.strip_prefix("sha256=").and_then(hex_decode) else {
                    return invalid("X-Hub-Signature-256 isn't sha256=<hex>");
                };
                let expected = hmac_sha256(secret.raw(), &[body]);
                return if ct_eq(&sig, &expected) {
                    Verification::Valid
                } else {
                    invalid("the signature doesn't match")
                };
            }
            let Some(sig) = header(headers, "x-hub-signature")
                .and_then(|v| v.strip_prefix("sha1="))
                .and_then(hex_decode)
            else {
                return invalid("X-Hub-Signature-256 is missing");
            };
            if ct_eq(&sig, &hmac_sha1(secret.raw(), &[body])) {
                Verification::Valid
            } else {
                invalid("the signature doesn't match")
            }
        }
        Provider::Slack => {
            let (Some(ts), Some(sig)) = (
                header(headers, "x-slack-request-timestamp"),
                header(headers, "x-slack-signature"),
            ) else {
                return invalid("X-Slack-Request-Timestamp or X-Slack-Signature is missing");
            };
            let Ok(timestamp) = ts.parse::<u64>() else {
                return invalid("X-Slack-Request-Timestamp isn't a number");
            };
            let Some(sig) = sig.strip_prefix("v0=").and_then(hex_decode) else {
                return invalid("X-Slack-Signature isn't v0=<hex>");
            };
            let expected = hmac_sha256(secret.raw(), &[b"v0:", ts.as_bytes(), b":", body]);
            if ct_eq(&sig, &expected) {
                check_time(timestamp, input.now, tolerance)
            } else {
                invalid("the signature doesn't match")
            }
        }
        Provider::Shopify => {
            let Some(sig) =
                header(headers, "x-shopify-hmac-sha256").and_then(|v| STANDARD.decode(v).ok())
            else {
                return invalid("X-Shopify-Hmac-Sha256 is missing or not base64");
            };
            if ct_eq(&sig, &hmac_sha256(secret.raw(), &[body])) {
                Verification::Valid
            } else {
                invalid("the signature doesn't match")
            }
        }
        Provider::StandardWebhooks => verify_standard(input, secret, tolerance),
        Provider::Twilio => verify_twilio(input, secret),
        Provider::Linear => {
            let Some(sig) = header(headers, "linear-signature").and_then(hex_decode) else {
                return invalid("Linear-Signature is missing or not hex");
            };
            if !ct_eq(&sig, &hmac_sha256(secret.raw(), &[body])) {
                return invalid("the signature doesn't match");
            }
            let timestamp = serde_json::from_slice::<serde_json::Value>(body)
                .ok()
                .and_then(|json| {
                    json.get("webhookTimestamp")
                        .and_then(serde_json::Value::as_u64)
                });
            match timestamp {
                Some(ms) => check_time(ms / 1_000, input.now, tolerance),
                None => Verification::Valid,
            }
        }
        Provider::Discord => {
            let (Some(sig), Some(ts)) = (
                header(headers, "x-signature-ed25519").and_then(hex_decode),
                header(headers, "x-signature-timestamp"),
            ) else {
                return invalid("X-Signature-Ed25519 or X-Signature-Timestamp is missing");
            };
            let Some(public_key) = hex_decode(secret.0.expose().trim()) else {
                return invalid("the Discord public key isn't hex");
            };
            let mut message = ts.as_bytes().to_vec();
            message.extend_from_slice(body);
            let key =
                ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, public_key);
            if key.verify(&message, &sig).is_ok() {
                Verification::Valid
            } else {
                invalid("the signature doesn't match")
            }
        }
    }
}

/// Header names for Standard Webhooks: `webhook-*`, or Svix's `svix-*`.
fn standard_names(headers: &HeaderMap) -> [&'static str; 3] {
    if headers.contains_key("webhook-signature") || !headers.contains_key("svix-signature") {
        ["webhook-id", "webhook-timestamp", "webhook-signature"]
    } else {
        ["svix-id", "svix-timestamp", "svix-signature"]
    }
}

fn verify_standard(
    input: &VerifyInput<'_>,
    secret: &WebhookSecret,
    tolerance: Option<Duration>,
) -> Verification {
    let [id_name, ts_name, sig_name] = standard_names(input.headers);
    let (Some(id), Some(ts), Some(signatures)) = (
        header(input.headers, id_name),
        header(input.headers, ts_name),
        header(input.headers, sig_name),
    ) else {
        return invalid(format!("{id_name}, {ts_name} or {sig_name} is missing"));
    };
    let Ok(timestamp) = ts.parse::<u64>() else {
        return invalid(format!("{ts_name} isn't a number"));
    };
    let Some(key) = secret.standard_key() else {
        return invalid("the secret isn't whsec_<base64>");
    };
    let expected = hmac_sha256(
        &key,
        &[id.as_bytes(), b".", ts.as_bytes(), b".", input.body],
    );
    let matched = signatures
        .split(' ')
        .filter_map(|entry| entry.split_once(','))
        .filter(|(version, _)| *version == "v1")
        .filter_map(|(_, sig)| STANDARD.decode(sig).ok())
        .any(|sig| ct_eq(&sig, &expected));
    if matched {
        check_time(timestamp, input.now, tolerance)
    } else {
        invalid("no v1 signature matches")
    }
}

fn verify_twilio(input: &VerifyInput<'_>, secret: &WebhookSecret) -> Verification {
    let Some(sig) =
        header(input.headers, "x-twilio-signature").and_then(|v| STANDARD.decode(v).ok())
    else {
        return invalid("X-Twilio-Signature is missing or not base64");
    };
    let form = input
        .headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    let mut signed = input.url.as_bytes().to_vec();
    if form {
        let mut params: Vec<(String, String)> =
            crate::gate::parse_form(input.body).into_iter().collect();
        // parse_form keeps the last value of repeated keys; Twilio doesn't repeat keys
        // in webhooks, and sorting by key then value is its documented order.
        params.sort();
        for (key, value) in params {
            signed.extend_from_slice(key.as_bytes());
            signed.extend_from_slice(value.as_bytes());
        }
    } else if let Some(query) = input.url.split_once('?').map(|(_, q)| q) {
        // JSON bodies: the URL carries `bodySHA256`, which must match the body.
        let declared = query
            .split('&')
            .find_map(|pair| pair.strip_prefix("bodySHA256="));
        if let Some(declared) = declared {
            let actual = ring::digest::digest(&ring::digest::SHA256, input.body);
            if !declared.eq_ignore_ascii_case(&hex_encode(actual.as_ref())) {
                return invalid("bodySHA256 doesn't match the body");
            }
        }
    }
    if ct_eq(&sig, &hmac_sha1(secret.raw(), &[&signed])) {
        Verification::Valid
    } else {
        invalid("the signature doesn't match (check the URL Twilio called)")
    }
}

/// Verifies a captured exchange, detecting the provider unless given.
pub fn verify_exchange(
    exchange: &Exchange,
    secret: &WebhookSecret,
    provider: Option<Provider>,
    now: u64,
) -> Verification {
    let request = &exchange.request;
    let Some(provider) = provider.or_else(|| detect(&request.headers)) else {
        return Verification::UnknownProvider;
    };
    let body = match decode_body(&request.headers, &request.body.data) {
        Ok(body) => body,
        Err(err) => {
            return Verification::NotEnoughData {
                reason: err.to_string(),
            };
        }
    };
    let url = request.url();
    verify(
        provider,
        &VerifyInput {
            headers: &request.headers,
            body: &body,
            body_complete: request.body.complete && !request.body.truncated,
            url: &url,
            now,
            tolerance: None,
        },
        secret,
    )
}

/// Why re-signing failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WebhookError {
    /// The provider's scheme can't be recomputed here (Twilio signs the public URL;
    /// Discord needs Discord's private key).
    #[error("{0:?} signatures can't be recomputed")]
    Unsupported(Provider),
    /// The secret has the wrong format.
    #[error("the webhook secret isn't in the expected format")]
    InvalidSecret,
}

fn set(headers: &mut HeaderMap, name: &'static str, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

/// Re-signs `body` for `provider`, replacing the signature (and timestamp) headers,
/// so a replayed or edited webhook verifies again.
///
/// Linear also checks `webhookTimestamp` inside the body, which this doesn't change.
///
/// # Errors
/// [`WebhookError::Unsupported`] for Twilio and Discord; [`WebhookError::InvalidSecret`]
/// for a Standard Webhooks secret that isn't base64.
pub fn resign(
    provider: Provider,
    headers: &mut HeaderMap,
    body: &[u8],
    secret: &WebhookSecret,
    now: u64,
) -> Result<(), WebhookError> {
    let ts = now.to_string();
    match provider {
        Provider::Stripe => {
            let sig = hmac_sha256(secret.raw(), &[ts.as_bytes(), b".", body]);
            set(
                headers,
                "stripe-signature",
                &format!("t={ts},v1={}", hex_encode(&sig)),
            );
        }
        Provider::GitHub => {
            let sig = hmac_sha256(secret.raw(), &[body]);
            set(
                headers,
                "x-hub-signature-256",
                &format!("sha256={}", hex_encode(&sig)),
            );
            if headers.contains_key("x-hub-signature") {
                let sig = hmac_sha1(secret.raw(), &[body]);
                set(
                    headers,
                    "x-hub-signature",
                    &format!("sha1={}", hex_encode(&sig)),
                );
            }
        }
        Provider::Slack => {
            let sig = hmac_sha256(secret.raw(), &[b"v0:", ts.as_bytes(), b":", body]);
            set(headers, "x-slack-request-timestamp", &ts);
            set(
                headers,
                "x-slack-signature",
                &format!("v0={}", hex_encode(&sig)),
            );
        }
        Provider::Shopify => {
            let sig = hmac_sha256(secret.raw(), &[body]);
            set(headers, "x-shopify-hmac-sha256", &STANDARD.encode(sig));
        }
        Provider::StandardWebhooks => {
            let key = secret.standard_key().ok_or(WebhookError::InvalidSecret)?;
            let [id_name, ts_name, sig_name] = standard_names(headers);
            let id = header(headers, id_name)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("msg_{}", uuid::Uuid::new_v4().simple()));
            let sig = hmac_sha256(&key, &[id.as_bytes(), b".", ts.as_bytes(), b".", body]);
            set(headers, id_name, &id);
            set(headers, ts_name, &ts);
            set(headers, sig_name, &format!("v1,{}", STANDARD.encode(sig)));
        }
        Provider::Linear => {
            let sig = hmac_sha256(secret.raw(), &[body]);
            set(headers, "linear-signature", &hex_encode(&sig));
        }
        Provider::Twilio | Provider::Discord => return Err(WebhookError::Unsupported(provider)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    fn input<'a>(headers: &'a HeaderMap, body: &'a [u8], now: u64) -> VerifyInput<'a> {
        VerifyInput {
            headers,
            body,
            body_complete: true,
            url: "",
            now,
            tolerance: None,
        }
    }

    // GitHub's documented example: secret "It's a Secret to Everybody", payload
    // "Hello, World!" (docs.github.com, "Validating webhook deliveries", Testing).
    #[test]
    fn github_official_vector() {
        let secret = WebhookSecret::new("It's a Secret to Everybody");
        let h = headers(&[(
            "x-hub-signature-256",
            "sha256=757107ea0eb2509fc211221cce984b8a37570b6d7586c22c46f4379c8b043e17",
        )]);
        assert_eq!(detect(&h), Some(Provider::GitHub));
        assert_eq!(
            verify(Provider::GitHub, &input(&h, b"Hello, World!", 0), &secret),
            Verification::Valid
        );
        assert!(matches!(
            verify(Provider::GitHub, &input(&h, b"Hello, World?", 0), &secret),
            Verification::Invalid { .. }
        ));
    }

    // Slack's documented example (api.slack.com/authentication/verifying-requests-from-slack).
    #[test]
    fn slack_official_vector() {
        let secret = WebhookSecret::new("8f742231b10e8888abcd99yyyzzz85a5");
        let body = b"token=xyzz0WbapA4vBCDEFasx0q6G&team_id=T1DC2JH3J&team_domain=testteamnow&channel_id=G8PSS9T3V&channel_name=foobar&user_id=U2CERLKJA&user_name=roadrunner&command=%2Fwebhook-collect&text=&response_url=https%3A%2F%2Fhooks.slack.com%2Fcommands%2FT1DC2JH3J%2F397700885554%2F96rGlfmibIGlgcZRskXaIFfN&trigger_id=398738663015.47445629121.803a0bc887a14d10d2c447fce8b6703c";
        let h = headers(&[
            ("x-slack-request-timestamp", "1531420618"),
            (
                "x-slack-signature",
                "v0=a2114d57b48eac39b9ad189dd8316235a7b4a8d21a10bd27519666489c69b503",
            ),
        ]);
        assert_eq!(detect(&h), Some(Provider::Slack));
        assert_eq!(
            verify(Provider::Slack, &input(&h, body, 1_531_420_618), &secret),
            Verification::Valid
        );
        assert_eq!(
            verify(
                Provider::Slack,
                &input(&h, body, 1_531_420_618 + 301),
                &secret
            ),
            Verification::Expired {
                timestamp: 1_531_420_618,
                age_secs: 301
            }
        );
    }

    // Svix / Standard Webhooks documented example (docs.svix.com, "Verifying manually").
    #[test]
    fn standard_webhooks_official_vector() {
        let secret = WebhookSecret::new("whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw");
        let body = br#"{"test": 2432232314}"#;
        for prefix in ["webhook", "svix"] {
            let h = headers(&[
                (&format!("{prefix}-id"), "msg_p5jXN8AQM9LWM0D4loKWxJek"),
                (&format!("{prefix}-timestamp"), "1614265330"),
                (
                    &format!("{prefix}-signature"),
                    "v1,g0hM9SsE+OTPJTGt/tmIKtSyZlE3uFJELVlNIOLJ1OE= v1,bm9ldHU=",
                ),
            ]);
            assert_eq!(detect(&h), Some(Provider::StandardWebhooks));
            assert_eq!(
                verify(
                    Provider::StandardWebhooks,
                    &input(&h, body, 1_614_265_330),
                    &secret
                ),
                Verification::Valid
            );
            assert!(matches!(
                verify(
                    Provider::StandardWebhooks,
                    &input(&h, body, 1_614_265_330 + 3_600),
                    &secret
                ),
                Verification::Expired { .. }
            ));
        }
    }

    // Twilio's documented example (twilio.com/docs/usage/security, "Validating
    // signatures"): auth token 12345, URL and form parameters below.
    #[test]
    fn twilio_official_vector() {
        let secret = WebhookSecret::new("12345");
        let h = headers(&[
            ("x-twilio-signature", "0/KCTR6DLpKmkAf8muzZqo1nDgQ="),
            ("content-type", "application/x-www-form-urlencoded"),
        ]);
        let body = b"CallSid=CA1234567890ABCDE&Caller=%2B12349013030&Digits=1234&From=%2B12349013030&To=%2B18005551212";
        let mut verify_input = input(&h, body, 0);
        verify_input.url = "https://mycompany.com/myapp.php?foo=1&bar=2";
        assert_eq!(detect(&h), Some(Provider::Twilio));
        assert_eq!(
            verify(Provider::Twilio, &verify_input, &secret),
            Verification::Valid
        );
        verify_input.url = "https://mycompany.com/other";
        assert!(matches!(
            verify(Provider::Twilio, &verify_input, &secret),
            Verification::Invalid { .. }
        ));
    }

    // Twilio JSON bodies: the URL carries bodySHA256 (value computed with Python's
    // hashlib/hmac, following the same page).
    #[test]
    fn twilio_json_body() {
        let secret = WebhookSecret::new("12345");
        let h = headers(&[
            ("x-twilio-signature", "pampnf0XBgYaQgiAzHhOwoP7gcI="),
            ("content-type", "application/json"),
        ]);
        let body = br#"{"hello":"world"}"#;
        let mut verify_input = input(&h, body, 0);
        verify_input.url = "https://example.com/sms?bodySHA256=93a23971a914e5eacbf0a8d25154cda309c3c1c72fbb9914d47c60f3cb681588";
        assert_eq!(
            verify(Provider::Twilio, &verify_input, &secret),
            Verification::Valid
        );
        let tampered = br#"{"hello":"there"}"#;
        verify_input.body = tampered;
        assert!(matches!(
            verify(Provider::Twilio, &verify_input, &secret),
            Verification::Invalid { .. }
        ));
    }

    // Stripe's scheme (stripe.com/docs/webhooks#verify-manually); the expected value was
    // computed independently with Python's hmac module.
    #[test]
    fn stripe_vector_and_tolerance() {
        let secret = WebhookSecret::new("whsec_test_secret");
        let body = br#"{"id":"evt_test_webhook","object":"event"}"#;
        let h = headers(&[(
            "stripe-signature",
            "t=1492774577,v1=88a022085c6bdb887b02cb26ff76dd681234d9675c0f22844059f55552a8883a,v0=6ffbb59b2300aae63f272406069a9788598b792a944a07aba816edb039989a39",
        )]);
        assert_eq!(detect(&h), Some(Provider::Stripe));
        assert_eq!(
            verify(
                Provider::Stripe,
                &input(&h, body, 1_492_774_577 + 10),
                &secret
            ),
            Verification::Valid
        );
        assert!(matches!(
            verify(
                Provider::Stripe,
                &input(&h, body, 1_492_774_577 + 400),
                &secret
            ),
            Verification::Expired { age_secs: 400, .. }
        ));
        let wrong = WebhookSecret::new("whsec_other");
        assert!(matches!(
            verify(Provider::Stripe, &input(&h, body, 1_492_774_577), &wrong),
            Verification::Invalid { .. }
        ));
    }

    // Shopify and Linear: expected values computed with Python's hmac module per each
    // provider's docs (shopify.dev "Verify webhooks"; linear.app/developers/webhooks).
    #[test]
    fn shopify_and_linear_vectors() {
        let h = headers(&[(
            "x-shopify-hmac-sha256",
            "VnKUjZsLuN5iZWjn5EntcBVCF9kMN43LglzCE1/GSeY=",
        )]);
        assert_eq!(detect(&h), Some(Provider::Shopify));
        assert_eq!(
            verify(
                Provider::Shopify,
                &input(&h, br#"{"id":1}"#, 0),
                &WebhookSecret::new("hush")
            ),
            Verification::Valid
        );

        let body = br#"{"action":"create","webhookTimestamp":1700000000000}"#;
        let h = headers(&[(
            "linear-signature",
            "0e7eb538ce08f2d147d43dcc1a8d5fef4d5dd6effe602c4c11c3d2456d92c6be",
        )]);
        let secret = WebhookSecret::new("lin_wh_secret");
        assert_eq!(detect(&h), Some(Provider::Linear));
        assert_eq!(
            verify(Provider::Linear, &input(&h, body, 1_700_000_030), &secret),
            Verification::Valid
        );
        assert!(matches!(
            verify(Provider::Linear, &input(&h, body, 1_700_000_120), &secret),
            Verification::Expired { .. }
        ));
    }

    // RFC 8032 §7.1 TEST 2 (message 0x72): Discord signs timestamp + body, so the
    // timestamp "r" (0x72) and an empty body form the RFC's message.
    #[test]
    fn discord_rfc8032_vector() {
        let secret =
            WebhookSecret::new("3d4017c3e843895a92b70aa74d1b7ebc9c982ccf2ec4968cc0cd55f12af4660c");
        let h = headers(&[
            (
                "x-signature-ed25519",
                "92a009a9f0d4cab8720e820b5f642540a2b27b5416503f8fb3762223ebdb69da085ac1e43e15996e458f3613d0f11d8c387b2eaeb4302aeeb00d291612bb0c00",
            ),
            ("x-signature-timestamp", "r"),
        ]);
        assert_eq!(detect(&h), Some(Provider::Discord));
        assert_eq!(
            verify(Provider::Discord, &input(&h, b"", 0), &secret),
            Verification::Valid
        );
        assert!(matches!(
            verify(Provider::Discord, &input(&h, b"x", 0), &secret),
            Verification::Invalid { .. }
        ));
    }

    #[test]
    fn truncated_bodies_and_unknown_providers() {
        let h = headers(&[("x-hub-signature-256", "sha256=00")]);
        let mut partial = input(&h, b"", 0);
        partial.body_complete = false;
        assert!(matches!(
            verify(Provider::GitHub, &partial, &WebhookSecret::new("s")),
            Verification::NotEnoughData { .. }
        ));
        assert_eq!(detect(&HeaderMap::new()), None);
        assert!(matches!(
            verify(
                Provider::Stripe,
                &input(&HeaderMap::new(), b"", 0),
                &WebhookSecret::new("s")
            ),
            Verification::Invalid { .. }
        ));
    }

    #[test]
    fn resign_round_trips() {
        let now = 1_800_000_000;
        let body = br#"{"edited":true}"#;
        let cases: Vec<(Provider, WebhookSecret, HeaderMap)> = vec![
            (
                Provider::Stripe,
                WebhookSecret::new("whsec_abc"),
                headers(&[("stripe-signature", "t=1,v1=00")]),
            ),
            (
                Provider::GitHub,
                WebhookSecret::new("gh"),
                headers(&[
                    ("x-hub-signature-256", "sha256=00"),
                    ("x-hub-signature", "sha1=00"),
                ]),
            ),
            (
                Provider::Slack,
                WebhookSecret::new("sl"),
                headers(&[("x-slack-signature", "v0=00")]),
            ),
            (
                Provider::Shopify,
                WebhookSecret::new("sh"),
                headers(&[("x-shopify-hmac-sha256", "AA==")]),
            ),
            (
                Provider::StandardWebhooks,
                WebhookSecret::new("whsec_MfKQ9r8GKYqrTwjUPD8ILPZIo2LaLaSw"),
                headers(&[("svix-signature", "v1,AA=="), ("svix-id", "msg_1")]),
            ),
            (
                Provider::Linear,
                WebhookSecret::new("li"),
                headers(&[("linear-signature", "00")]),
            ),
        ];
        for (provider, secret, mut h) in cases {
            assert!(provider.can_resign());
            resign(provider, &mut h, body, &secret, now).unwrap();
            assert_eq!(detect(&h), Some(provider));
            assert_eq!(
                verify(provider, &input(&h, body, now), &secret),
                Verification::Valid,
                "{provider:?}"
            );
        }
        let mut h = HeaderMap::new();
        assert_eq!(
            resign(
                Provider::Discord,
                &mut h,
                body,
                &WebhookSecret::new("k"),
                now
            ),
            Err(WebhookError::Unsupported(Provider::Discord))
        );
        assert_eq!(
            resign(
                Provider::StandardWebhooks,
                &mut h,
                body,
                &WebhookSecret::new("whsec_!!"),
                now
            ),
            Err(WebhookError::InvalidSecret)
        );
    }

    #[test]
    fn secrets_never_print() {
        let secret = WebhookSecret::new("whsec_supersecret");
        assert!(!format!("{secret:?}").contains("supersecret"));
    }

    proptest! {
        #[test]
        fn verification_never_panics(
            value in "[ -~]{0,120}",
            body in proptest::collection::vec(any::<u8>(), 0..256),
        ) {
            let names = [
                "stripe-signature", "x-hub-signature-256", "x-hub-signature", "x-slack-signature",
                "x-slack-request-timestamp", "x-shopify-hmac-sha256", "webhook-signature",
                "webhook-id", "webhook-timestamp", "x-twilio-signature", "linear-signature",
                "x-signature-ed25519", "x-signature-timestamp",
            ];
            let mut h = HeaderMap::new();
            if let Ok(v) = HeaderValue::from_str(&value) {
                for name in names {
                    h.insert(name, v.clone());
                }
            }
            for provider in Provider::ALL {
                let _ = verify(provider, &input(&h, &body, 0), &WebhookSecret::new(value.clone()));
            }
        }
    }
}
