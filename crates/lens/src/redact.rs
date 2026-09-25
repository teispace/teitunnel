//! Masking of secrets in captured traffic.
//!
//! Captures keep raw data (so replay is exact); every read path masks through these
//! functions unless the caller explicitly asks for [`Redaction::revealed`]. Masking is
//! deliberately generous for headers (a masked header costs a click to reveal) and
//! precise for bodies (known token formats and values of secret-looking keys), so that
//! ordinary payloads such as commit hashes or ids stay readable.
//!
//! [`Redaction::revealed`]: crate::Redaction::revealed

use std::{borrow::Cow, sync::LazyLock};

use regex::Regex;

use crate::Redaction;

/// The text that replaces a masked value.
pub const MASK: &str = "[redacted]";

/// Headers that always carry credentials or signatures.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
    "x-auth-token",
    "x-access-token",
    "x-csrf-token",
    "x-xsrf-token",
    "x-goog-api-key",
    "x-amz-security-token",
    "x-gitlab-token",
    "x-webhook-secret",
    "x-hub-signature",
    "x-hub-signature-256",
    "stripe-signature",
    "x-slack-signature",
    "x-shopify-hmac-sha256",
    "svix-signature",
    "webhook-signature",
    "x-twilio-signature",
    "linear-signature",
    "x-signature-ed25519",
    "cf-access-jwt-assertion",
    "cf-access-client-secret",
    "cf-authorization",
];

/// Header names that look like they carry a secret, e.g. `x-my-service-token`.
static SENSITIVE_HEADER_NAME: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?i)(^|[-_])(token|secret|password|passwd|signature|api[-_]?key|access[-_]?key|private[-_]?key|session[-_]?(id|token)|credentials?|auth[-_]?token|hmac)([-_]|$)",
    )
    .ok()
});

/// Query/form/JSON keys whose values are secrets.
static SENSITIVE_KEY: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(.*[-_.])?(password|passwd|pwd|pass|secret|token|access[-_]?token|refresh[-_]?token|id[-_]?token|api[-_]?key|apikey|access[-_]?key|private[-_]?key|client[-_]?secret|authorization|auth|credentials?|session(id)?|sig|signature|otp|code[-_]?verifier|key)$",
    )
    .ok()
});

struct Rule {
    pattern: Regex,
    replacement: &'static str,
}

/// Token formats masked anywhere in text.
static TOKEN_RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    [
        // `Bearer abc…` / `Basic abc…` inside text.
        (
            r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]{8,}",
            "$1 [redacted]",
        ),
        // JWT / JWS (header.payload.signature, base64url, header starts with `{"`).
        (
            r"\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}",
            MASK,
        ),
        // Stripe secret/restricted keys and webhook secrets.
        (r"\b(sk|rk)_(live|test)_[A-Za-z0-9]{10,}", MASK),
        (r"\bwhsec_[A-Za-z0-9+/=]{10,}", MASK),
        // Slack tokens.
        (r"\bxox[abposr]-[A-Za-z0-9-]{10,}", MASK),
        // GitHub tokens.
        (r"\bgh[pousr]_[A-Za-z0-9]{30,}", MASK),
        (r"\bgithub_pat_[A-Za-z0-9_]{30,}", MASK),
        // GitLab, npm, Shopify, SendGrid.
        (r"\bglpat-[A-Za-z0-9_-]{20,}", MASK),
        (r"\bnpm_[A-Za-z0-9]{36}\b", MASK),
        (r"\bshp(at|ss|ca|pa)_[a-fA-F0-9]{32}\b", MASK),
        (r"\bSG\.[A-Za-z0-9_-]{16,}\.[A-Za-z0-9_-]{16,}", MASK),
        // AWS access key ids, Google API keys.
        (r"\b(AKIA|ASIA)[0-9A-Z]{16}\b", MASK),
        (r"\bAIza[0-9A-Za-z_-]{35}\b", MASK),
        // OpenAI / Anthropic style keys.
        (r"\bsk-(proj-|ant-[a-z0-9]+-)?[A-Za-z0-9_-]{20,}", MASK),
        // PEM private keys.
        (
            r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----",
            "[redacted private key]",
        ),
    ]
    .into_iter()
    .filter_map(|(pattern, replacement)| {
        // Constant patterns covered by tests; dropping a broken rule beats panicking.
        Regex::new(pattern).ok().map(|pattern| Rule {
            pattern,
            replacement,
        })
    })
    .collect()
});

