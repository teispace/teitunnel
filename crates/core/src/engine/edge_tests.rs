//! Planner scenarios for edge protection and service tokens (insta snapshots of the
//! steps as the preview shows them), the quota rules, foreign rules left alone, and the
//! idempotency property.

use cf_api::NewRule;

use super::{
    access::{AccessRule, AccessState, ObservedAccessApp, app_definition, with_service_token},
    edge::{
        BotMode, EdgeHeaderOp, EdgeProtection, EdgeState, HeaderRule, LimitAction, ObservedRule,
        ObservedRuleset, ObservedServiceToken, PHASES, QuotaKind, RateLimitSpec, ZonePlan,
        rate_limit_rule,
    },
    planner::{PlanError, plan},
    simulate::apply,
    types::{Intent, Plan, Snapshot, Step, Warning, ZoneRef},
};
use crate::domain::Hostname;

fn host(h: &str) -> Hostname {
    Hostname::parse(h).unwrap()
}

fn edge(plan: ZonePlan, rules: &[(&str, ObservedRule)]) -> EdgeState {
    EdgeState {
        zone_id: "z-xyz".into(),
        zone: "xyz.com".into(),
        plan,
        rulesets: PHASES
            .iter()
            .map(|phase| {
                let rules: Vec<ObservedRule> = rules
                    .iter()
                    .filter(|(p, _)| p == phase)
                    .map(|(_, r)| r.clone())
                    .collect();
                ObservedRuleset {
                    phase: (*phase).to_owned(),
                    id: (!rules.is_empty()).then(|| format!("rs-{phase}")),
                    rules,
                }
            })
            .collect(),
    }
}

fn snapshot(edge_state: EdgeState) -> Snapshot {
    Snapshot {
        account_id: "acc".into(),
        machine_name: "Mac".into(),
        zones: vec![ZoneRef {
            id: "z-xyz".into(),
            name: "xyz.com".into(),
        }],
        tunnel: None,
        tunnel_names: Vec::new(),
        elsewhere: Vec::new(),
        records: Vec::new(),
        access: None,
        networks: None,
        balance: None,
        site: None,
        held: Vec::new(),
        owner: "me@Mac".into(),
        now: 0,
        edge: vec![edge_state],
        service_tokens: None,
        database: None,
        front: Vec::new(),
    }
}

fn foreign(id: &str, action: &str) -> ObservedRule {
    ObservedRule {
        id: id.into(),
        rule: NewRule {
            action: action.into(),
            expression: "(ip.src eq 192.0.2.1)".into(),
            description: "Their own rule".into(),
            enabled: true,
            action_parameters: None,
            ratelimit: None,
        },
        owned: false,
    }
}

fn limit(requests: u32, period: u32) -> RateLimitSpec {
    RateLimitSpec {
        requests,
        period,
        action: LimitAction::Block,
    }
}

fn ours_rate_limit(id: &str, spec: RateLimitSpec, hosts: &[&str]) -> ObservedRule {
    ObservedRule {
        id: id.into(),
        rule: rate_limit_rule(&spec, &hosts.iter().map(|h| (*h).to_owned()).collect()),
        owned: true,
    }
}

fn protect(hostname: &str, protection: EdgeProtection) -> Intent {
    Intent::ProtectHostname {
        hostname: host(hostname),
        protection,
    }
}

/// The steps as the preview shows them, and the warnings.
fn view(plan: &Plan) -> (Vec<String>, Vec<Warning>) {
    (
        plan.steps
            .iter()
            .map(|s| s.describe(&plan.tunnel_name).english())
            .collect(),
        plan.warnings.clone(),
    )
}

/// Applying the plan and planning again changes nothing.
fn assert_idempotent(intent: &Intent, before: &Snapshot) -> Plan {
    let first = plan(intent, before).unwrap();
    let after = apply(before, &first);
    let again = plan(intent, &after).unwrap();
    assert!(again.steps.is_empty(), "{:?}", again.steps);
    first
}

fn everything() -> EdgeProtection {
    EdgeProtection {
        bots: BotMode::Challenge,
        ai_crawlers: true,
        rate_limit: None,
        request_headers: vec![HeaderRule {
            name: "X-Env".into(),
            op: EdgeHeaderOp::Set,
            value: Some("preview".into()),
        }],
        response_headers: vec![HeaderRule {
            name: "X-Robots-Tag".into(),
            op: EdgeHeaderOp::Set,
            value: Some("noindex".into()),
        }],
    }
}

