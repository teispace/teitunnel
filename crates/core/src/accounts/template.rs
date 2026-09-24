//! The pre-filled "Create API token" link (research/cloudflare.md).

/// Permissions Teitunnel asks for: `(short key, type)`.
///
/// `argotunnel` (Cloudflare Tunnel) isn't in Cloudflare's documented key table and
/// unknown keys are dropped silently, so the UI also tells people to add
/// "Cloudflare Tunnel: Edit" if it's missing, and capability probing catches it.
/// `access` (Access: Apps and Policies) and `access_acct` (Access: Organizations,
/// Identity Providers, and Groups) let routes require a login from the start (D-065).
/// `analytics` (Zone ▸ Analytics ▸ Read) is what the zone-scoped GraphQL traffic
/// datasets need; `account_analytics` (Account Analytics ▸ Read) covers the
/// account-scoped ones (research/cloudflare-analytics.md).
/// `workers_scripts` (account) and `workers_routes` (zone) publish Snapshots and give
/// them a hostname (docs/research/cloudflare-snapshots.md).
/// `zone_waf` (custom and rate limiting rules), `transform_rules` (header rules) and
/// `access_service_tokens` protect hostnames at the edge (M12-04). None of the three is
/// in Cloudflare's documented key table yet (docs/research/cloudflare-edge-rules.md):
/// an unknown key is dropped silently, and the capability probe and the in-place fix
/// name the dashboard permission instead.
pub(super) const PERMISSIONS: &[(&str, &str)] = &[
    ("argotunnel", "edit"),
    ("dns", "edit"),
    ("zone", "read"),
    ("account_settings", "read"),
    ("access", "edit"),
    ("access_acct", "edit"),
    ("analytics", "read"),
    ("account_analytics", "read"),
    ("workers_scripts", "edit"),
    ("workers_routes", "edit"),
    ("zone_waf", "edit"),
    ("transform_rules", "edit"),
    ("access_service_tokens", "edit"),
];

/// The dashboard's list of the user's API tokens, where an existing token's permissions
/// can be edited without changing the token itself.
pub const TOKENS_PAGE: &str = "https://dash.cloudflare.com/profile/api-tokens";

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
        let access = "%7B%22key%22%3A%22access%22%2C%22type%22%3A%22edit%22%7D";
        let access_acct = "%7B%22key%22%3A%22access_acct%22%2C%22type%22%3A%22edit%22%7D";
        // Logins need both Access groups, asked for up front (D-065).
        assert!(url.contains(access) && url.contains(access_acct));
        // Analytics reads traffic per hostname (zone) and per account.
        assert!(url.contains("%7B%22key%22%3A%22analytics%22%2C%22type%22%3A%22read%22%7D"));
        assert!(
            url.contains("%7B%22key%22%3A%22account_analytics%22%2C%22type%22%3A%22read%22%7D")
        );
        // Snapshots: Workers Scripts (account) and Workers Routes (zones).
        assert!(url.contains("%7B%22key%22%3A%22workers_scripts%22%2C%22type%22%3A%22edit%22%7D"));
        assert!(url.contains("%7B%22key%22%3A%22workers_routes%22%2C%22type%22%3A%22edit%22%7D"));
        // Edge protection: rules, header rules and service tokens.
        for key in ["zone_waf", "transform_rules", "access_service_tokens"] {
            assert!(url.contains(&format!(
                "%7B%22key%22%3A%22{key}%22%2C%22type%22%3A%22edit%22%7D"
            )));
        }
        // Same encoding as Cloudflare's own example for `[{"key":"dns","type":"edit"}]`.
        assert_eq!(
            percent_encode(r#"[{"key":"dns","type":"edit"}]"#),
            "%5B%7B%22key%22%3A%22dns%22%2C%22type%22%3A%22edit%22%7D%5D"
        );
    }
}
