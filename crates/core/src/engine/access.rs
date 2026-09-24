//! Protecting a route with a login (Cloudflare Access): who may reach it, how that maps
//! to an Access application, and what was observed. Pure.
//!
//! Teitunnel manages one self-hosted application per protected route, named
//! "Teitunnel · <domain>", with a single "allow" policy for the listed emails and email
//! domains. It changes only applications it created (the local ownership index); a
//! hostname already protected by someone else's application is left alone.

use cf_api::{AccessApp, AccessPolicy, NewAccessApp};
use serde::{Deserialize, Serialize};

use super::types::{Intent, RouteSpec};
use crate::domain::{Hostname, PathRule};

use crate::text::{Text, UserText, english_display, msg};

/// Who may reach a protected route: any of these emails, or anyone at these domains.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AccessRule {
    /// Email addresses, e.g. `me@xyz.com`.
    pub emails: Vec<String>,
    /// Email domains, e.g. `xyz.com`.
    pub email_domains: Vec<String>,
}

/// Why an access rule was rejected. Messages are shown next to the field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccessRuleError {
    /// Nobody would be allowed in.
    Empty,
    /// Not an email address.
    Email(String),
    /// Not a domain.
    Domain(String),
}

impl UserText for AccessRuleError {
    fn text(&self) -> Text {
        match self {
            Self::Empty => msg::error::access_rule::empty(),
            Self::Email(value) => msg::error::access_rule::email(value),
            Self::Domain(value) => msg::error::access_rule::domain(value),
        }
    }
}

english_display!(AccessRuleError);

fn valid_domain(domain: &str) -> bool {
    Hostname::parse(domain).is_ok() && domain.contains('.')
}

