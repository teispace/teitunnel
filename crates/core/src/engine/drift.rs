//! Drift: someone changed this Mac's tunnel configuration outside Teitunnel (the
//! dashboard, another machine, the API). Detected by a config version higher than the
//! one Teitunnel last wrote (ARCHITECTURE §4.6).

use cf_api::IngressRule;
use serde::Serialize;

/// One route that differs between what Teitunnel wrote and what's there now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RuleChange {
    /// Hostname.
    pub hostname: String,
    /// Path regex.
    pub path: Option<String>,
    /// The service Teitunnel wrote (None: added elsewhere).
    pub before: Option<String>,
    /// The service now (None: removed elsewhere).
    pub after: Option<String>,
}

/// An outside edit of this Mac's tunnel configuration.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Drift {
    /// Tunnel id.
    pub tunnel_id: String,
    /// Version Teitunnel last wrote.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub applied_version: u64,
    /// Version now.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub current_version: u64,
    /// Routes that differ.
    pub changes: Vec<RuleChange>,
    /// What Teitunnel wrote (for "Restore mine").
    #[serde(skip)]
    pub ours: Vec<IngressRule>,
    /// What's there now (for "Keep theirs").
    #[serde(skip)]
    pub theirs: Vec<IngressRule>,
}

type Key<'a> = (&'a str, Option<&'a str>);

fn key(rule: &IngressRule) -> Option<Key<'_>> {
    Some((rule.hostname.as_deref()?, rule.path.as_deref()))
}

/// Route-level differences between two ingress lists (catch-alls ignored). A rule whose
/// service is the same but whose options changed shows the same service before and
/// after.
pub fn diff(ours: &[IngressRule], theirs: &[IngressRule]) -> Vec<RuleChange> {
    let find =
        |list: &'_ [IngressRule], k: Key<'_>| list.iter().find(|r| key(r) == Some(k)).cloned();
    let mut keys: Vec<Key<'_>> = ours.iter().chain(theirs).filter_map(key).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
        .filter_map(|k| {
            let (before, after) = (find(ours, k), find(theirs, k));
            (before != after).then(|| RuleChange {
                hostname: k.0.to_owned(),
                path: k.1.map(str::to_owned),
                before: before.map(|r| r.service),
                after: after.map(|r| r.service),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};

    use super::*;

    fn rule(host: Option<&str>, service: &str) -> IngressRule {
        IngressRule {
            hostname: host.map(str::to_owned),
            path: None,
            service: service.into(),
            origin_request: Map::new(),
            extra: Map::new(),
        }
    }

    #[test]
    fn reports_added_removed_and_changed_routes() {
        let ours = [
            rule(Some("a.xyz.com"), "http://localhost:3000"),
            rule(Some("b.xyz.com"), "http://localhost:4000"),
            rule(Some("c.xyz.com"), "http://localhost:5000"),
            rule(None, "http_status:404"),
        ];
        let mut tweaked = rule(Some("c.xyz.com"), "http://localhost:5000");
        tweaked
            .origin_request
            .insert("noTLSVerify".into(), json!(true));
        let theirs = [
            rule(Some("a.xyz.com"), "http://localhost:3000"),
            rule(Some("b.xyz.com"), "http://localhost:4001"),
            tweaked,
            rule(Some("d.xyz.com"), "http://localhost:6000"),
            rule(None, "http_status:503"),
        ];
        let changes = diff(&ours, &theirs);
        let summary: Vec<_> = changes
            .iter()
            .map(|c| (c.hostname.as_str(), c.before.as_deref(), c.after.as_deref()))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "b.xyz.com",
                    Some("http://localhost:4000"),
                    Some("http://localhost:4001")
                ),
                (
                    "c.xyz.com",
                    Some("http://localhost:5000"),
                    Some("http://localhost:5000")
                ),
                ("d.xyz.com", None, Some("http://localhost:6000")),
            ]
        );
        assert!(diff(&ours, &ours).is_empty());
    }
}