static EMAIL: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b").ok());

/// `"secret_key": "value"` in JSON (string values only; formatting is preserved).
static JSON_SECRET_VALUE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#""([^"\\]{1,64})"(\s*:\s*)"((?:[^"\\]|\\.)*)""#).ok());

/// A long, opaque, mixed letters-and-digits value (a likely secret in a query string).
fn looks_like_secret(value: &str) -> bool {
    value.len() >= 32
        && value.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'+' | b'/' | b'=' | b'.' | b'%')
        })
        && value.bytes().any(|b| b.is_ascii_digit())
        && value.bytes().any(|b| b.is_ascii_alphabetic())
}

/// Whether a header's value is masked under `redaction`.
pub fn is_sensitive_header(name: &str, redaction: &Redaction) -> bool {
    if !redaction.mask {
        return false;
    }
    let lower = name.to_ascii_lowercase();
    SENSITIVE_HEADERS.contains(&lower.as_str())
        || redaction
            .extra_headers
            .iter()
            .any(|extra| extra.eq_ignore_ascii_case(&lower))
        || SENSITIVE_HEADER_NAME
            .as_ref()
            .is_some_and(|re| re.is_match(&lower))
}

/// Whether a query/form/JSON key names a secret.
pub fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_KEY.as_ref().is_some_and(|re| re.is_match(key))
}

/// Masks a header value, keeping what helps debugging: the auth scheme, cookie names
/// and `Set-Cookie` attributes.
pub fn mask_header<'a>(name: &str, value: &'a str, redaction: &Redaction) -> Cow<'a, str> {
    if !redaction.mask {
        return Cow::Borrowed(value);
    }
    if !is_sensitive_header(name, redaction) {
        return mask_text(value, redaction);
    }
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "authorization" | "proxy-authorization" => match value.split_once(' ') {
            Some((scheme, _)) => Cow::Owned(format!("{scheme} {MASK}")),
            None => Cow::Borrowed(MASK),
        },
        "cookie" => Cow::Owned(
            value
                .split(';')
                .map(|pair| match pair.trim().split_once('=') {
                    Some((name, _)) => format!("{name}={MASK}"),
                    None => MASK.to_owned(),
                })
                .collect::<Vec<_>>()
                .join("; "),
        ),
        "set-cookie" => {
            let mut parts = value.split(';');
            let first = parts.next().unwrap_or_default();
            let masked_first = match first.trim().split_once('=') {
                Some((name, _)) => format!("{name}={MASK}"),
                None => MASK.to_owned(),
            };
            let rest: Vec<&str> = parts.map(str::trim).collect();
            if rest.is_empty() {
                Cow::Owned(masked_first)
            } else {
                Cow::Owned(format!("{masked_first}; {}", rest.join("; ")))
            }
        }
        _ => Cow::Borrowed(MASK),
    }
}

/// Masks known token formats (and emails when asked) anywhere in `text`.
///
/// Borrows when nothing matched, so it's cheap for ordinary text.
pub fn mask_text<'a>(text: &'a str, redaction: &Redaction) -> Cow<'a, str> {
    if !redaction.mask {
        return Cow::Borrowed(text);
    }
    let mut out = Cow::Borrowed(text);
    for rule in TOKEN_RULES.iter() {
        if let Cow::Owned(replaced) = rule.pattern.replace_all(&out, rule.replacement) {
            out = Cow::Owned(replaced);
        }
    }
    if redaction.emails
        && let Some(re) = EMAIL.as_ref()
        && let Cow::Owned(replaced) = re.replace_all(&out, "[email]")
    {
        out = Cow::Owned(replaced);
    }
    out
}