impl AccessRule {
    /// Trims, lower-cases, de-duplicates and checks the entries.
    ///
    /// # Errors
    /// See [`AccessRuleError`].
    pub fn normalized(&self) -> Result<Self, AccessRuleError> {
        let clean = |items: &[String]| {
            let mut out: Vec<String> = items
                .iter()
                .map(|s| s.trim().trim_start_matches('@').to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect();
            out.sort();
            out.dedup();
            out
        };
        let emails = clean(&self.emails);
        let email_domains = clean(&self.email_domains);
        if emails.is_empty() && email_domains.is_empty() {
            return Err(AccessRuleError::Empty);
        }
        for email in &emails {
            let valid = email
                .split_once('@')
                .is_some_and(|(user, domain)| !user.is_empty() && valid_domain(domain));
            if !valid {
                return Err(AccessRuleError::Email(email.clone()));
            }
        }
        if let Some(bad) = email_domains.iter().find(|d| !valid_domain(d)) {
            return Err(AccessRuleError::Domain(bad.clone()));
        }
        Ok(Self {
            emails,
            email_domains,
        })
    }

    /// Who's allowed, in any language: `me@xyz.com, @team.com` (`@domain` is everyone
    /// there).
    pub fn people(&self) -> String {
        self.emails
            .iter()
            .cloned()
            .chain(self.email_domains.iter().map(|d| format!("@{d}")))
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The rule an application's policies express, if they're exactly the kind Teitunnel
    /// writes (one allow policy of emails and email domains).
    pub fn from_app(app: &AccessApp) -> Option<Self> {
        Self::from_policies(&app.policies)
    }

    /// The rule a definition expresses (see [`Self::from_app`]).
    pub fn from_new(app: &NewAccessApp) -> Option<Self> {
        Self::from_policies(&app.policies)
    }

    fn from_policies(policies: &[AccessPolicy]) -> Option<Self> {
        // The machines' (Service Auth) policy says nothing about people.
        let people: Vec<&AccessPolicy> = policies.iter().filter(|p| !is_machines(p)).collect();
        let [policy] = people.as_slice() else {
            return None;
        };
        if policy.decision != "allow" {
            return None;
        }
        let mut rule = Self::default();
        for include in &policy.include {
            if let Some(email) = cf_api::rule_email(include) {
                rule.emails.push(email.to_owned());
            } else {
                rule.email_domains
                    .push(cf_api::rule_email_domain(include)?.to_owned());
            }
        }
        rule.normalized().ok()
    }
}

/// Why a route can't be protected as asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AccessDomainError {
    /// Access protects path prefixes, not arbitrary regular expressions.
    PathPattern(String),
}

impl UserText for AccessDomainError {
    fn text(&self) -> Text {
        match self {
            Self::PathPattern(pattern) => msg::error::access_domain::path_pattern(pattern),
        }
    }
}

english_display!(AccessDomainError);

/// The Access domain for a route: the hostname, plus the path when the route's path
/// rule is a plain prefix (`^/admin`, `/admin/`, `^/admin/.*`).
///
/// # Errors
/// A path rule that isn't a plain prefix.
pub fn access_domain(
    hostname: &Hostname,
    path: Option<&PathRule>,
) -> Result<String, AccessDomainError> {
    let Some(path) = path else {
        return Ok(hostname.to_string());
    };
    let raw = path.as_str();
    let prefix = raw
        .trim_start_matches('^')
        .trim_end_matches(".*")
        .trim_end_matches('/');
    let plain = prefix.starts_with('/')
        && prefix[1..]
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./~".contains(&b));
    if plain && prefix.len() > 1 {
        Ok(format!("{hostname}{prefix}"))
    } else {
        Err(AccessDomainError::PathPattern(raw.to_owned()))
    }
}

/// The application Teitunnel creates for `domain`, letting in whoever `rule` allows.
pub fn app_definition(domain: &str, rule: &AccessRule) -> NewAccessApp {
    let include = rule
        .emails
        .iter()
        .map(|e| cf_api::email_rule(e))
        .chain(
            rule.email_domains
                .iter()
                .map(|d| cf_api::email_domain_rule(d)),
        )
        .collect();
    NewAccessApp {
        name: format!("{}{domain}", cf_api::TEITUNNEL_PREFIX),
        domain: domain.to_owned(),
        kind: "self_hosted".into(),
        session_duration: "24h".into(),
        app_launcher_visible: false,
        policies: vec![AccessPolicy {
            id: None,
            name: "Allowed people".into(),
            decision: "allow".into(),
            include,
            precedence: Some(1),
            reusable: false,
            app_count: None,
        }],
    }
}

/// The name of the Service Auth policy Teitunnel adds for service tokens.
pub(crate) const MACHINES_POLICY: &str = "Machines";

/// Whether a policy is a Service Auth policy of service tokens only (machines).
pub(crate) fn is_machines(policy: &AccessPolicy) -> bool {
    policy.decision == "non_identity"
        && !policy.include.is_empty()
        && policy
            .include
            .iter()
            .all(|rule| cf_api::rule_service_token(rule).is_some())
}

/// The service tokens an application's definition lets through.
pub(crate) fn service_tokens_of(app: &NewAccessApp) -> Vec<String> {
    app.policies
        .iter()
        .filter(|p| is_machines(p))
        .flat_map(|p| p.include.iter().filter_map(cf_api::rule_service_token))
        .map(str::to_owned)
        .collect()
}

/// An application for `domain` that only service tokens pass (no login for people).
pub(crate) fn machine_only_definition(domain: &str) -> NewAccessApp {
    NewAccessApp {
        name: format!("{}{domain}", cf_api::TEITUNNEL_PREFIX),
        domain: domain.to_owned(),
        kind: "self_hosted".into(),
        session_duration: "24h".into(),
        app_launcher_visible: false,
        policies: Vec::new(),
    }
}

/// `app` also letting in the service token `id` (added to its Machines policy, which
/// comes first so machines never see a login page).
pub(crate) fn with_service_token(app: &NewAccessApp, id: &str) -> NewAccessApp {
    let mut app = app.clone();
    let rule = cf_api::service_token_rule(id);
    match app.policies.iter_mut().find(|p| is_machines(p)) {
        Some(policy) => {
            if !policy.include.contains(&rule) {
                policy.include.push(rule);
            }
        }
        None => app.policies.insert(
            0,
            AccessPolicy {
                id: None,
                name: MACHINES_POLICY.into(),
                decision: "non_identity".into(),
                include: vec![rule],
                precedence: None,
                reusable: false,
                app_count: None,
            },
        ),
    }
    renumber(&mut app);
    app
}

/// `app` no longer letting in the service token `id` (its Machines policy goes when
/// it's empty).
pub(crate) fn without_service_token(app: &NewAccessApp, id: &str) -> NewAccessApp {
    let mut app = app.clone();
    let rule = cf_api::service_token_rule(id);
    for policy in app.policies.iter_mut().filter(|p| is_machines(p)) {
        policy.include.retain(|r| *r != rule);
    }
    app.policies
        .retain(|p| !(p.decision == "non_identity" && p.include.is_empty()));
    renumber(&mut app);
    app
}

/// Keeps `from`'s Machines policies in `to` (a change of who may log in mustn't lock
/// machines out).
pub(crate) fn keep_machines(to: &NewAccessApp, from: &NewAccessApp) -> NewAccessApp {
    let mut app = to.clone();
    let machines: Vec<AccessPolicy> = from
        .policies
        .iter()
        .filter(|p| is_machines(p))
        .cloned()
        .collect();
    app.policies.retain(|p| !is_machines(p));
    for (i, policy) in machines.into_iter().enumerate() {
        app.policies.insert(i, policy);
    }
    renumber(&mut app);
    app
}

fn renumber(app: &mut NewAccessApp) {
    for (policy, precedence) in app.policies.iter_mut().zip(1u32..) {
        policy.precedence = Some(precedence);
    }
}

/// An existing application as a definition that recreates it (to undo a change).
pub(crate) fn definition_of(app: &AccessApp) -> NewAccessApp {
    NewAccessApp {
        name: app.name.clone(),
        domain: app.domain.clone(),
        kind: if app.kind.is_empty() {
            "self_hosted".into()
        } else {
            app.kind.clone()
        },
        session_duration: app.session_duration.clone().unwrap_or_else(|| "24h".into()),
        app_launcher_visible: false,
        policies: app
            .policies
            .iter()
            .map(|p| AccessPolicy {
                id: None,
                ..p.clone()
            })
            .collect(),
    }
}

/// An Access application for one of the domains involved.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservedAccessApp {
    /// Application id.
    pub id: String,
    /// Its domain.
    pub domain: String,
    /// Created by Teitunnel (in the ownership index).
    pub owned: bool,
    /// The rule it expresses, when it's in the form Teitunnel writes.
    pub rule: Option<AccessRule>,
    /// A definition that recreates it.
    pub definition: NewAccessApp,
}

