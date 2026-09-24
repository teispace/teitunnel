//! The change engine: observe → plan → apply → verify (ARCHITECTURE §4).
//!
//! Every Cloudflare mutation goes through this pipeline. The planner is pure: given an
//! intent and an observed snapshot it returns an ordered plan the user can review. The
//! executor applies a reviewed plan with a staleness guard, logs every step and
//! compensates completed steps on failure.

mod access;
mod activity;
pub mod balance;
mod cloud;
mod drift;
mod executor;
mod ingress;
mod local;
mod networks;
mod observe;
mod planner;
mod tunnels;
mod types;
mod verify;
mod views;

#[cfg(test)]
mod executor_tests;
#[cfg(test)]
pub(crate) mod fake;
#[cfg(test)]
mod planner_tests;
#[cfg(test)]
mod simulate;

pub use access::{
    AccessDomainError, AccessNeed, AccessRule, AccessRuleError, AccessState, ObservedAccessApp,
    access_domain, app_definition,
};
pub use activity::{
    ActivityKind, ActivityRecord, Actor, Delta, DeltaArea, RecordedStep, current_actor, deltas,
    with_actor,
};
pub use cloud::{CloudApi, Connectors};
pub use drift::{Drift, RuleChange, diff};
pub use executor::{Approval, Context, Engine, EngineError, Outcome, Progress, StepState};
pub use ingress::{CATCH_ALL, sort_ingress};
pub use local::{ActivityEntry, Local, LocalTunnel};
pub use networks::{NETWORK_COMMENT, NetworkState, ObservedNetworkRoute};
pub use observe::{ObserveError, ObserveNeed, Want, observe};
pub use planner::{PlanError, plan};
pub use tunnels::{ConnectionView, ConnectorView, TunnelSummary};
pub use types::{
    Intent, ObservedRecord, ObservedTunnel, Plan, RouteSpec, Snapshot, Step, TunnelRef, Warning,
    ZoneRef, ownership_comment, tunnel_target,
};
pub use verify::{Edge, Failure, Stage, Verification, classify};
pub use views::{
    Change, DnsState, InputError, NetworkView, PlanView, RouteHealth, RouteInput, RouteView,
    RoutesOverview, StepKind, StepView, TunnelView, route_id,
};
