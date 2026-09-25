//! Keeping secrets away from agents, and messages readable for them.
//!
//! The core never returns credentials; this is the second line: every string that
//! leaves the server passes through [`teitunnel_core::redact::redact`] (bearer tokens,
//! `token=…`, JWTs and tunnel tokens, PEM blocks), and captured HTTP headers that carry
//! credentials are masked, unless the server was started with `--allow-secrets`.

use serde::Serialize;
use serde_json::Value;
use teitunnel_core::{redact::redact, text::Text};

/// What a masked header value reads as.
pub const MASKED: &str = "[masked]";

/// Headers whose values are credentials (lowercase).
const SECRET_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "api-key",
    "x-auth-token",
    "x-access-token",
    "x-csrf-token",
    "x-xsrf-token",
    "cf-access-jwt-assertion",
    "cf-access-token",
    "cf-access-client-secret",
    // Webhook signatures: a replay with a copied signature is as good as the secret.
    "stripe-signature",
    "x-hub-signature",
    "x-hub-signature-256",
    "x-slack-signature",
    "x-shopify-hmac-sha256",
    "svix-signature",
    "webhook-signature",
    "x-twilio-signature",
    "linear-signature",
    "x-signature-ed25519",
];

/// Whether a header's value is a credential that stays masked.
pub fn is_secret_header(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    SECRET_HEADERS.contains(&name.as_str())
        || name.ends_with("-token")
        || name.ends_with("-secret")
        || name.contains("api-key")
}

/// Masks a header value when it's a credential (unless secrets are allowed).
pub fn header_value(name: &str, value: &str, allow_secrets: bool) -> String {
    if allow_secrets {
        value.to_owned()
    } else if is_secret_header(name) {
        MASKED.to_owned()
    } else {
        redact(value).into_owned()
    }
}

/// Redacts a free-form string (unless secrets are allowed).
pub fn text(value: &str, allow_secrets: bool) -> String {
    if allow_secrets {
        value.to_owned()
    } else {
        redact(value).into_owned()
    }
}

/// Every string value in `value`, redacted. Keys are left alone.
pub fn value(value: Value, allow_secrets: bool) -> Value {
    if allow_secrets {
        return value;
    }
    match value {
        Value::String(s) => Value::String(redact(&s).into_owned()),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|v| self::value(v, false)).collect())
        }
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(k, v)| (k, self::value(v, false)))
                .collect(),
        ),
        other => other,
    }
}

/// `value` as JSON with every message (`{ "key": "core.…", "args": … }`) rendered as
/// English text: agents read sentences, not catalog keys.
pub fn english<T: Serialize>(value: &T) -> Value {
    fn walk(value: Value) -> Value {
        match value {
            Value::Object(map)
                if map.len() == 2
                    && map
                        .get("key")
                        .and_then(Value::as_str)
                        .is_some_and(|k| k.starts_with("core."))
                    && map.contains_key("args") =>
            {
                match serde_json::from_value::<Text>(Value::Object(map.clone())) {
                    Ok(text) => Value::String(text.english()),
                    Err(_) => Value::Object(map),
                }
            }
            Value::Object(map) => {
                Value::Object(map.into_iter().map(|(k, v)| (k, walk(v))).collect())
            }
            Value::Array(items) => Value::Array(items.into_iter().map(walk).collect()),
            other => other,
        }
    }
    walk(serde_json::to_value(value).unwrap_or(Value::Null))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn masks_credential_headers() {
        for name in [
            "Authorization",
            "cookie",
            "Set-Cookie",
            "X-Hub-Signature-256",
            "x-my-token",
            "X-Api-Key",
        ] {
            assert_eq!(header_value(name, "abc", false), MASKED, "{name}");
        }
        assert_eq!(
            header_value("content-type", "application/json", false),
            "application/json"
        );
        assert_eq!(
            header_value("authorization", "Bearer abc", true),
            "Bearer abc"
        );
    }

    #[test]
    fn redacts_every_string_but_not_keys() {
        let out = value(
            json!({ "line": "connecting with token=abc123", "token": 3, "list": ["Bearer xyz"] }),
            false,
        );
        assert_eq!(out["line"], "connecting with token=[redacted]");
        assert_eq!(out["token"], 3);
        assert_eq!(out["list"][0], "Bearer [redacted]");
        let raw = json!({ "line": "token=abc" });
        assert_eq!(value(raw.clone(), true), raw);
    }

    #[test]
    fn renders_messages_as_english() {
        let out = english(&json!({
            "a": { "key": "core.raw", "args": { "text": "Create tunnel" } },
            "b": { "key": "other", "args": {} }
        }));
        assert_eq!(out["a"], "Create tunnel");
        assert_eq!(out["b"]["key"], "other");
    }
}
