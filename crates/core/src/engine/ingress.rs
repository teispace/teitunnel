//! Ingress ordering. cloudflared matches rules top to bottom, so more specific rules
//! must come first and the catch-all must be last.

use cf_api::IngressRule;
use serde_json::Map;

/// The catch-all service every config ends with.
pub const CATCH_ALL: &str = "http_status:404";

fn specificity(rule: &IngressRule) -> (u8, u8, std::cmp::Reverse<usize>) {
    let host = rule.hostname.as_deref();
    let wildcard = host.is_some_and(|h| h.starts_with('*'));
    let has_path = rule.path.is_some();
    (
        u8::from(host.is_none()), // catch-all (and hostname-less rules) last
        u8::from(wildcard),       // exact hostnames before wildcards
        std::cmp::Reverse(rule.path.as_ref().map_or(0, String::len) + usize::from(has_path)),
    )
}

/// Sorts rules by specificity (stable, so equal rules keep the user's order), drops
/// existing catch-alls and appends one `http_status:404`.
pub fn sort_ingress(rules: Vec<IngressRule>) -> Vec<IngressRule> {
    let mut rules: Vec<IngressRule> = rules
        .into_iter()
        .filter(|rule| rule.hostname.is_some())
        .collect();
    rules.sort_by_key(specificity);
    rules.push(IngressRule {
        hostname: None,
        path: None,
        service: CATCH_ALL.to_owned(),
        origin_request: Map::new(),
        extra: Map::new(),
    });
    rules
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(host: Option<&str>, path: Option<&str>) -> IngressRule {
        IngressRule {
            hostname: host.map(str::to_owned),
            path: path.map(str::to_owned),
            service: "http://localhost:1".into(),
            origin_request: Map::new(),
            extra: Map::new(),
        }
    }

    #[test]
    fn orders_by_specificity_with_one_catch_all() {
        let sorted = sort_ingress(vec![
            rule(None, None),
            rule(Some("*.xyz.com"), None),
            rule(Some("app.xyz.com"), None),
            rule(Some("app.xyz.com"), Some("^/api/v2/")),
            rule(Some("app.xyz.com"), Some("^/api/")),
        ]);
        let order: Vec<_> = sorted
            .iter()
            .map(|r| (r.hostname.as_deref(), r.path.as_deref()))
            .collect();
        assert_eq!(
            order,
            [
                (Some("app.xyz.com"), Some("^/api/v2/")),
                (Some("app.xyz.com"), Some("^/api/")),
                (Some("app.xyz.com"), None),
                (Some("*.xyz.com"), None),
                (None, None),
            ]
        );
        assert_eq!(sorted.last().map(|r| r.service.as_str()), Some(CATCH_ALL));
    }
}
