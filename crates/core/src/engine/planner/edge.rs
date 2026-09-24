//! Planning edge protection (rules scoped to one hostname) and service tokens.
//!
//! Only Teitunnel's own rules are added, changed or removed, one by one; other rules in
//! the same phases are counted towards the plan's quotas and otherwise left alone.
//! Rules are added first, then changed, then removed from the end of a phase towards
//! its start, so undoing in reverse puts every removed rule back where it was.

use std::collections::BTreeSet;

use cf_api::NewRule;

use super::{Builder, PlanError};
use crate::domain::Hostname;
use crate::engine::{
    access::{service_tokens_of, without_service_token},
    edge::{
        self, EdgeProtection, EdgeState, MAX_SERVICE_TOKENS, QuotaKind, RateLimitSpec, RuleKind,
    },
    types::{Step, TokenRef, Warning},
};

/// What the plan does to the rules, before they're ordered.
#[derive(Default)]
struct Changes {
    creates: Vec<Step>,
    updates: Vec<Step>,
    /// With each rule's position, to remove from the end first.
    deletes: Vec<(u32, Step)>,
}

impl Changes {
    fn create(&mut self, state: &EdgeState, kind: RuleKind, hostnames: Vec<String>, rule: NewRule) {
        let phase = kind.phase();
        self.creates.push(Step::CreateEdgeRule {
            zone_id: state.zone_id.clone(),
            phase: phase.to_owned(),
            ruleset_id: state.ruleset(phase).and_then(|r| r.id.clone()),
            kind,
            hostnames,
            rule,
        });
    }

    fn update(
        &mut self,
        state: &EdgeState,
        kind: RuleKind,
        hostnames: Vec<String>,
        observed: &edge::ObservedRule,
        rule: NewRule,
    ) {
        self.updates.push(Step::UpdateEdgeRule {
            zone_id: state.zone_id.clone(),
            ruleset_id: state
                .ruleset(kind.phase())
                .and_then(|r| r.id.clone())
                .unwrap_or_default(),
            rule_id: observed.id.clone(),
            kind,
            hostnames,
            rule,
            previous: observed.rule.clone(),
        });
    }

    fn delete(
        &mut self,
        state: &EdgeState,
        kind: RuleKind,
        hostnames: Vec<String>,
        position: u32,
        observed: &edge::ObservedRule,
    ) {
        let phase = kind.phase();
        self.deletes.push((
            position,
            Step::DeleteEdgeRule {
                zone_id: state.zone_id.clone(),
                phase: phase.to_owned(),
                ruleset_id: state
                    .ruleset(phase)
                    .and_then(|r| r.id.clone())
                    .unwrap_or_default(),
                rule_id: observed.id.clone(),
                kind,
                hostnames,
                previous: observed.rule.clone(),
                position,
            },
        ));
    }

    /// Rules added minus rules removed in `quota`'s phases.
    fn delta(&self, quota: QuotaKind) -> i64 {
        let in_quota = |phase: &str| quota.phases().contains(&phase);
        let created = self
            .creates
            .iter()
            .filter(|s| matches!(s, Step::CreateEdgeRule { phase, .. } if in_quota(phase)))
            .count();
        let deleted = self
            .deletes
            .iter()
            .filter(|(_, s)| matches!(s, Step::DeleteEdgeRule { phase, .. } if in_quota(phase)))
            .count();
        i64::try_from(created).unwrap_or(i64::MAX) - i64::try_from(deleted).unwrap_or(i64::MAX)
    }

    fn touches(&self, quota: QuotaKind) -> bool {
        let kind_of = |s: &Step| match s {
            Step::CreateEdgeRule { kind, .. }
            | Step::UpdateEdgeRule { kind, .. }
            | Step::DeleteEdgeRule { kind, .. } => Some(kind.quota()),
            _ => None,
        };
        self.creates
            .iter()
            .chain(&self.updates)
            .chain(self.deletes.iter().map(|(_, s)| s))
            .any(|s| kind_of(s) == Some(quota))
    }
}

