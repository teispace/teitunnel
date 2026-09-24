//! A project file is checked into a repository, so it must never hold a secret. Keys
//! that name one must reference it (`password: { env: NAME }`), and values that look
//! like credentials are refused wherever they appear, even under keys this version
//! doesn't know.

/// Keys (lower-cased, without `-`/`_`) whose value is a secret.
const SECRET_KEYS: &[&str] = &[
    "password",
    "passwd",
    "passphrase",
    "secret",
    "clientsecret",
    "token",
    "apitoken",
    "apikey",
    "tunneltoken",
    "runtoken",
    "privatekey",
    "accesskey",
    "secretkey",
    "webhooksecret",
];

/// Whether a key's value must be a secret reference, not a literal.
pub(crate) fn is_secret_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect();
    SECRET_KEYS.contains(&normalized.as_str())
}

/// What kind of credential `value` looks like, if any.
pub(crate) fn credential_kind(value: &str) -> Option<&'static str> {
    let v = value.trim();
    let starts = |prefixes: &[&str]| prefixes.iter().any(|p| v.starts_with(p));
    if v.contains("-----BEGIN") && v.contains("PRIVATE KEY") {
        return Some("private key");
    }
    // cloudflared run tokens are base64 JSON starting `{"a":"`.
    if v.starts_with("eyJhIjoi") && v.len() > 60 {
        return Some("tunnel token");
    }
    // A JWT: three base64url parts, the first a JSON header.
    let parts: Vec<&str> = v.split('.').collect();
    if parts.len() == 3
        && v.starts_with("eyJ")
        && parts
            .iter()
            .all(|p| p.len() >= 8 && p.bytes().all(is_base64url))
    {
        return Some("JSON web token");
    }
    if starts(&["ttk_"]) && v.len() > 20 {
        return Some("Teitunnel API key");
    }
    if starts(&["ghp_", "gho_", "ghs_", "ghu_", "github_pat_"]) && v.len() > 20 {
        return Some("GitHub token");
    }
    if starts(&["sk_live_", "sk_test_", "rk_live_", "whsec_"]) {
        return Some("Stripe key");
    }
    if starts(&["xoxb-", "xoxp-", "xoxa-", "xoxs-"]) {
        return Some("Slack token");
    }
    if starts(&["AKIA", "ASIA"])
        && v.len() == 20
        && v.bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return Some("AWS access key");
    }
    if starts(&["sk-ant-", "sk-proj-"]) {
        return Some("AI provider key");
    }
    // Cloudflare API tokens: 40 characters of letters, digits, `-` and `_`, mixed case.
    if v.len() == 40
        && v.bytes().all(is_base64url)
        && v.bytes().any(|b| b.is_ascii_uppercase())
        && v.bytes().any(|b| b.is_ascii_lowercase())
        && v.bytes().any(|b| b.is_ascii_digit())
    {
        return Some("Cloudflare API token");
    }
    // Cloudflare Global API keys: 37 lower-case hex characters.
    if v.len() == 37
        && v.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Some("Cloudflare API key");
    }
    None
}

fn is_base64url(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_secret_keys() {
        for key in [
            "password",
            "Password",
            "api_token",
            "apiKey",
            "client-secret",
            "token",
        ] {
            assert!(is_secret_key(key), "{key}");
        }
        for key in ["hostname", "origin", "tokens_used", "passwordless"] {
            assert!(!is_secret_key(key), "{key}");
        }
    }

    #[test]
    fn recognises_credentials() {
        let cases = [
            (
                "eyJhIjoiMTIzNDU2Nzg5MCIsInQiOiJhYmNkZWYtMTIzNC01Njc4IiwicyI6Ik1USXpORFUyIn0=",
                "tunnel token",
            ),
            (
                "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0In0.c2lnbmF0dXJlLXZhbHVl",
                "JSON web token",
            ),
            ("ghp_abcdefghijklmnopqrstuvwxyz0123456789", "GitHub token"),
            ("sk_live_51H8abcdEFGH", "Stripe key"),
            ("AKIAIOSFODNN7EXAMPLE", "AWS access key"),
            (
                "Y2hhbmdlLW1lLXRoaXMtaXMtbm90LXJlYWw0MDAx",
                "Cloudflare API token",
            ),
            (
                "0123456789abcdef0123456789abcdef01234",
                "Cloudflare API key",
            ),
            ("-----BEGIN OPENSSH PRIVATE KEY-----\nabc", "private key"),
            ("ttk_0123456789abcdef0123456789", "Teitunnel API key"),
        ];
        for (value, kind) in cases {
            assert_eq!(credential_kind(value), Some(kind), "{value}");
        }
    }

    #[test]
    fn leaves_ordinary_values_alone() {
        for value in [
            "app.example.com",
            "{branch}-{project}.example.com",
            "http://localhost:3000",
            "^/api/v1/.*",
            "me@example.com",
            "dist",
            "localhost:5173",
            "shop.localhost",
        ] {
            assert_eq!(credential_kind(value), None, "{value}");
        }
    }
}
