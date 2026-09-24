//! The Doctor's checks for local domains: not served, ports, trust, `.test` names,
//! expiring certificates. Each issue has a fix the app applies in place.

use serde::Serialize;

use super::{
    LocalDomains,
    model::{LocalDomainsStatus, NameResolution, PortProblem, PortReason, TrustView},
};
use crate::{
    doctor::{Fix, Issue, Severity},
    text::{Text, msg, msg::doctor as m},
};

/// A CA expiring within this many seconds is flagged.
const CA_WARN: i64 = 30 * 24 * 60 * 60;
/// A certificate expiring within this many seconds wasn't renewed in time.
const LEAF_WARN: i64 = 5 * 24 * 60 * 60;

/// A fix for a local domains issue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum LocalDomainFix {
    /// Trust the local certificate authority.
    Trust,
    /// Add the `.test` resolver entry (the walkthrough; `pkexec` on Linux).
    SetUpResolver,
    /// Start serving again.
    Restart,
    /// Issue fresh certificates.
    RenewCertificates,
    /// Replace the certificate authority (and trust the new one).
    RenewCa,
}

/// What the checks look at.
#[derive(Debug, Clone)]
pub struct LocalFacts {
    /// The status.
    pub status: LocalDomainsStatus,
    /// Trust, when some domain uses HTTPS.
    pub trust: Option<TrustView>,
    /// When the CA expires (Unix seconds), if loaded.
    pub ca_expires: Option<i64>,
    /// Certificates issued, by name, and when they expire.
    pub certificates: Vec<(String, i64)>,
    /// Now (Unix seconds).
    pub now: i64,
}

impl LocalDomains {
    /// Reads what the checks need (runs the trust tools; no prompts).
    pub async fn facts(&self) -> LocalFacts {
        let status = self.status().await;
        let https = status.domains.iter().any(|d| d.https);
        let trust = if https {
            Some(self.trust_status().await)
        } else {
            None
        };
        let mut certificates = Vec::new();
        for domain in status.domains.iter().filter(|d| d.https) {
            if let Some(expiry) = self.certificate_expiry(&domain.name).await {
                certificates.push((domain.name.clone(), expiry));
            }
        }
        LocalFacts {
            ca_expires: self.ca_expiry(),
            status,
            trust,
            certificates,
            now: time::OffsetDateTime::now_utc().unix_timestamp(),
        }
    }

    /// The local domains issues.
    pub async fn doctor(&self) -> Vec<Issue> {
        diagnose(&self.facts().await)
    }

    /// Applies a fix.
    ///
    /// # Errors
    /// The fix failed (the resolver entry needs the walkthrough outside Linux).
    pub async fn fix(&self, action: LocalDomainFix) -> Result<(), super::LocalDomainError> {
        match action {
            LocalDomainFix::Trust => self.trust(super::TrustOptions::default()).await.map(drop),
            LocalDomainFix::SetUpResolver => self.run_as_admin(super::AdminTask::Resolver).await,
            LocalDomainFix::Restart => self.sync().await,
            LocalDomainFix::RenewCertificates => self.renew().await.map(drop),
            LocalDomainFix::RenewCa => self.renew_ca().await.map(drop),
        }
    }
}

fn issue(
    check: &str,
    severity: Severity,
    subject: &str,
    title: Text,
    detail: Text,
    evidence: Vec<Text>,
    fixes: Vec<LocalDomainFix>,
) -> Issue {
    Issue {
        id: format!("{check}:-:{subject}"),
        check: check.to_owned(),
        severity,
        account_id: None,
        subject: subject.to_owned(),
        label: msg::raw(subject),
        title,
        detail,
        evidence,
        fixes: fixes
            .into_iter()
            .map(|action| Fix::LocalDomains { action })
            .collect(),
        tunnel_id: None,
    }
}

fn port_issue(problem: &PortProblem) -> Issue {
    use m::local_port as p;
    let detail = match problem.reason {
        PortReason::InUse => p::in_use(problem.port),
        PortReason::PermissionDenied => p::denied(problem.port),
        PortReason::Other => p::other(problem.port),
    };
    match problem.fallback {
        Some(fallback) => issue(
            "local.port",
            Severity::Info,
            &problem.port.to_string(),
            p::title_fallback(fallback, problem.port),
            detail,
            Vec::new(),
            Vec::new(),
        ),
        None => issue(
            "local.port",
            Severity::Error,
            &problem.port.to_string(),
            p::title_none(problem.port),
            detail,
            Vec::new(),
            vec![LocalDomainFix::Restart],
        ),
    }
}

/// Runs the checks.
pub fn diagnose(facts: &LocalFacts) -> Vec<Issue> {
    let status = &facts.status;
    let mut issues = Vec::new();
    if status.domains.is_empty() {
        return issues;
    }
    let port_failed = status.port_problems.iter().any(|p| p.fallback.is_none());
    if let Some(error) = &status.error
        && !port_failed
    {
        issues.push(issue(
            "local.stopped",
            Severity::Error,
            "local-domains",
            m::local_stopped::title(),
            error.clone(),
            Vec::new(),
            vec![LocalDomainFix::Restart],
        ));
    }
    issues.extend(status.port_problems.iter().map(port_issue));
    if let Some(trust) = &facts.trust
        && !trust.trusted
    {
        issues.push(issue(
            "local.untrusted",
            Severity::Warning,
            "local-ca",
            m::local_untrusted::title(),
            m::local_untrusted::detail(),
            trust
                .ca
                .iter()
                .map(|ca| m::local_untrusted::authority(&ca.common_name))
                .collect(),
            vec![LocalDomainFix::Trust],
        ));
    }
    if let Some(error) = &status.resolver.error {
        issues.push(issue(
            "local.dns",
            Severity::Error,
            "test",
            m::local_dns::title(),
            error.clone(),
            Vec::new(),
            vec![LocalDomainFix::Restart],
        ));
    } else if let Some(domain) = status
        .domains
        .iter()
        .find(|d| d.name.ends_with(".test") && d.resolution == NameResolution::NeedsResolver)
    {
        issues.push(issue(
            "local.resolver",
            Severity::Warning,
            "test",
            m::local_resolver::title(&domain.name),
            m::local_resolver::detail(),
            status
                .resolver
                .setup
                .iter()
                .map(|s| msg::raw(&s.command))
                .collect(),
            vec![LocalDomainFix::SetUpResolver],
        ));
    }
    for domain in &status.domains {
        if domain.resolution == NameResolution::Elsewhere {
            issues.push(issue(
                "local.elsewhere",
                Severity::Warning,
                &domain.name,
                m::local_elsewhere::title(&domain.name),
                m::local_elsewhere::detail(),
                Vec::new(),
                Vec::new(),
            ));
        }
    }
    if let Some(expires) = facts.ca_expires
        && expires - facts.now < CA_WARN
    {
        issues.push(issue(
            "local.ca_expiry",
            Severity::Warning,
            "local-ca",
            m::local_ca_expiry::title(),
            m::local_ca_expiry::detail(),
            Vec::new(),
            vec![LocalDomainFix::RenewCa],
        ));
    }
    for (name, expires) in &facts.certificates {
        if expires - facts.now < LEAF_WARN {
            issues.push(issue(
                "local.certificate",
                Severity::Warning,
                name,
                m::local_certificate::title(name),
                m::local_certificate::detail(),
                Vec::new(),
                vec![LocalDomainFix::RenewCertificates],
            ));
        }
    }
    issues
}
