//! `teitunnel inspect <hostname>`: points one of this machine's routes at an inspector
//! run by this command (a plan to review, like any change), prints its requests, and
//! points it back when the command ends. `--off` ends an inspection left behind.

use std::process::ExitCode;

use teitunnel_core::{
    engine::{Approval, Outcome, PlanView},
    inspect::{Inspector, routes},
    runtime,
};

use crate::{context::App, share::status};

fn print_plan(plan: &PlanView) -> Result<(), String> {
    for (index, step) in plan.steps.iter().enumerate() {
        out!("{:>2}. {}", index + 1, step.description)?;
    }
    Ok(())
}

fn outcome(outcome: &Outcome) -> Result<(), String> {
    match outcome {
        Outcome::Applied { .. } => Ok(()),
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
            Err(error.english())
        }
    }
}

/// Every 30 s, cleans up after processes that ended without doing so (for servers
/// without the app, which does the same): points routes back at their own service when
/// the `teitunnel inspect` that pointed them at its inspector has exited, and removes
/// shares on a domain that expired or whose `share`/`mcp` process was killed.
pub(crate) fn sweep_left_behind(
    app: &App,
    machine: teitunnel_core::machine::MachineTunnels,
) -> tokio::task::JoinHandle<()> {
    let (accounts, engine, name) = (
        app.accounts.clone(),
        std::sync::Arc::clone(&app.engine),
        app.machine_name.clone(),
    );
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            tick.tick().await;
            for failure in routes::sweep(&accounts, &engine, &machine, &name, None, |route| {
                route.is_over()
            })
            .await
            {
                status(&format!(
                    "Couldn't end an inspection left behind: {}",
                    failure.english()
                ));
            }
            let now = teitunnel_core::domain_shares::now_ms();
            for failure in
                teitunnel_core::domain_shares::sweep(&accounts, &engine, &machine, &name, |share| {
                    share.is_over(now)
                })
                .await
            {
                status(&format!(
                    "Couldn't remove a share left behind: {}",
                    failure.english()
                ));
            }
        }
    })
}

/// Runs `teitunnel inspect`.
pub(crate) async fn run(
    app: &App,
    hostname: &str,
    path: Option<&str>,
    account: Option<&str>,
    off: bool,
    yes: bool,
) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let connectors = app.connectors(&account).await;
    let ctx = app.context(&account);
    if off {
        let Some(plan) = routes::plan_off(&app.engine, &api, &connectors, ctx, hostname, path)
            .await
            .map_err(|e| e.to_string())?
        else {
            out!("{hostname} already points at its own service.")?;
            return Ok(ExitCode::SUCCESS);
        };
        print_plan(&plan.plan)?;
        if !yes && !crate::confirm("Point it back at its own service?")? {
            out!("Nothing changed.")?;
            return Ok(ExitCode::SUCCESS);
        }
        let done = routes::apply_off(
            &app.engine,
            &api,
            &connectors,
            ctx,
            None,
            hostname,
            path,
            Approval {
                fingerprint: &plan.plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .map_err(|e| e.to_string())?;
        if let Some(done) = &done {
            outcome(done)?;
        }
        out!("{hostname} points at its own service again.")?;
        return Ok(ExitCode::SUCCESS);
    }

    let inspector = Inspector::new(
        Some(app.store().clone()),
        Some(app.secrets().clone()),
        &runtime::this_process(),
    );
    inspector.load().await.map_err(|e| e.to_string())?;
    let plan = routes::plan_on(
        &app.engine,
        &api,
        &connectors,
        ctx,
        &inspector,
        hostname,
        path,
    )
    .await
    .map_err(|e| e.to_string())?;
    print_plan(&plan.plan)?;
    status(
        "While this command runs, the route goes through an inspector on this machine; it points back at its own service when the command ends.",
    );
    if !yes && !crate::confirm("Inspect it?")? {
        inspector.shutdown().await;
        out!("Nothing changed.")?;
        return Ok(ExitCode::SUCCESS);
    }
    let applied = routes::apply_on(
        &app.engine,
        &api,
        &connectors,
        ctx,
        &inspector,
        hostname,
        path,
        Approval {
            fingerprint: &plan.plan.fingerprint,
            confirmed: false,
        },
        |_| {},
    )
    .await
    .map_err(|e| e.to_string());
    let applied = applied.and_then(|o| outcome(&o));
    if let Err(err) = applied {
        inspector.shutdown().await;
        return Err(err);
    }
    let printer = crate::traffic::print_requests(&inspector);
    status(&format!(
        "Inspecting https://{hostname}. Requests appear below (and in `teitunnel traffic`). Press Ctrl-C to stop."
    ));
    crate::share::interrupted().await;
    if let Some(printer) = printer {
        printer.abort();
    }
    let remembered = routes::list(app.store(), Some(&account.id))
        .await
        .map_err(|e| e.to_string())?;
    let mut result = Ok(ExitCode::SUCCESS);
    for route in remembered.iter().filter(|r| {
        r.owner == inspector.owner()
            && r.hostname.eq_ignore_ascii_case(hostname)
            && r.path.as_deref() == path
    }) {
        match routes::revert(
            &app.engine,
            &api,
            &connectors,
            &app.machine_name,
            Some(&inspector),
            route,
        )
        .await
        {
            Ok(()) => status(&format!("{hostname} points at its own service again.")),
            Err(message) => {
                result = Err(format!(
                    "Couldn't point {hostname} back at its own service: {}. Run `teitunnel inspect {hostname} --off`, or the app does it the next time it runs.",
                    message.english()
                ));
            }
        }
    }
    inspector.shutdown().await;
    result
}
