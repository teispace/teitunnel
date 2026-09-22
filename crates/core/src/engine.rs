//! The change engine: observe → plan → apply → verify (ARCHITECTURE §4).
//!
//! Every Cloudflare mutation goes through this pipeline. The planner is pure: given an
//! intent and an observed snapshot it returns an ordered plan the user can review. The
//! executor applies a reviewed plan with a staleness guard, logs every step and
//! compensates completed steps on failure.

mod cloud;
mod executor;
mod ingress;
mod local;
mod observe;
mod planner;
mod types;
mod verify;

#[cfg(test)]
mod executor_tests;
#[cfg(test)]
mod fake;
#[cfg(test)]
mod planner_tests;
#[cfg(test)]
mod simulate;

pub use cloud::{CloudApi, Connectors};
pub use executor::{Approval, Context, Engine, EngineError, Outcome, Progress, StepState};
pub use ingress::{CATCH_ALL, sort_ingress};
pub use local::{ActivityEntry, Local, LocalTunnel};
pub use observe::{ObserveError, observe};
pub use planner::{PlanError, plan};
pub use types::{
    Intent, ObservedRecord, ObservedTunnel, Plan, RouteSpec, Snapshot, Step, TunnelRef, Warning,
    ZoneRef, ownership_comment, tunnel_target,
};
pub use verify::{Edge, Failure, Stage, Verification, classify};
