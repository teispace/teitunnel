//! Planner scenarios (insta snapshots) and the idempotency property.

use cf_api::{DnsRecord, IngressRule};
use serde_json::Map;

use super::{
    planner::{PlanError, plan, tunnel_record},
    simulate::apply,
    types::{Intent, ObservedRecord, ObservedTunnel, Plan, RouteSpec, Snapshot, Step, ZoneRef},
};
use crate::domain::{Hostname, PathRule, RouteOrigin};

const TUNNEL: &str = "6ff42ae2-765d-4adf-8112-31c55c1551ef";

fn host(h: &str) -> Hostname {
    Hostname::parse(h).unwrap()
}

fn route(id: &str, hostname: &str, origin: &str) -> RouteSpec {
    RouteSpec {
        id: id.into(),
        hostname: host(hostname),
        path: None,
        origin: RouteOrigin::parse(origin).unwrap(),
        options: Map::new(),
    }
}

fn rule(hostname: &str, service: &str) -> IngressRule {
    IngressRule {
        hostname: Some(hostname.into()),
        path: None,
        service: service.into(),
        origin_request: Map::new(),
        extra: Map::new(),
    }
}

fn catch_all() -> IngressRule {
    IngressRule {
        hostname: None,
        path: None,
        service: "http_status:404".into(),
        origin_request: Map::new(),
        extra: Map::new(),
    }
}

fn owned(zone: &str, id: &str, name: &str) -> ObservedRecord {
    ObservedRecord {
        zone_id: zone.into(),
        record: tunnel_record(id, name, TUNNEL),
        owned: true,
    }
}

fn foreign(zone: &str, id: &str, name: &str, kind: &str, content: &str) -> ObservedRecord {
    ObservedRecord {
        zone_id: zone.into(),
        record: DnsRecord {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            content: content.into(),
            proxied: false,
            comment: None,
            ttl: 1,
        },
        owned: false,
    }
}

/// A fresh account: two zones, no tunnel, no records.
fn fresh() -> Snapshot {
    Snapshot {
        account_id: "acc".into(),
        machine_name: "Krishna's MacBook Pro".into(),
        zones: vec![
            ZoneRef {
                id: "z-xyz".into(),
                name: "xyz.com".into(),
            },
            ZoneRef {
                id: "z-yx".into(),
                name: "yx.com".into(),
            },
        ],
        tunnel: None,
        tunnel_names: Vec::new(),
        records: Vec::new(),
    }
}

/// One route (app.xyz.com → 3000) already applied.
fn with_app() -> Snapshot {
    Snapshot {
        tunnel: Some(ObservedTunnel {
            id: TUNNEL.into(),
            name: "Krishna's MacBook Pro".into(),
            config_version: 3,
            ingress: vec![rule("app.xyz.com", "http://localhost:3000"), catch_all()],
        }),
        records: vec![owned("z-xyz", "r-app", "app.xyz.com")],
        ..fresh()
    }
}

/// Snapshot tests redact the fingerprint (it changes with any fixture edit).
fn snap(plan: &Plan) -> Plan {
    Plan {
        fingerprint: "<fingerprint>".into(),
        ..plan.clone()
    }
}

#[test]
fn a_new_tunnel_gets_a_name_no_other_tunnel_has() {
    let snapshot = Snapshot {
        tunnel_names: vec![
            "Krishna's MacBook Pro".into(),
            "krishna's macbook pro 2".into(),
        ],
        ..fresh()
    };
    let p = plan(
        &Intent::AddRoute {
            route: route("r1", "xyz.com", "3000"),
        },
        &snapshot,
    )
    .unwrap();
    assert!(
        matches!(&p.steps[0], Step::CreateTunnel { name } if name == "Krishna's MacBook Pro 3"),
        "{:?}",
        p.steps[0]
    );
}

