//! Reservations for teams sharing one account (M12-11), as a [`ToolProvider`]:
//! `list_reservations`, `reserve_hostname` and `release_hostname`. Changes go through
//! the same plan → apply engine as routes, with the person's approval asked in the same
//! way; a name someone else holds is taken only when the person confirms.

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::engine::{Change, Outcome, ownership::format_until};

use crate::{
    backend::{ApplyApproval, BackendError, BoxFuture, SharedBackend, Target},
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolProvider,
        ToolResult, ToolSpec, arguments,
    },
    tools::{DEFAULT_TIMEOUT, Hints, account, account_for_hostname, plan_text, spec},
};

/// The reservation tools.
pub struct ReservationTools {
    backend: SharedBackend,
}

impl std::fmt::Debug for ReservationTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReservationTools").finish_non_exhaustive()
    }
}

impl ReservationTools {
    /// The tools over `backend`.
    pub fn new(backend: SharedBackend) -> Self {
        Self { backend }
    }
}

/// Which account.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ListArgs {
    /// Account name or id (needed when several are connected).
    #[serde(default)]
    account: Option<String>,
}

/// A hostname to reserve.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReserveArgs {
    /// The hostname, e.g. `alice.dev.example.com`.
    hostname: String,
    /// When the reservation ends: `2026-12-31` (end of that day, UTC) or
    /// `2026-12-31T18:00Z`. Leave out for no end date.
    #[serde(default)]
    until: Option<String>,
    /// Account name or id (default: the one whose domains include the hostname).
    #[serde(default)]
    account: Option<String>,
    /// The person reviewed the plan and agreed (needed when this server can't ask them,
    /// and to take a name someone else holds).
    #[serde(default)]
    confirmed: bool,
}

/// A hostname to release.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ReleaseArgs {
    /// The hostname.
    hostname: String,
    /// Account name or id.
    #[serde(default)]
    account: Option<String>,
    /// The person reviewed the plan and agreed (needed when this server can't ask them,
    /// and to release someone else's reservation).
    #[serde(default)]
    confirmed: bool,
}

/// One reserved hostname.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ReservationOut {
    hostname: String,
    /// Who holds it (`person@machine`), when known.
    owner: Option<String>,
    /// Held by whoever runs this server.
    mine: bool,
    /// When it ends (UTC), or none.
    until: Option<String>,
    /// It has ended: the name is free.
    ended: bool,
    /// The holder routes it too.
    routed: bool,
}

/// The account's reservations.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ListOut {
    reservations: Vec<ReservationOut>,
    /// Cloudflare couldn't be reached: these were seen last.
    cached: bool,
}

/// What a reserve or release did.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
struct ChangeOut {
    /// `applied`, `nothingToDo`, `needsApproval`, `declined` or `failed`.
    outcome: String,
    /// What happened, for the person.
    message: String,
    /// The plan, as the person should read it.
    plan: String,
}

impl ChangeOut {
    fn new(outcome: &str, message: impl Into<String>, plan: &str) -> Self {
        Self {
            outcome: outcome.into(),
            message: message.into(),
            plan: plan.into(),
        }
    }
}

const WRITE: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: true,
    open_world: true,
};

fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<ListArgs, ListOut>(
            "list_reservations",
            "List reserved hostnames",
            "Lists the hostnames reserved in a Cloudflare account and who holds each (person@machine), with end dates. Teammates sharing one account reserve names so nobody takes them; use this before picking a hostname for a route or share. Example: {} or {\"account\": \"Acme\"}.",
            ToolClass::Read,
            Hints::READ_CLOUD,
            DEFAULT_TIMEOUT,
        ),
        spec::<ReserveArgs, ChangeOut>(
            "reserve_hostname",
            "Reserve a hostname",
            "Reserves a hostname for the person running Teitunnel, optionally until a date, so teammates sharing the account see it's taken (a placeholder DNS record that serves nothing). Reserving it again changes the end date; a route added there later keeps the reservation. Shows the plan and asks the person before applying. A name someone else holds is taken only if the person confirms. Example: {\"hostname\": \"alice.dev.example.com\", \"until\": \"2026-12-31\"}.",
            ToolClass::Change,
            WRITE,
            DEFAULT_TIMEOUT,
        ),
        spec::<ReleaseArgs, ChangeOut>(
            "release_hostname",
            "Release a reserved hostname",
            "Gives up a hostname's reservation (a route there stays). Asks the person before applying; someone else's reservation is released only if the person confirms. Example: {\"hostname\": \"alice.dev.example.com\"}.",
            ToolClass::Destructive,
            Hints {
                destructive: true,
                ..WRITE
            },
            DEFAULT_TIMEOUT,
        ),
    ]
}