/// What a change needs to know about Access, so the observer reads only that. A token
/// without Access permissions keeps working for routes that don't use a login.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AccessNeed {
    /// Read whether Zero Trust is set up and how many login methods there are
    /// (the change protects a route).
    pub setup: bool,
    /// Domains whose applications the plan depends on.
    pub domains: Vec<String>,
    /// Also read the applications Teitunnel created for the hostnames in scope (the
    /// change may remove or move a route's protection).
    pub owned: bool,
}

impl AccessNeed {
    /// Nothing about Access.
    pub fn none() -> Self {
        Self::default()
    }

    /// What planning `intent` needs.
    pub fn of(intent: &Intent) -> Self {
        let requested = |route: &RouteSpec| {
            route
                .access
                .as_ref()
                .and_then(|_| access_domain(&route.hostname, route.path.as_ref()).ok())
        };
        match intent {
            Intent::AddRoute { route } => {
                let domains: Vec<String> = requested(route).into_iter().collect();
                Self {
                    setup: !domains.is_empty(),
                    domains,
                    owned: false,
                }
            }
            Intent::UpdateRoute { route, .. } => {
                let domains: Vec<String> = requested(route).into_iter().collect();
                Self {
                    setup: !domains.is_empty(),
                    domains,
                    owned: true,
                }
            }
            Intent::RemoveRoute { .. } | Intent::RemoveTunnel => Self {
                owned: true,
                ..Self::default()
            },
            Intent::RemoveLogin { domain } => Self {
                domains: vec![domain.clone()],
                ..Self::default()
            },
            Intent::PublishSnapshot { site, .. } => {
                let domains: Vec<String> = site
                    .access
                    .as_ref()
                    .and(site.address.hostname())
                    .map(ToString::to_string)
                    .into_iter()
                    .collect();
                Self {
                    setup: !domains.is_empty(),
                    domains,
                    owned: false,
                }
            }
            Intent::UpdateSnapshot { site, .. } => {
                let domains: Vec<String> = site
                    .access
                    .as_ref()
                    .and(site.address.hostname())
                    .map(ToString::to_string)
                    .into_iter()
                    .collect();
                Self {
                    setup: !domains.is_empty(),
                    domains,
                    owned: site.address.hostname().is_some(),
                }
            }
            Intent::DeleteSnapshot { site } => Self {
                owned: site.address.hostname().is_some(),
                ..Self::default()
            },
            Intent::CreateServiceToken { hostname, .. } => Self {
                setup: true,
                domains: vec![hostname.to_string()],
                owned: false,
            },
            Intent::RevokeServiceToken { hostname, .. } => Self {
                domains: vec![hostname.to_string()],
                ..Self::default()
            },
            Intent::ProtectHostname { .. }
            | Intent::RotateServiceToken { .. }
            | Intent::RollbackSnapshot { .. }
            | Intent::ImportRoutes { .. }
            | Intent::DeleteRecord { .. }
            | Intent::RestoreConfig { .. }
            | Intent::AddNetwork { .. }
            | Intent::RemoveNetwork { .. }
            | Intent::CreateTunnel { .. }
            | Intent::BalanceRoute { .. }
            | Intent::UnbalanceRoute { .. }
            | Intent::Reserve { .. }
            | Intent::Release { .. } => Self::default(),
        }
    }

    /// Whether anything needs reading.
    pub fn is_empty(&self) -> bool {
        !self.setup && !self.owned && self.domains.is_empty()
    }
}

/// The host part of an Access domain (`app.xyz.com/admin` → `app.xyz.com`).
pub(crate) fn domain_host(domain: &str) -> &str {
    domain.split_once('/').map_or(domain, |(host, _)| host)
}