#[test]
fn first_protection_adds_one_rule_per_kind_scoped_to_the_hostname() {
    let before = snapshot(edge(
        ZonePlan::Free,
        &[(cf_api::PHASE_CUSTOM, foreign("theirs", "block"))],
    ));
    let p = assert_idempotent(&protect("app.xyz.com", everything()), &before);
    insta::assert_yaml_snapshot!(view(&p));
    for step in &p.steps {
        let Step::CreateEdgeRule { rule, .. } = step else {
            panic!("{step:?}")
        };
        assert!(
            rule.expression
                .starts_with(r#"(http.host eq "app.xyz.com")"#)
        );
    }
}

#[test]
fn changing_and_turning_off_touches_only_teitunnels_rules() {
    let start = snapshot(edge(ZonePlan::Pro, &[]));
    let on = apply(
        &start,
        &plan(&protect("app.xyz.com", everything()), &start).unwrap(),
    );
    // Their rule arrives in between.
    let mut on = on;
    if let Some(e) = on.edge.first_mut() {
        e.rulesets[0].rules.insert(0, foreign("theirs", "block"));
    }
    let block = EdgeProtection {
        bots: BotMode::Block,
        ..everything()
    };
    let p = assert_idempotent(&protect("app.xyz.com", block), &on);
    insta::assert_yaml_snapshot!("changing_the_bot_mode", view(&p));

    let p = assert_idempotent(&protect("app.xyz.com", EdgeProtection::default()), &on);
    insta::assert_yaml_snapshot!("turning_everything_off", view(&p));
    for step in &p.steps {
        let (Step::DeleteEdgeRule { rule_id, .. } | Step::UpdateEdgeRule { rule_id, .. }) = step
        else {
            panic!("{step:?}")
        };
        assert_ne!(rule_id, "theirs", "a foreign rule is never touched");
    }
    // Removed from the end of a phase first, so undoing puts each back in place.
    let positions: Vec<u32> = p
        .steps
        .iter()
        .filter_map(|s| match s {
            Step::DeleteEdgeRule {
                phase, position, ..
            } if phase == cf_api::PHASE_CUSTOM => Some(*position),
            _ => None,
        })
        .collect();
    assert!(positions.windows(2).all(|w| w[0] > w[1]), "{positions:?}");
}

#[test]
fn free_zones_refuse_a_rate_limit_rather_than_going_domain_wide() {
    let before = snapshot(edge(ZonePlan::Free, &[]));
    let with_limit = EdgeProtection {
        rate_limit: Some(limit(30, 10)),
        ..EdgeProtection::default()
    };
    assert_eq!(
        plan(&protect("app.xyz.com", with_limit), &before),
        Err(PlanError::EdgeRateLimitNeedsPro("xyz.com".into()))
    );
}

#[test]
fn hostnames_with_the_same_limit_share_one_rule() {
    let before = snapshot(edge(
        ZonePlan::Pro,
        &[(
            cf_api::PHASE_RATE_LIMIT,
            ours_rate_limit("rl1", limit(30, 60), &["a.xyz.com"]),
        )],
    ));
    let with = |spec| EdgeProtection {
        rate_limit: Some(spec),
        ..EdgeProtection::default()
    };
    let p = assert_idempotent(&protect("b.xyz.com", with(limit(30, 60))), &before);
    insta::assert_yaml_snapshot!("joining_a_shared_rate_limit", view(&p));
    let [Step::UpdateEdgeRule { rule, .. }] = p.steps.as_slice() else {
        panic!("{:?}", p.steps)
    };
    assert_eq!(
        rule.expression,
        r#"(http.host in {"a.xyz.com" "b.xyz.com"})"#
    );

    // Leaving it: the rule stays for the others, or goes with the last hostname.
    let joined = apply(&before, &p);
    let p = assert_idempotent(&protect("a.xyz.com", EdgeProtection::default()), &joined);
    assert!(
        matches!(p.steps.as_slice(), [Step::UpdateEdgeRule { rule, .. }]
        if rule.expression == r#"(http.host in {"b.xyz.com"})"#)
    );
    let p = assert_idempotent(&protect("a.xyz.com", EdgeProtection::default()), &before);
    assert!(matches!(p.steps.as_slice(), [Step::DeleteEdgeRule { .. }]));
}

#[test]
fn a_different_limit_explains_the_conflict_when_the_quota_is_used() {
    // Pro allows two rate limits: one is someone else's, one is Teitunnel's.
    let before = snapshot(edge(
        ZonePlan::Pro,
        &[
            (cf_api::PHASE_RATE_LIMIT, foreign("theirs", "block")),
            (
                cf_api::PHASE_RATE_LIMIT,
                ours_rate_limit("rl1", limit(30, 60), &["a.xyz.com"]),
            ),
        ],
    ));
    let different = EdgeProtection {
        rate_limit: Some(limit(100, 60)),
        ..EdgeProtection::default()
    };
    assert_eq!(
        plan(&protect("b.xyz.com", different.clone()), &before),
        Err(PlanError::EdgeRateLimitConflict {
            zone: "xyz.com".into(),
            hostnames: "a.xyz.com".into(),
            requests: 30,
            period: 60,
        })
    );
    // The only hostname in Teitunnel's rule can change its limit (the rule is reused).
    let p = assert_idempotent(&protect("a.xyz.com", different), &before);
    assert!(
        matches!(
            p.steps.as_slice(),
            [Step::CreateEdgeRule { .. }, Step::DeleteEdgeRule { .. }]
        ),
        "{:?}",
        p.steps
    );
    // Without a rule of Teitunnel's, the quota is simply full.
    let full = snapshot(edge(
        ZonePlan::Pro,
        &[
            (cf_api::PHASE_RATE_LIMIT, foreign("t1", "block")),
            (cf_api::PHASE_RATE_LIMIT, foreign("t2", "block")),
        ],
    ));
    assert!(matches!(
        plan(
            &protect(
                "b.xyz.com",
                EdgeProtection {
                    rate_limit: Some(limit(30, 60)),
                    ..EdgeProtection::default()
                }
            ),
            &full
        ),
        Err(PlanError::EdgeQuotaFull {
            quota: QuotaKind::RateLimit,
            ..
        })
    ));
    // Pro's longest period is a minute.
    assert!(matches!(
        plan(
            &protect(
                "b.xyz.com",
                EdgeProtection {
                    rate_limit: Some(limit(30, 600)),
                    ..EdgeProtection::default()
                }
            ),
            &snapshot(edge(ZonePlan::Pro, &[]))
        ),
        Err(PlanError::EdgeRateLimitPeriod { longest: 60, .. })
    ));
}

#[test]
fn a_full_custom_rule_quota_is_an_error_and_usage_is_shown() {
    let five: Vec<(&str, ObservedRule)> = (0..5)
        .map(|i| (cf_api::PHASE_CUSTOM, foreign(&format!("t{i}"), "block")))
        .collect();
    let bots = EdgeProtection {
        bots: BotMode::Block,
        ..EdgeProtection::default()
    };
    assert_eq!(
        plan(
            &protect("app.xyz.com", bots.clone()),
            &snapshot(edge(ZonePlan::Free, &five))
        ),
        Err(PlanError::EdgeQuotaFull {
            quota: QuotaKind::Custom,
            zone: "xyz.com".into(),
            limit: 5,
        })
    );
    let p = plan(
        &protect("app.xyz.com", bots),
        &snapshot(edge(ZonePlan::Free, &five[..2])),
    )
    .unwrap();
    assert_eq!(
        p.warnings,
        [Warning::EdgeQuota {
            quota: QuotaKind::Custom,
            zone: "xyz.com".into(),
            used: 3,
            limit: 5,
        }]
    );
}

fn tokens_snapshot(app: Option<ObservedAccessApp>, tokens: Vec<ObservedServiceToken>) -> Snapshot {
    Snapshot {
        access: Some(AccessState {
            organization: Some(true),
            login_methods: Some(1),
            apps: app.into_iter().collect(),
        }),
        service_tokens: Some(tokens),
        edge: Vec::new(),
        ..snapshot(edge(ZonePlan::Free, &[]))
    }
}

fn login_app(domain: &str) -> ObservedAccessApp {
    let rule = AccessRule {
        emails: vec!["me@xyz.com".into()],
        email_domains: Vec::new(),
    };
    ObservedAccessApp {
        id: "app1".into(),
        domain: domain.into(),
        owned: true,
        definition: app_definition(domain, &rule),
        rule: Some(rule),
    }
}

fn token(id: &str, owned: bool) -> ObservedServiceToken {
    ObservedServiceToken {
        id: id.into(),
        name: format!("Teitunnel · api.xyz.com · {id}"),
        client_id: format!("{id}.access"),
        expires_at: None,
        owned,
        made_for: owned.then(|| "api.xyz.com".into()),
    }
}

#[test]
fn a_service_token_joins_the_routes_login_or_gets_a_machine_only_one() {
    let create = Intent::CreateServiceToken {
        hostname: host("api.xyz.com"),
        label: "CI".into(),
    };
    let with_login = tokens_snapshot(Some(login_app("api.xyz.com")), Vec::new());
    let p = plan(&create, &with_login).unwrap();
    insta::assert_yaml_snapshot!("token_for_a_route_with_a_login", view(&p));
    assert!(matches!(
        &p.steps[1],
        Step::AllowServiceToken { app: Some((id, _)), .. } if id == "app1"
    ));

    let p = plan(&create, &tokens_snapshot(None, Vec::new())).unwrap();
    assert_eq!(
        p.warnings,
        [Warning::MachineOnly {
            domain: "api.xyz.com".into()
        }]
    );
    let mut theirs = login_app("api.xyz.com");
    theirs.owned = false;
    assert_eq!(
        plan(&create, &tokens_snapshot(Some(theirs), Vec::new())),
        Err(PlanError::AccessAppExists("api.xyz.com".into()))
    );
    let full: Vec<_> = (0..50).map(|i| token(&format!("t{i}"), false)).collect();
    assert_eq!(
        plan(&create, &tokens_snapshot(None, full)),
        Err(PlanError::ServiceTokenLimit(50))
    );
    let blank = Intent::CreateServiceToken {
        hostname: host("api.xyz.com"),
        label: "  ".into(),
    };
    assert_eq!(plan(&blank, &with_login), Err(PlanError::InvalidTokenLabel));
}

#[test]
fn revoking_takes_the_token_out_of_the_login_before_deleting_it() {
    let mut app = login_app("api.xyz.com");
    app.definition = with_service_token(&app.definition, "t1");
    let snapshot = tokens_snapshot(Some(app), vec![token("t1", true), token("t2", false)]);
    let revoke = |id: &str| Intent::RevokeServiceToken {
        hostname: host("api.xyz.com"),
        token_id: id.into(),
    };
    let p = plan(&revoke("t1"), &snapshot).unwrap();
    insta::assert_yaml_snapshot!("revoking_a_token", view(&p));
    let [
        Step::UpdateAccessApp { app, .. },
        Step::DeleteServiceToken { .. },
    ] = p.steps.as_slice()
    else {
        panic!("{:?}", p.steps)
    };
    assert_eq!(app.policies.len(), 1, "only the people's policy is left");
    assert_eq!(
        plan(&revoke("t2"), &snapshot),
        Err(PlanError::ServiceTokenNotOwned(
            "Teitunnel · api.xyz.com · t2".into()
        ))
    );
    assert_eq!(
        plan(&revoke("gone"), &snapshot),
        Err(PlanError::NoSuchServiceToken("gone".into()))
    );
}

#[test]
fn removing_a_login_keeps_service_tokens_passing() {
    use super::types::RouteSpec;
    use crate::domain::RouteOrigin;
    let mut app = login_app("api.xyz.com");
    app.definition = with_service_token(&app.definition, "t1");
    let mut s = tokens_snapshot(Some(app), Vec::new());
    s.tunnel = Some(super::types::ObservedTunnel {
        id: "tun".into(),
        name: "Mac".into(),
        config_version: 1,
        ingress: vec![cf_api::IngressRule {
            hostname: Some("api.xyz.com".into()),
            path: None,
            service: "http://localhost:3000".into(),
            origin_request: serde_json::Map::new(),
            extra: serde_json::Map::new(),
        }],
    });
    let edit = Intent::UpdateRoute {
        hostname: host("api.xyz.com"),
        path: None,
        route: RouteSpec {
            id: "r1".into(),
            hostname: host("api.xyz.com"),
            path: None,
            origin: RouteOrigin::parse("3000").unwrap(),
            options: serde_json::Map::new(),
            access: None,
        },
    };
    let p = plan(&edit, &s).unwrap();
    let updates: Vec<_> = p
        .steps
        .iter()
        .filter_map(|step| match step {
            Step::UpdateAccessApp { app, .. } => Some(app),
            _ => None,
        })
        .collect();
    assert_eq!(updates.len(), 1, "{:?}", p.steps);
    assert_eq!(super::access::service_tokens_of(updates[0]), ["t1"]);
    assert_eq!(
        AccessRule::from_new(updates[0]),
        None,
        "no login for people"
    );
}