impl ToolProvider for ReservationTools {
    fn tools(&self) -> Vec<ToolSpec> {
        specs()
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: JsonObject,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            match name {
                "list_reservations" => list(&self.backend, arguments).await,
                "reserve_hostname" => {
                    let args: ReserveArgs = self::arguments(arguments)?;
                    let summary = match &args.until {
                        Some(until) => format!("Reserve {} until {until}", args.hostname),
                        None => format!("Reserve {}", args.hostname),
                    };
                    let change = Change::ReserveHostname {
                        hostname: args.hostname.clone(),
                        until: args.until,
                    };
                    change_one(
                        &self.backend,
                        ctx,
                        args.account.as_deref(),
                        &args.hostname,
                        change,
                        summary,
                        args.confirmed,
                    )
                    .await
                }
                "release_hostname" => {
                    let args: ReleaseArgs = self::arguments(arguments)?;
                    let change = Change::ReleaseHostname {
                        hostname: args.hostname.clone(),
                    };
                    let summary = format!("Release the reservation of {}", args.hostname);
                    change_one(
                        &self.backend,
                        ctx,
                        args.account.as_deref(),
                        &args.hostname,
                        change,
                        summary,
                        args.confirmed,
                    )
                    .await
                }
                other => Err(ToolError::new(format!("Unknown tool {other}."))),
            }
        })
    }
}

async fn list(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: ListArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let listed = backend.reservations(&account.id).await?;
    let count = listed.items.len();
    Ok(ToolOutput::new(&ListOut {
        cached: listed.cached,
        reservations: listed
            .items
            .into_iter()
            .map(|r| ReservationOut {
                until: r.until.map(format_until),
                hostname: r.hostname,
                owner: r.owner,
                mine: r.mine,
                ended: r.ended,
                routed: r.routed,
            })
            .collect(),
    })
    .with_summary(format!("{count} reserved hostname(s).")))
}

/// Plans `change`, asks the person, applies it.
async fn change_one(
    backend: &SharedBackend,
    ctx: &ToolContext,
    wanted: Option<&str>,
    hostname: &str,
    change: Change,
    summary: String,
    confirmed: bool,
) -> ToolResult {
    let account = account_for_hostname(backend, wanted, hostname).await?;
    let target = Target {
        account: account.id.clone(),
        tunnel: None,
    };
    let view = backend.preview(&target, &change).await?;
    let text = plan_text(&view);
    if view.steps.is_empty() {
        return Ok(ToolOutput::new(&ChangeOut::new(
            "nothingToDo",
            "Nothing to change: it's already like that.",
            &text,
        )));
    }
    let approval = ctx
        .approve(&ApprovalRequest {
            title: summary,
            details: text.clone(),
            confirmed,
        })
        .await;
    let by_person = match approval {
        Approval::Granted { how } => how == "person",
        Approval::NeedsConfirmation => {
            return Ok(ToolOutput::new(&ChangeOut::new(
                "needsApproval",
                "Nothing was applied. Show the person the plan and call again with \"confirmed\": true only if they agree.",
                &text,
            )));
        }
        Approval::Declined(why) => {
            return Ok(ToolOutput::new(&ChangeOut::new(
                "declined",
                format!("{why} Nothing was applied."),
                &text,
            )));
        }
    };
    if view.requires_confirmation && !(by_person || confirmed) {
        return Ok(ToolOutput::new(&ChangeOut::new(
            "needsApproval",
            "Nothing was applied. Someone else holds this hostname (see the plan): call again with \"confirmed\": true only if the person agrees to take it.",
            &text,
        )));
    }
    let outcome = backend
        .apply(
            &target,
            &change,
            ApplyApproval {
                fingerprint: view.fingerprint.clone(),
                confirmed: view.requires_confirmation,
            },
            Some(ctx.actor().clone()),
            Box::new(|_| {}),
        )
        .await;
    let out = match outcome {
        Ok(Outcome::Applied { .. }) => ChangeOut::new("applied", "Done.", &text),
        Ok(Outcome::RolledBack { error, .. }) => ChangeOut::new(
            "failed",
            format!("{} Everything was undone.", error.english()),
            &text,
        ),
        Ok(Outcome::PartiallyApplied {
            error, leftovers, ..
        }) => ChangeOut::new(
            "failed",
            format!(
                "{} Left over: {}",
                error.english(),
                leftovers
                    .iter()
                    .map(teitunnel_core::text::Text::english)
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
            &text,
        ),
        Err(BackendError::Stale(_)) => ChangeOut::new(
            "failed",
            "Something changed in Cloudflare meanwhile; nothing was applied. Call again to see the new plan.",
            &text,
        ),
        Err(err) => return Err(err.into()),
    };
    Ok(ToolOutput::new(&out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_three_tools_with_classes() {
        let specs = specs();
        let names: Vec<(String, ToolClass)> = specs
            .iter()
            .map(|s| (s.tool.name.to_string(), s.class))
            .collect();
        assert_eq!(
            names,
            [
                ("list_reservations".to_owned(), ToolClass::Read),
                ("reserve_hostname".to_owned(), ToolClass::Change),
                ("release_hostname".to_owned(), ToolClass::Destructive),
            ]
        );
        let reserve = &specs[1].tool.input_schema;
        assert!(reserve["properties"]["until"].is_object(), "{reserve:?}");
        assert!(
            reserve["required"]
                .as_array()
                .is_some_and(|r| r.iter().any(|v| v == "hostname"))
        );
    }
}