#[test]
fn first_route_on_a_fresh_account() {
    let p = plan(
        &Intent::AddRoute {
            route: route("r1", "xyz.com", "3000"),
        },
        &fresh(),
    )
    .unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn second_route_in_another_zone() {
    let p = plan(
        &Intent::AddRoute {
            route: route("r2", "yx.com", "5000"),
        },
        &with_app(),
    )
    .unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn wildcard_route_sorts_after_exact_names() {
    let p = plan(
        &Intent::AddRoute {
            route: route("r3", "*.xyz.com", "8080"),
        },
        &with_app(),
    )
    .unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn path_route_on_an_existing_hostname_needs_no_dns() {
    let mut r = route("r4", "app.xyz.com", "8080");
    r.path = Some(PathRule::parse("^/api/").unwrap());
    let p = plan(&Intent::AddRoute { route: r }, &with_app()).unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn conflicting_a_record_requires_confirmation() {
    let mut s = with_app();
    s.records.push(foreign(
        "z-xyz",
        "r-www-a",
        "www.xyz.com",
        "A",
        "203.0.113.4",
    ));
    s.records.push(foreign(
        "z-xyz",
        "r-www-aaaa",
        "www.xyz.com",
        "AAAA",
        "2001:db8::4",
    ));
    let p = plan(
        &Intent::AddRoute {
            route: route("r5", "www.xyz.com", "3001"),
        },
        &s,
    )
    .unwrap();
    assert!(p.requires_confirmation);
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn owned_cname_to_an_old_tunnel_is_repointed_without_confirmation() {
    let mut s = with_app();
    s.records.push(ObservedRecord {
        zone_id: "z-xyz".into(),
        record: tunnel_record("r-old", "old.xyz.com", "dead-tunnel"),
        owned: true,
    });
    let p = plan(
        &Intent::AddRoute {
            route: route("r6", "old.xyz.com", "4000"),
        },
        &s,
    )
    .unwrap();
    assert!(!p.requires_confirmation);
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn hostname_outside_the_account_is_refused() {
    let err = plan(
        &Intent::AddRoute {
            route: route("r7", "other.org", "3000"),
        },
        &fresh(),
    )
    .unwrap_err();
    assert_eq!(err, PlanError::NoZone("other.org".into()));
}

#[test]
fn adding_an_existing_route_is_refused_or_a_no_op() {
    let err = plan(
        &Intent::AddRoute {
            route: route("r8", "app.xyz.com", "9999"),
        },
        &with_app(),
    )
    .unwrap_err();
    assert!(matches!(err, PlanError::RouteExists(_)));
    let p = plan(
        &Intent::AddRoute {
            route: route("r8", "app.xyz.com", "3000"),
        },
        &with_app(),
    )
    .unwrap();
    assert!(p.is_empty(), "same route again is nothing to do: {p:?}");
}

#[test]
fn change_origin_only_updates_config() {
    let intent = Intent::UpdateRoute {
        hostname: host("app.xyz.com"),
        path: None,
        route: route("r1", "app.xyz.com", "3001"),
    };
    let p = plan(&intent, &with_app()).unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn rename_creates_new_dns_and_removes_the_old_owned_record() {
    let intent = Intent::UpdateRoute {
        hostname: host("app.xyz.com"),
        path: None,
        route: route("r1", "web.xyz.com", "3000"),
    };
    let p = plan(&intent, &with_app()).unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn remove_last_route_deletes_owned_dns_and_warns_tunnel_empty() {
    let p = plan(
        &Intent::RemoveRoute {
            hostname: host("app.xyz.com"),
            path: None,
        },
        &with_app(),
    )
    .unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn remove_never_deletes_foreign_records() {
    let mut s = with_app();
    s.records = vec![ObservedRecord {
        zone_id: "z-xyz".into(),
        record: tunnel_record("r-app", "app.xyz.com", TUNNEL),
        owned: false,
    }];
    let p = plan(
        &Intent::RemoveRoute {
            hostname: host("app.xyz.com"),
            path: None,
        },
        &s,
    )
    .unwrap();
    assert!(
        !p.steps
            .iter()
            .any(|s| matches!(s, super::types::Step::DeleteRecord { .. }))
    );
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn remove_tunnel_cascades() {
    let mut s = with_app();
    if let Some(t) = s.tunnel.as_mut() {
        t.ingress.insert(1, rule("yx.com", "http://localhost:5000"));
    }
    s.records.push(owned("z-yx", "r-yx", "yx.com"));
    let p = plan(&Intent::RemoveTunnel, &s).unwrap();
    insta::assert_yaml_snapshot!(snap(&p));
}

#[test]
fn removing_without_a_tunnel_is_an_error() {
    assert_eq!(
        plan(&Intent::RemoveTunnel, &fresh()),
        Err(PlanError::NoTunnel)
    );
}

#[test]
fn remote_origins_are_flagged() {
    let p = plan(
        &Intent::AddRoute {
            route: route("r9", "nas.xyz.com", "http://192.168.1.20:5000"),
        },
        &with_app(),
    )
    .unwrap();
    assert!(
        p.warnings
            .iter()
            .any(|w| matches!(w, super::types::Warning::RemoteOrigin { .. }))
    );
}

#[test]
fn fingerprint_changes_with_the_snapshot() {
    let a = with_app();
    let mut b = with_app();
    if let Some(t) = b.tunnel.as_mut() {
        t.config_version = 4;
    }
    assert_ne!(a.fingerprint(), b.fingerprint());
    assert_eq!(a.fingerprint(), with_app().fingerprint());
}

mod property {
    use proptest::prelude::*;

    use super::*;

    fn arb_intent() -> impl Strategy<Value = Intent> {
        let names = prop_oneof![
            Just("a.xyz.com"),
            Just("b.xyz.com"),
            Just("yx.com"),
            Just("*.yx.com")
        ];
        let ports = prop_oneof![Just("3000"), Just("5000"), Just("8080")];
        prop_oneof![
            (names.clone(), ports.clone()).prop_map(|(h, p)| Intent::AddRoute {
                route: route("rid", h, p)
            }),
            (names.clone(), names.clone(), ports).prop_map(|(from, to, p)| Intent::UpdateRoute {
                hostname: host(from),
                path: None,
                route: route("rid", to, p)
            }),
            names.prop_map(|h| Intent::RemoveRoute {
                hostname: host(h),
                path: None
            }),
            Just(Intent::RemoveTunnel),
        ]
    }

    proptest! {
        /// After applying a plan, planning the same intent again changes nothing.
        #[test]
        fn plans_converge(intents in proptest::collection::vec(arb_intent(), 1..8)) {
            let mut state = fresh();
            for intent in &intents {
                let Ok(first) = plan(intent, &state) else { continue };
                state = apply(&state, &first);
                if let Ok(again) = plan(intent, &state) {
                    prop_assert!(again.is_empty(), "not idempotent for {intent:?}: {again:?}");
                }
                // Invariants: exactly one catch-all, last; no two rules for one hostname+path.
                if let Some(tunnel) = &state.tunnel {
                    prop_assert_eq!(tunnel.ingress.iter().filter(|r| r.hostname.is_none()).count(), usize::from(!tunnel.ingress.is_empty()));
                    if let Some(last) = tunnel.ingress.last() {
                        prop_assert!(last.hostname.is_none());
                    }
                    let mut seen = std::collections::HashSet::new();
                    for r in &tunnel.ingress {
                        prop_assert!(seen.insert((r.hostname.clone(), r.path.clone())));
                    }
                }
            }
        }
    }
}

fn with_path(mut r: RouteSpec, path: &str) -> RouteSpec {
    r.path = Some(PathRule::parse(path).unwrap());
    r
}

fn remove(hostname: &str, path: Option<&str>) -> Intent {
    Intent::RemoveRoute {
        hostname: host(hostname),
        path: path.map(|p| PathRule::parse(p).unwrap()),
    }
}

fn update(hostname: &str, path: Option<&str>, route: RouteSpec) -> Intent {
    Intent::UpdateRoute {
        hostname: host(hostname),
        path: path.map(|p| PathRule::parse(p).unwrap()),
        route,
    }
}

/// Two routes on app.xyz.com (plain and ^/api/) and one on yx.com.
fn busy() -> Snapshot {
    let mut s = with_app();
    let tunnel = s.tunnel.as_mut().unwrap();
    let mut api = rule("app.xyz.com", "http://localhost:8080");
    api.path = Some("^/api/".into());
    tunnel.ingress = vec![
        api,
        rule("app.xyz.com", "http://localhost:3000"),
        rule("yx.com", "http://localhost:5000"),
        catch_all(),
    ];
    s.records.push(owned("z-yx", "r-yx", "yx.com"));
    s
}

/// More scenarios, one named snapshot each (reviewed like the ones above).
#[test]
fn scenarios() {
    let mut options = route("o1", "opts.xyz.com", "https://localhost:8443");
    options
        .options
        .insert("noTLSVerify".into(), serde_json::json!(true));

    let mut foreign_cname = with_app();
    foreign_cname
        .records
        .push(foreign("z-yx", "f1", "yx.com", "CNAME", "shop.example.net"));

    let mut unproxied = with_app();
    let mut record = tunnel_record("r-np", "np.xyz.com", TUNNEL);
    record.proxied = false;
    unproxied.records.push(ObservedRecord {
        zone_id: "z-xyz".into(),
        record,
        owned: false,
    });

    let mut already = with_app();
    already.records.push(ObservedRecord {
        zone_id: "z-xyz".into(),
        record: tunnel_record("r-hand", "hand.xyz.com", TUNNEL),
        owned: false,
    });

    let mut foreign_target = busy();
    foreign_target
        .records
        .push(foreign("z-xyz", "f2", "blog.xyz.com", "A", "198.51.100.7"));

    let mut unowned_tunnel_record = with_app();
    unowned_tunnel_record.records[0].owned = false;

    let mut empty_tunnel = with_app();
    empty_tunnel.tunnel.as_mut().unwrap().ingress = vec![catch_all()];
    empty_tunnel.records.clear();

    let mut edited = busy();
    edited
        .tunnel
        .as_mut()
        .unwrap()
        .ingress
        .insert(0, rule("dash.xyz.com", "http://localhost:9000"));
    edited.tunnel.as_mut().unwrap().config_version = 9;

    let taken = Snapshot {
        tunnel_names: vec!["Krishna's MacBook Pro".into()],
        ..fresh()
    };

    let mut nested = with_app();
    nested.zones.push(ZoneRef {
        id: "z-dev".into(),
        name: "dev.xyz.com".into(),
    });

    let cases: Vec<(&str, Snapshot, Intent)> = vec![
        (
            "add_second_route_in_the_same_zone",
            with_app(),
            Intent::AddRoute {
                route: route("a1", "api.xyz.com", "8080"),
            },
        ),
        (
            "add_with_origin_options",
            with_app(),
            Intent::AddRoute { route: options },
        ),
        (
            "add_remote_origin_warns",
            with_app(),
            Intent::AddRoute {
                route: route("a2", "nas.xyz.com", "http://192.168.1.10:5000"),
            },
        ),
        (
            "add_tcp_origin",
            with_app(),
            Intent::AddRoute {
                route: route("a3", "db.xyz.com", "tcp://localhost:5432"),
            },
        ),
        (
            "add_unix_socket_origin",
            with_app(),
            Intent::AddRoute {
                route: route("a4", "sock.xyz.com", "unix:/tmp/app.sock"),
            },
        ),
        (
            "add_over_a_foreign_cname_needs_confirmation",
            foreign_cname,
            Intent::AddRoute {
                route: route("a5", "yx.com", "5000"),
            },
        ),
        (
            "add_repoints_an_unproxied_tunnel_record",
            unproxied,
            Intent::AddRoute {
                route: route("a6", "np.xyz.com", "7000"),
            },
        ),
        (
            "add_reuses_a_record_that_already_points_here",
            already,
            Intent::AddRoute {
                route: route("a7", "hand.xyz.com", "7001"),
            },
        ),
        (
            "add_longer_path_sorts_first",
            busy(),
            Intent::AddRoute {
                route: with_path(route("a8", "app.xyz.com", "8081"), "^/api/v2/"),
            },
        ),
        (
            "add_first_route_when_the_name_is_taken",
            taken,
            Intent::AddRoute {
                route: route("a9", "xyz.com", "3000"),
            },
        ),
        (
            "add_uses_the_most_specific_zone",
            nested,
            Intent::AddRoute {
                route: route("a10", "api.dev.xyz.com", "3000"),
            },
        ),
        (
            "update_origin_only",
            busy(),
            update("yx.com", None, route("u1", "yx.com", "5001")),
        ),
        (
            "update_path_only",
            busy(),
            update(
                "app.xyz.com",
                Some("^/api/"),
                with_path(route("u2", "app.xyz.com", "8080"), "^/v1/"),
            ),
        ),
        (
            "rename_into_another_zone",
            busy(),
            update("yx.com", None, route("u3", "shop.xyz.com", "5000")),
        ),
        (
            "rename_keeps_dns_still_used_by_a_path_route",
            busy(),
            update("app.xyz.com", None, route("u4", "web.xyz.com", "3000")),
        ),
        (
            "rename_onto_a_foreign_record_needs_confirmation",
            foreign_target,
            update("yx.com", None, route("u5", "blog.xyz.com", "5000")),
        ),
        (
            "remove_path_route_keeps_the_hostname",
            busy(),
            remove("app.xyz.com", Some("^/api/")),
        ),
        (
            "remove_keeps_a_record_teitunnel_did_not_create",
            unowned_tunnel_record,
            remove("app.xyz.com", None),
        ),
        (
            "remove_tunnel_without_routes",
            empty_tunnel,
            Intent::RemoveTunnel,
        ),
        (
            "restore_undoes_an_outside_edit",
            edited,
            Intent::RestoreConfig {
                ingress: busy().tunnel.unwrap().ingress,
            },
        ),
    ];
    for (name, snapshot, intent) in cases {
        let p = plan(&intent, &snapshot).unwrap_or_else(|e| panic!("{name}: {e}"));
        insta::assert_yaml_snapshot!(name, snap(&p));
    }
}
