//! Plans an agent was shown and may apply: `plan_change` stores the change with its
//! reviewed plan; `apply_plan` applies exactly that (by id and fingerprint). Plans expire
//! after [`TTL`], and at most [`CAPACITY`] are kept (the oldest go first).

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use teitunnel_core::engine::{Change, PlanView};

use crate::backend::Target;

/// How long a plan stays applicable.
pub const TTL: Duration = Duration::from_secs(30 * 60);
/// How many plans are kept.
pub const CAPACITY: usize = 64;

/// A plan waiting to be applied.
#[derive(Debug, Clone)]
pub struct PendingPlan {
    /// Its id (`plan_…`).
    pub id: String,
    /// Where it applies.
    pub target: Target,
    /// The change.
    pub change: Change,
    /// The plan as shown.
    pub view: PlanView,
    /// One line saying what it does.
    pub summary: String,
    created: Instant,
}

/// The plans of a server (shared by its sessions).
#[derive(Debug, Default)]
pub struct Plans {
    plans: Mutex<HashMap<String, PendingPlan>>,
}

impl Plans {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingPlan>> {
        self.plans.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Stores a plan; returns it with its new id.
    pub fn insert(
        &self,
        target: Target,
        change: Change,
        view: PlanView,
        summary: String,
    ) -> PendingPlan {
        let mut plans = self.lock();
        plans.retain(|_, p| p.created.elapsed() < TTL);
        while plans.len() >= CAPACITY {
            let Some(oldest) = plans
                .values()
                .min_by_key(|p| p.created)
                .map(|p| p.id.clone())
            else {
                break;
            };
            plans.remove(&oldest);
        }
        let id = format!("plan_{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);
        let plan = PendingPlan {
            id: id.clone(),
            target,
            change,
            view,
            summary,
            created: Instant::now(),
        };
        plans.insert(id, plan.clone());
        plan
    }

    /// The plan with `id`, if it's still applicable.
    pub fn get(&self, id: &str) -> Option<PendingPlan> {
        let plans = self.lock();
        plans
            .get(id.trim())
            .filter(|p| p.created.elapsed() < TTL)
            .cloned()
    }

    /// Replaces a plan's view after Cloudflare changed (the new one needs a new review).
    pub fn replace_view(&self, id: &str, view: PlanView) {
        if let Some(plan) = self.lock().get_mut(id) {
            plan.view = view;
            plan.created = Instant::now();
        }
    }

    /// Forgets a plan (applied, or declined).
    pub fn remove(&self, id: &str) {
        self.lock().remove(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(fingerprint: &str) -> PlanView {
        PlanView {
            steps: Vec::new(),
            warnings: Vec::new(),
            requires_confirmation: false,
            fingerprint: fingerprint.into(),
        }
    }

    fn target() -> Target {
        Target {
            account: "acc".into(),
            tunnel: None,
        }
    }

    #[test]
    fn keeps_plans_until_applied_and_bounds_them() {
        let plans = Plans::default();
        let plan = plans.insert(target(), Change::RemoveTunnel, view("a"), "Remove".into());
        assert!(plan.id.starts_with("plan_"));
        assert_eq!(plans.get(&plan.id).unwrap().view.fingerprint, "a");
        plans.replace_view(&plan.id, view("b"));
        assert_eq!(plans.get(&plan.id).unwrap().view.fingerprint, "b");
        plans.remove(&plan.id);
        assert!(plans.get(&plan.id).is_none());

        let first = plans.insert(target(), Change::RemoveTunnel, view("x"), String::new());
        for _ in 0..CAPACITY {
            plans.insert(target(), Change::RemoveTunnel, view("y"), String::new());
        }
        assert!(plans.get(&first.id).is_none(), "the oldest made room");
        assert_eq!(plans.lock().len(), CAPACITY);
    }
}
