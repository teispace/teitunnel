//! `teitunnel project init`: a starter `teitunnel.yml` from what this machine already
//! runs for the folder's project (its routes and shares whose service is one of the
//! project's), or, with none, shares for the project's running services.

use crate::{
    discovery::LocalService, domain::RouteOrigin, domain_shares::DomainShare, engine::RouteView,
};

/// What the starter file is made from.
#[derive(Debug, Clone, Default)]
pub struct InitInput {
    /// The project's name.
    pub name: String,
    /// The account's name, when several are connected.
    pub account: Option<String>,
    /// This machine's routes in the account.
    pub routes: Vec<RouteView>,
    /// Shares on the account's domains.
    pub shares: Vec<DomainShare>,
    /// Services listening on this machine.
    pub services: Vec<LocalService>,
}

fn port_of(origin: &str) -> Option<u16> {
    RouteOrigin::parse(origin)
        .ok()
        .filter(RouteOrigin::is_local)
        .and_then(|o| o.port())
}

/// A YAML string (JSON quoting is valid YAML).
fn quote(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

/// Renders the starter file.
pub fn render(input: &InitInput) -> String {
    let label = super::template::label(&input.name);
    let ports: Vec<u16> = input
        .services
        .iter()
        .filter(|s| {
            s.project
                .as_deref()
                .is_some_and(|p| super::template::label(p) == label)
        })
        .map(|s| s.port)
        .collect();
    let ours = |origin: &str| port_of(origin).is_some_and(|p| ports.contains(&p));
    let routes: Vec<&RouteView> = input
        .routes
        .iter()
        .filter(|r| !r.temporary && ours(&r.origin))
        .collect();
    let shares: Vec<&DomainShare> = input.shares.iter().filter(|s| ours(&s.origin)).collect();

    let mut out = format!(
        "# yaml-language-server: $schema={}\n# Teitunnel project file: `teitunnel up` applies it. Never put secrets here.\nversion: 1\nproject: {}\n",
        super::SCHEMA_URL,
        quote(&label)
    );
    if let Some(account) = &input.account {
        out.push_str(&format!("account: {}\n", quote(account)));
    }
    if !routes.is_empty() {
        out.push_str("\nroutes:\n");
        for route in &routes {
            out.push_str(&format!("  - hostname: {}\n", quote(&route.hostname)));
            out.push_str(&format!("    origin: {}\n", quote(&route.origin)));
            if let Some(path) = &route.path {
                out.push_str(&format!("    path: {}\n", quote(path)));
            }
            if let Some(access) = &route.access {
                out.push_str("    login:\n");
                for email in &access.emails {
                    out.push_str(&format!("      - {}\n", quote(email)));
                }
                for domain in &access.email_domains {
                    out.push_str(&format!("      - {}\n", quote(&format!("@{domain}"))));
                }
            }
            let options = serde_json::to_value(&route.options).unwrap_or_default();
            if let Some(map) = options.as_object().filter(|m| !m.is_empty()) {
                out.push_str("    originRequest:\n");
                for (key, value) in map {
                    out.push_str(&format!("      {key}: {value}\n"));
                }
            }
        }
    }
    let quick: Vec<u16> = if routes.is_empty() && shares.is_empty() {
        ports.clone()
    } else {
        Vec::new()
    };
    if !shares.is_empty() || !quick.is_empty() {
        out.push_str("\nshares:\n");
        for share in &shares {
            match port_of(&share.origin) {
                Some(port) => out.push_str(&format!("  - port: {port}\n")),
                None => out.push_str(&format!("  - url: {}\n", quote(&share.origin))),
            }
            out.push_str(&format!("    hostname: {}\n", quote(&share.hostname)));
        }
        for port in &quick {
            out.push_str(&format!(
                "  - port: {port}\n    # hostname: \"{{branch}}-{{project}}.example.com\"\n"
            ));
        }
    }
    if routes.is_empty() && shares.is_empty() && quick.is_empty() {
        out.push_str(concat!(
            "\n# routes:\n",
            "#   - hostname: app.example.com\n",
            "#     origin: 3000\n",
            "# shares:\n",
            "#   - port: 5173\n",
            "#     hostname: \"{branch}-{project}.example.com\"\n",
            "#     expires: 2h\n",
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        discovery::ServiceKind,
        domain::OriginOptions,
        engine::{AccessRule, DnsState},
    };

    fn route(hostname: &str, origin: &str) -> RouteView {
        RouteView {
            hostname: hostname.into(),
            path: None,
            origin: origin.into(),
            local: true,
            zone: Some("example.com".into()),
            dns: DnsState::Ok,
            access: None,
            client: None,
            tunnel_id: None,
            temporary: false,
            balanced: false,
            paused: false,
            options: OriginOptions::default(),
        }
    }

    fn service(port: u16, project: &str) -> LocalService {
        LocalService {
            port,
            all_interfaces: false,
            pid: 1,
            process: "node".into(),
            kind: ServiceKind::Vite,
            project: Some(project.into()),
            folder: None,
            origin: format!("http://localhost:{port}"),
        }
    }

    #[test]
    fn starts_from_the_projects_routes_and_shares() {
        let mut api = route("api.example.com", "http://localhost:4000");
        api.path = Some("^/v1".into());
        api.access = Some(AccessRule {
            emails: vec!["me@example.com".into()],
            email_domains: vec!["team.io".into()],
            bypass: Vec::new(),
        });
        api.options.http_host_header = Some("localhost:4000".into());
        let input = InitInput {
            name: "Shop".into(),
            account: Some("Acme".into()),
            routes: vec![api, route("other.example.com", "http://localhost:9999")],
            shares: vec![DomainShare {
                account_id: "a".into(),
                hostname: "demo.example.com".into(),
                origin: "5173".into(),
                owner: "app".into(),
                expires_at: None,
                created_at: 0,
                source: None,
                folder: false,
                paused: false,
                schedule: None,
            }],
            services: vec![
                service(4000, "shop"),
                service(5173, "shop"),
                service(9999, "blog"),
            ],
        };
        let text = render(&input);
        insta::assert_snapshot!(text);
        let parsed = super::super::parse(&text);
        assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
        let file = parsed.file.unwrap();
        assert_eq!(file.routes.len(), 1, "only the project's");
        assert_eq!(file.shares[0].hostname.as_deref(), Some("demo.example.com"));
    }

    #[test]
    fn suggests_shares_for_running_services_or_an_example() {
        let input = InitInput {
            name: "blog".into(),
            services: vec![service(3000, "blog")],
            ..InitInput::default()
        };
        let text = render(&input);
        assert!(text.contains("- port: 3000"), "{text}");
        assert!(!super::super::parse(&text).has_errors());

        let empty = render(&InitInput {
            name: "empty".into(),
            ..InitInput::default()
        });
        assert!(empty.contains("# routes:"));
        assert!(!super::super::parse(&empty).has_errors());
    }
}
