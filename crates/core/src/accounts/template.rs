//! The pre-filled "Create API token" link (research/cloudflare.md).

/// Permissions Teitunnel asks for: `(short key, type)`.
///
/// `argotunnel` (Cloudflare Tunnel) isn't in Cloudflare's documented key table and
/// unknown keys are dropped silently, so the UI also tells people to add
/// "Cloudflare Tunnel: Edit" if it's missing, and capability probing catches it.
pub(super) const PERMISSIONS: &[(&str, &str)] = &[
    ("argotunnel", "edit"),
    ("dns", "edit"),
    ("zone", "read"),
    ("account_settings", "read"),
];

/// The dashboard URL that opens "Create API token" with Teitunnel's permissions,
/// all accounts and all zones pre-selected.
pub fn token_template_url() -> String {
    let permissions: Vec<String> = PERMISSIONS
        .iter()
        .map(|(key, kind)| format!(r#"{{"key":"{key}","type":"{kind}"}}"#))
        .collect();
    let json = format!("[{}]", permissions.join(","));
    format!(
        "https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys={}&accountId=*&zoneId=all&name=Teitunnel",
        percent_encode(&json)
    )
}

fn percent_encode(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_documented_format() {
        let url = token_template_url();
        assert!(url.starts_with("https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys=%5B%7B%22key%22%3A%22argotunnel%22%2C%22type%22%3A%22edit%22%7D"));
        assert!(url.ends_with("&accountId=*&zoneId=all&name=Teitunnel"));
        // Same encoding as Cloudflare's own example for `[{"key":"dns","type":"edit"}]`.
        assert_eq!(
            percent_encode(r#"[{"key":"dns","type":"edit"}]"#),
            "%5B%7B%22key%22%3A%22dns%22%2C%22type%22%3A%22edit%22%7D%5D"
        );
    }
}
