//! Redaction of secrets from free-form text (log lines, error messages, diagnostics).
//!
//! This is a safety net. Secrets are wrapped in [`crate::Secret`] and never logged on
//! purpose; this catches what slips through, e.g. a token echoed back in an HTTP error
//! body or a cloudflared log line.

use std::{borrow::Cow, sync::LazyLock};

use regex::Regex;

/// The text that replaces a redacted value.
pub const REDACTED: &str = "[redacted]";

struct Rule {
    pattern: Regex,
    replacement: &'static str,
}

static RULES: LazyLock<Vec<Rule>> = LazyLock::new(|| {
    [
        // `Authorization: Bearer abc`, `bearer abc`
        (r"(?i)\b(bearer)\s+[A-Za-z0-9._~+/=-]+", "$1 [redacted]"),
        // `token=abc`, `"api_key": "abc"`, `TUNNEL_TOKEN=abc`, `client_secret: abc`
        (
            r#"(?i)\b([a-z_]*(?:token|secret|password|passwd|api[_-]?key|authorization)[a-z_]*)(["']?\s*[:=]\s*["']?)[^\s"',;}&]+"#,
            "$1$2[redacted]",
        ),
        // JWTs and base64-encoded JSON blobs (cloudflared tunnel tokens start with `eyJ`).
        (r"\beyJ[A-Za-z0-9_+/=-]{16,}(?:\.[A-Za-z0-9_+/=-]+){0,2}", "[redacted]"),
        // PEM blocks, e.g. the `ARGO TUNNEL TOKEN` in cert.pem.
        (
            r"-----BEGIN [A-Z ]+-----[\s\S]*?-----END [A-Z ]+-----",
            "[redacted pem]",
        ),
    ]
    .into_iter()
    .filter_map(|(pattern, replacement)| {
        // The patterns are constants covered by tests; a failure here is a programming
        // error, and dropping the rule is safer than panicking in the logging path.
        Regex::new(pattern)
            .ok()
            .map(|pattern| Rule { pattern, replacement })
    })
    .collect()
});

/// Returns `text` with anything that looks like a credential replaced.
///
/// Borrows when there's nothing to redact, so it is cheap on the hot logging path.
#[must_use]
pub fn redact(text: &str) -> Cow<'_, str> {
    let mut out = Cow::Borrowed(text);
    for rule in RULES.iter() {
        if let Cow::Owned(replaced) = rule.pattern.replace_all(&out, rule.replacement) {
            out = Cow::Owned(replaced);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const CF_TOKEN: &str = "Xy1aB2cD3eF4gH5iJ6kL7mN8oP9qR0sT1uV2wX3y";
    const TUNNEL_TOKEN: &str =
        "eyJhIjoiMTIzNDU2Nzg5MCIsInQiOiJhYmNkZWYiLCJzIjoiWjJWdVpYSmhkR1ZrIn0=";

    #[test]
    fn all_rules_compile() {
        assert_eq!(RULES.len(), 4);
    }

    #[test]
    fn leaves_clean_text_borrowed() {
        let line = "tunnel app connected to edge colo=AMS conn=0";
        assert!(matches!(redact(line), Cow::Borrowed(_)));
    }

    #[test]
    fn redacts_bearer_header() {
        let line = format!("Authorization: Bearer {CF_TOKEN}");
        let out = redact(&line);
        assert!(!out.contains(CF_TOKEN), "{out}");
    }

    #[test]
    fn redacts_key_value_pairs() {
        for line in [
            format!("token={CF_TOKEN}"),
            format!("TUNNEL_TOKEN={CF_TOKEN} other=1"),
            format!(r#"{{"api_token": "{CF_TOKEN}", "id": "x"}}"#),
            format!("client_secret: {CF_TOKEN}"),
            format!("refresh_token={CF_TOKEN}&state=abc"),
        ] {
            let out = redact(&line);
            assert!(!out.contains(CF_TOKEN), "not redacted: {out}");
            assert!(out.contains(REDACTED), "{out}");
        }
    }

    #[test]
    fn keeps_surrounding_structure() {
        let out = redact(r#"{"api_token": "abc123", "id": "x"}"#);
        assert_eq!(out, r#"{"api_token": "[redacted]", "id": "x"}"#);
    }

    #[test]
    fn redacts_tunnel_tokens_and_jwts() {
        let line = format!("running with {TUNNEL_TOKEN} now");
        let out = redact(&line);
        assert_eq!(out, "running with [redacted] now");
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJlLXZhbHVl";
        assert!(!redact(jwt).contains("c2lnbmF0dXJl"));
    }

    #[test]
    fn redacts_pem_blocks() {
        let pem = "cert:\n-----BEGIN ARGO TUNNEL TOKEN-----\nabc\ndef\n-----END ARGO TUNNEL TOKEN-----\nend";
        assert_eq!(redact(pem), "cert:\n[redacted pem]\nend");
    }

    #[test]
    fn does_not_touch_hostnames_or_ids() {
        let line = "route app.example.com -> http://localhost:3000 tunnel=6ff42ae2-765d-4adf-8112-31c55c1551ef";
        assert_eq!(redact(line), line);
    }
}