/// Access as observed for a change.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AccessState {
    /// Whether Zero Trust is set up (an organization exists). Only read when the change
    /// adds or changes a login.
    pub organization: Option<bool>,
    /// Number of login methods (identity providers), when read.
    pub login_methods: Option<usize>,
    /// Applications for the domains involved.
    pub apps: Vec<ObservedAccessApp>,
}

impl AccessState {
    /// The application for `domain`, if any.
    pub fn app(&self, domain: &str) -> Option<&ObservedAccessApp> {
        self.apps
            .iter()
            .find(|a| a.domain.eq_ignore_ascii_case(domain))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn rule(emails: &[&str], domains: &[&str]) -> AccessRule {
        AccessRule {
            emails: emails.iter().map(|s| (*s).to_owned()).collect(),
            email_domains: domains.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[test]
    fn normalizes_and_validates_rules() {
        assert_eq!(
            rule(&[" Me@XYZ.com", "me@xyz.com"], &["@Team.io"]).normalized(),
            Ok(rule(&["me@xyz.com"], &["team.io"]))
        );
        assert_eq!(rule(&[], &[" "]).normalized(), Err(AccessRuleError::Empty));
        assert_eq!(
            rule(&["nope"], &[]).normalized(),
            Err(AccessRuleError::Email("nope".into()))
        );
        assert_eq!(
            rule(&[], &["localhost"]).normalized(),
            Err(AccessRuleError::Domain("localhost".into()))
        );
        assert_eq!(
            rule(&["me@xyz.com"], &["team.io"]).people(),
            "me@xyz.com, @team.io"
        );
    }

    #[test]
    fn maps_paths_to_access_domains() {
        let host = Hostname::parse("app.xyz.com").unwrap();
        let domain =
            |p: Option<&str>| access_domain(&host, p.map(|p| PathRule::parse(p).unwrap()).as_ref());
        assert_eq!(domain(None).unwrap(), "app.xyz.com");
        assert_eq!(domain(Some("^/admin")).unwrap(), "app.xyz.com/admin");
        assert_eq!(domain(Some("^/admin/.*")).unwrap(), "app.xyz.com/admin");
        assert!(matches!(
            domain(Some("^/(a|b)")),
            Err(AccessDomainError::PathPattern(_))
        ));
    }

    #[test]
    fn round_trips_through_an_application() {
        let wanted = rule(&["me@xyz.com"], &["team.io"]);
        let definition = app_definition("app.xyz.com", &wanted);
        assert_eq!(definition.name, "Teitunnel · app.xyz.com");
        let app: AccessApp = serde_json::from_value(json!({
            "id": "a1", "name": definition.name, "domain": "app.xyz.com", "type": "self_hosted",
            "session_duration": "24h",
            "policies": [{"id": "p1", "name": "Allowed people", "decision": "allow", "precedence": 1,
                          "include": serde_json::to_value(&definition.policies[0].include).unwrap()}]
        }))
        .unwrap();
        assert_eq!(AccessRule::from_app(&app), Some(wanted));
        let recreated = definition_of(&app);
        assert_eq!(
            recreated.policies[0].id, None,
            "recreated inline, not linked"
        );
        // A policy with rules Teitunnel doesn't write isn't claimed as a simple rule.
        let mut custom = app.clone();
        custom.policies[0]
            .include
            .push(json!({"ip": {"ip": "10.0.0.0/8"}}));
        assert_eq!(AccessRule::from_app(&custom), None);
    }

    #[test]
    fn service_tokens_join_and_leave_the_machines_policy() {
        let people = app_definition("app.xyz.com", &rule(&["me@xyz.com"], &[]));
        let one = with_service_token(&people, "tok1");
        assert_eq!(one.policies.len(), 2);
        assert_eq!(one.policies[0].decision, "non_identity", "machines first");
        assert_eq!(one.policies[1].precedence, Some(2));
        assert_eq!(
            AccessRule::from_new(&one),
            AccessRule::from_new(&people),
            "the people allowed are still read"
        );
        let two = with_service_token(&one, "tok2");
        assert_eq!(service_tokens_of(&two), ["tok1", "tok2"]);
        assert_eq!(with_service_token(&two, "tok2"), two, "added once");
        assert_eq!(without_service_token(&two, "tok1").policies.len(), 2);
        assert_eq!(
            without_service_token(&one, "tok1").policies,
            people.policies
        );
        // Changing who may log in keeps the machines.
        let others = app_definition("app.xyz.com", &rule(&[], &["team.io"]));
        assert_eq!(
            service_tokens_of(&keep_machines(&others, &two)),
            ["tok1", "tok2"]
        );
        let machines = with_service_token(&machine_only_definition("api.xyz.com"), "tok9");
        assert_eq!(AccessRule::from_new(&machines), None);
        assert!(without_service_token(&machines, "tok9").policies.is_empty());
    }
}