/// Makes the edge enforce `protection` for `hostname`.
pub(super) fn protect(
    b: &mut Builder<'_>,
    hostname: &Hostname,
    protection: &EdgeProtection,
) -> Result<(), PlanError> {
    b.zone_id(hostname)?;
    let state = b
        .snapshot
        .edge
        .as_ref()
        .ok_or_else(|| PlanError::NoZone(hostname.to_string()))?;
    let mut changes = Changes::default();
    let host = vec![hostname.to_string()];

    let desired = edge::hostname_rules(hostname, protection);
    for kind in [
        RuleKind::Block,
        RuleKind::Challenge,
        RuleKind::RequestHeaders,
        RuleKind::ResponseHeaders,
    ] {
        let existing = state.owned_rule(kind.phase(), &edge::marker(hostname, kind));
        let wanted = desired.iter().find(|(k, _)| *k == kind).map(|(_, r)| r);
        match (existing, wanted) {
            (None, Some(rule)) => changes.create(state, kind, host.clone(), rule.clone()),
            (Some((_, observed)), Some(rule)) if observed.rule != *rule => {
                changes.update(state, kind, host.clone(), observed, rule.clone());
            }
            (Some((position, observed)), None) => {
                changes.delete(state, kind, host.clone(), position, observed);
            }
            _ => {}
        }
    }
    rate_limit(state, hostname, protection.rate_limit, &mut changes)?;

    for quota in [
        QuotaKind::Custom,
        QuotaKind::RateLimit,
        QuotaKind::Transform,
    ] {
        if !changes.touches(quota) {
            continue;
        }
        let limit = quota.limit(state.plan);
        let used = i64::from(state.used(quota)) + changes.delta(quota);
        let used = u32::try_from(used.max(0)).unwrap_or(u32::MAX);
        if changes.delta(quota) > 0 && used > limit {
            return Err(PlanError::EdgeQuotaFull {
                quota,
                zone: state.zone.clone(),
                limit,
            });
        }
        b.warnings.push(Warning::EdgeQuota {
            quota,
            zone: state.zone.clone(),
            used,
            limit,
        });
    }

    let Changes {
        creates,
        updates,
        mut deletes,
    } = changes;
    deletes.sort_by_key(|d| std::cmp::Reverse(d.0));
    b.steps.extend(creates);
    b.steps.extend(updates);
    b.steps.extend(deletes.into_iter().map(|(_, step)| step));
    Ok(())
}

/// Moves `hostname` into the shared rate limit with `wanted` (or out of every one).
fn rate_limit(
    state: &EdgeState,
    hostname: &Hostname,
    wanted: Option<RateLimitSpec>,
    changes: &mut Changes,
) -> Result<(), PlanError> {
    let host = hostname.as_str();
    let groups = state.rate_limits();
    let current = groups.iter().find(|(_, _, _, hosts)| hosts.contains(host));
    if current.map(|(_, _, spec, _)| *spec) == wanted {
        return Ok(());
    }
    let limits = state.plan.limits();
    if let Some(spec) = &wanted {
        if !limits.host_rate_limit {
            return Err(PlanError::EdgeRateLimitNeedsPro(state.zone.clone()));
        }
        if spec.period > limits.longest_period {
            return Err(PlanError::EdgeRateLimitPeriod {
                zone: state.zone.clone(),
                longest: limits.longest_period,
            });
        }
    }
    let list = |hosts: &BTreeSet<String>| hosts.iter().cloned().collect::<Vec<_>>();
    // Out of the rule it's in now; the rule goes when nobody's left in it.
    let mut remaining: Option<(&edge::ObservedRule, RateLimitSpec, BTreeSet<String>)> = None;
    if let Some((position, observed, spec, hosts)) = current {
        let mut rest = hosts.clone();
        rest.remove(host);
        if rest.is_empty() {
            changes.delete(state, RuleKind::RateLimit, list(hosts), *position, observed);
        } else {
            changes.update(
                state,
                RuleKind::RateLimit,
                list(&rest),
                observed,
                edge::rate_limit_rule(spec, &rest),
            );
            remaining = Some((observed, *spec, rest));
        }
    }
    let Some(spec) = wanted else {
        return Ok(());
    };
    let current_id = current.map(|(_, r, _, _)| r.id.as_str());
    if let Some((_, observed, _, hosts)) = groups
        .iter()
        .find(|(_, r, s, _)| *s == spec && Some(r.id.as_str()) != current_id)
    {
        // Joins the hostnames that already have this limit.
        let mut hosts = hosts.clone();
        hosts.insert(host.to_owned());
        changes.update(
            state,
            RuleKind::RateLimit,
            list(&hosts),
            observed,
            edge::rate_limit_rule(&spec, &hosts),
        );
        return Ok(());
    }
    let freed = u32::from(current.is_some() && remaining.is_none());
    let used = state.used(QuotaKind::RateLimit).saturating_sub(freed);
    let limit = QuotaKind::RateLimit.limit(state.plan);
    if used >= limit {
        // Another of Teitunnel's limits holds the only room: the same limit is needed.
        let other = groups
            .iter()
            .find(|(_, r, _, _)| Some(r.id.as_str()) != current_id)
            .map(|(_, _, s, h)| (*s, h.clone()))
            .or_else(|| remaining.map(|(_, s, h)| (s, h)));
        return Err(match other {
            Some((other, hosts)) => PlanError::EdgeRateLimitConflict {
                zone: state.zone.clone(),
                hostnames: list(&hosts).join(", "),
                requests: other.requests,
                period: other.period,
            },
            None => PlanError::EdgeQuotaFull {
                quota: QuotaKind::RateLimit,
                zone: state.zone.clone(),
                limit,
            },
        });
    }
    let hosts = BTreeSet::from([host.to_owned()]);
    changes.create(
        state,
        RuleKind::RateLimit,
        list(&hosts),
        edge::rate_limit_rule(&spec, &hosts),
    );
    Ok(())
}