/// Masks a URL query string or form body (`a=1&token=…`): values of secret keys, and
/// long opaque values, keeping keys and structure.
pub fn mask_query<'a>(query: &'a str, redaction: &Redaction) -> Cow<'a, str> {
    if !redaction.mask || query.is_empty() {
        return Cow::Borrowed(query);
    }
    let mut changed = false;
    let parts: Vec<Cow<'_, str>> = query
        .split('&')
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let decoded_key = percent_encoding::percent_decode_str(key).decode_utf8_lossy();
            if !value.is_empty() && (is_sensitive_key(&decoded_key) || looks_like_secret(value)) {
                changed = true;
                Cow::Owned(format!("{key}={MASK}"))
            } else {
                let masked = mask_text(pair, redaction);
                if matches!(masked, Cow::Owned(_)) {
                    changed = true;
                }
                masked
            }
        })
        .collect();
    if changed {
        Cow::Owned(parts.join("&"))
    } else {
        Cow::Borrowed(query)
    }
}

/// Masks a JSON document's secret-keyed string values and token formats, preserving
/// formatting (no re-serialization).
pub fn mask_json<'a>(text: &'a str, redaction: &Redaction) -> Cow<'a, str> {
    if !redaction.mask {
        return Cow::Borrowed(text);
    }
    let keyed = match JSON_SECRET_VALUE.as_ref() {
        Some(re) => re.replace_all(text, |caps: &regex::Captures<'_>| {
            let key = caps.get(1).map_or("", |m| m.as_str());
            let sep = caps.get(2).map_or("", |m| m.as_str());
            let value = caps.get(3).map_or("", |m| m.as_str());
            if is_sensitive_key(key) && !value.is_empty() {
                format!("\"{key}\"{sep}\"{MASK}\"")
            } else {
                caps.get(0).map_or(String::new(), |m| m.as_str().to_owned())
            }
        }),
        None => Cow::Borrowed(text),
    };
    match keyed {
        Cow::Borrowed(borrowed) => mask_text(borrowed, redaction),
        Cow::Owned(owned) => Cow::Owned(mask_text(&owned, redaction).into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn masked() -> Redaction {
        Redaction::masked()
    }

    #[test]
    fn all_rules_compile() {
        assert_eq!(TOKEN_RULES.len(), 15);
        assert!(SENSITIVE_HEADER_NAME.is_some());
        assert!(SENSITIVE_KEY.is_some());
        assert!(EMAIL.is_some());
        assert!(JSON_SECRET_VALUE.is_some());
    }

    #[test]
    fn headers_keep_useful_structure() {
        let r = masked();
        assert_eq!(
            mask_header("Authorization", "Bearer abc.def", &r),
            "Bearer [redacted]"
        );
        assert_eq!(mask_header("authorization", "opaque", &r), "[redacted]");
        assert_eq!(
            mask_header("Cookie", "sid=abc; theme=dark", &r),
            "sid=[redacted]; theme=[redacted]"
        );
        assert_eq!(
            mask_header("Set-Cookie", "sid=abc; Path=/; HttpOnly", &r),
            "sid=[redacted]; Path=/; HttpOnly"
        );
        assert_eq!(
            mask_header("Stripe-Signature", "t=1,v1=abc", &r),
            "[redacted]"
        );
        assert_eq!(mask_header("X-My-Service-Token", "abc", &r), "[redacted]");
        assert_eq!(
            mask_header("X-Hub-Signature-256", "sha256=abc", &r),
            "[redacted]"
        );
        assert_eq!(mask_header("Content-Type", "text/html", &r), "text/html");
        assert_eq!(mask_header("Keep-Alive", "timeout=5", &r), "timeout=5");
        assert_eq!(
            mask_header("Sec-WebSocket-Key", "dGhlIHNhbXBsZQ==", &r),
            "dGhlIHNhbXBsZQ=="
        );
        assert_eq!(
            mask_header("Authorization", "Bearer abc", &Redaction::revealed()),
            "Bearer abc"
        );
    }

    #[test]
    fn extra_headers_are_masked() {
        let r = Redaction {
            extra_headers: vec!["X-Internal".into()],
            ..Redaction::masked()
        };
        assert_eq!(mask_header("x-internal", "v", &r), "[redacted]");
    }

    #[test]
    fn tokens_in_text() {
        let r = masked();
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
        let text = format!(
            "token {jwt} and sk_live_51Habcdefghijk and ghp_abcdefghijklmnopqrstuvwxyz0123456789"
        );
        let out = mask_text(&text, &r);
        assert!(!out.contains("eyJhbGci"));
        assert!(!out.contains("sk_live_51"));
        assert!(!out.contains("ghp_"));
        assert!(matches!(
            mask_text("nothing secret here", &r),
            Cow::Borrowed(_)
        ));
        let pem = "-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----";
        assert_eq!(mask_text(pem, &r), "[redacted private key]");
    }

    #[test]
    fn commit_hashes_stay_readable() {
        let r = masked();
        let body = r#"{"after":"8d1b0f2e6c3a4b5d6e7f8091a2b3c4d5e6f70812"}"#;
        assert_eq!(mask_json(body, &r), body);
    }

    #[test]
    fn emails_only_when_asked() {
        let text = "from ada@example.com";
        assert_eq!(mask_text(text, &masked()), text);
        let r = Redaction {
            emails: true,
            ..Redaction::masked()
        };
        assert_eq!(mask_text(text, &r), "from [email]");
    }

    #[test]
    fn query_masks_keys_and_opaque_values() {
        let r = masked();
        assert_eq!(
            mask_query("page=2&access_token=abc&q=hello", &r),
            "page=2&access_token=[redacted]&q=hello"
        );
        assert_eq!(
            mask_query("x=AbCdEf0123456789AbCdEf0123456789xyz", &r),
            "x=[redacted]"
        );
        assert!(matches!(mask_query("a=1&b=2", &r), Cow::Borrowed(_)));
        assert_eq!(mask_query("key=abc", &r), "key=[redacted]");
        assert_eq!(mask_query("author=ada", &r), "author=ada");
    }

    #[test]
    fn json_masks_secret_keys_preserving_format() {
        let r = masked();
        let body = "{\n  \"email\": \"a@b.c\",\n  \"password\" : \"hunter2\",\n  \"nested\": {\"client_secret\":\"x\\\"y\"}\n}";
        let out = mask_json(body, &r);
        assert_eq!(
            out,
            "{\n  \"email\": \"a@b.c\",\n  \"password\" : \"[redacted]\",\n  \"nested\": {\"client_secret\":\"[redacted]\"}\n}"
        );
    }

    proptest! {
        #[test]
        fn masking_never_panics(text in any::<String>()) {
            let r = Redaction { emails: true, ..Redaction::masked() };
            let _ = mask_text(&text, &r);
            let _ = mask_query(&text, &r);
            let _ = mask_json(&text, &r);
            let _ = mask_header("cookie", &text, &r);
            let _ = mask_header("set-cookie", &text, &r);
            let _ = mask_header("authorization", &text, &r);
        }

        #[test]
        fn secret_values_never_survive(secret in "[A-Za-z0-9]{12,40}") {
            let r = Redaction::masked();
            let query = format!("password={secret}");
            prop_assert!(!mask_query(&query, &r).contains(&secret));
            let json = format!("{{\"api_key\":\"{secret}\"}}");
            prop_assert!(!mask_json(&json, &r).contains(&secret));
            let header = format!("Bearer {secret}");
            prop_assert!(!mask_header("authorization", &header, &r).contains(&secret));
            let stripe = format!("sk_live_{secret}");
            prop_assert!(!mask_text(&stripe, &r).contains(&stripe));
        }

        #[test]
        fn revealed_is_identity(text in any::<String>()) {
            let r = Redaction::revealed();
            prop_assert_eq!(mask_text(&text, &r), text.as_str());
            prop_assert_eq!(mask_query(&text, &r), text.as_str());
            prop_assert_eq!(mask_json(&text, &r), text.as_str());
        }
    }
}