fn tokens<'a>(b: &Builder<'a>) -> &'a [edge::ObservedServiceToken] {
    b.snapshot.service_tokens.as_deref().unwrap_or_default()
}

fn own_token<'a>(
    b: &Builder<'a>,
    token_id: &str,
) -> Result<&'a edge::ObservedServiceToken, PlanError> {
    let token = tokens(b)
        .iter()
        .find(|t| t.id == token_id)
        .ok_or_else(|| PlanError::NoSuchServiceToken(token_id.to_owned()))?;
    if !token.owned {
        return Err(PlanError::ServiceTokenNotOwned(token.name.clone()));
    }
    Ok(token)
}

/// Creates a token and lets it through the hostname's login.
pub(super) fn create_token(
    b: &mut Builder<'_>,
    hostname: &Hostname,
    label: &str,
) -> Result<(), PlanError> {
    let label = label.trim();
    if label.is_empty() || label.chars().count() > 40 || label.chars().any(char::is_control) {
        return Err(PlanError::InvalidTokenLabel);
    }
    b.zone_id(hostname)?;
    let existing = tokens(b);
    if existing.len() >= MAX_SERVICE_TOKENS {
        return Err(PlanError::ServiceTokenLimit(MAX_SERVICE_TOKENS as u32));
    }
    let name = edge::service_token_name(hostname, label);
    if existing.iter().any(|t| t.name.eq_ignore_ascii_case(&name)) {
        return Err(PlanError::ServiceTokenExists(label.to_owned()));
    }
    let domain = hostname.to_string();
    let access = b.snapshot.access.as_ref();
    let app = match access.and_then(|a| a.app(&domain)) {
        Some(app) if !app.owned => return Err(PlanError::AccessAppExists(domain)),
        Some(app) => Some((app.id.clone(), app.definition.clone())),
        None => {
            if access.and_then(|a| a.organization) == Some(false) {
                return Err(PlanError::ZeroTrustNotSetUp);
            }
            b.warnings.push(Warning::MachineOnly {
                domain: domain.clone(),
            });
            None
        }
    };
    b.steps.push(Step::CreateServiceToken {
        hostname: domain.clone(),
        name,
    });
    b.steps.push(Step::AllowServiceToken {
        domain,
        app,
        token: TokenRef::Created,
    });
    Ok(())
}

/// Takes a token out of the hostname's login, then deletes it (that can't be undone,
/// so it's last).
pub(super) fn revoke_token(
    b: &mut Builder<'_>,
    hostname: &Hostname,
    token_id: &str,
) -> Result<(), PlanError> {
    let token = own_token(b, token_id)?.clone();
    let domain = hostname.to_string();
    if let Some(app) = b
        .snapshot
        .access
        .as_ref()
        .and_then(|a| a.app(&domain))
        .filter(|a| a.owned && service_tokens_of(&a.definition).contains(&token.id))
    {
        let without = without_service_token(&app.definition, &token.id);
        if without.policies.is_empty() {
            b.steps.push(Step::DeleteAccessApp {
                id: app.id.clone(),
                previous: app.definition.clone(),
            });
        } else {
            b.steps.push(Step::UpdateAccessApp {
                id: app.id.clone(),
                app: without,
                previous: app.definition.clone(),
            });
        }
    }
    b.steps.push(Step::DeleteServiceToken { token });
    Ok(())
}

/// Gives a token a new secret.
pub(super) fn rotate_token(b: &mut Builder<'_>, token_id: &str) -> Result<(), PlanError> {
    let token = own_token(b, token_id)?.clone();
    b.steps.push(Step::RotateServiceToken { token });
    Ok(())
}
