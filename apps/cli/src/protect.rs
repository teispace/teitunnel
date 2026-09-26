//! `teitunnel protect` and `teitunnel service-token`: rules at Cloudflare's edge for one
//! hostname (bots, AI crawlers, a rate limit, header rules) and Access service tokens
//! for machines. Every change is shown as a plan before it's applied, like routes; a new
//! token's secret is printed once and never saved.

use std::{
    io::{self, Write},
    process::ExitCode,
};

use clap::{Args, Subcommand, ValueEnum};
use teitunnel_core::{
    accounts::Account,
    engine::{
        Approval, Outcome, StepState,
        edge::{
            BotMode, EdgeHeaderOp, EdgeProtection, HeaderRule, IssuedToken, LimitAction, QuotaKind,
            RateLimitSpec,
        },
    },
    protection::{self, ProtectionChange, ProtectionView, ServiceTokenView},
    text::UserText,
};

use crate::context::App;

/// What to do with automated clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Bots {
    /// Nothing.
    Off,
    /// A managed challenge.
    Challenge,
    /// Refused.
    Block,
}

/// `teitunnel protect <hostname>`.
#[derive(Debug, Args)]
pub(crate) struct ProtectArgs {
    /// The hostname, e.g. `app.teispace.com` (a route, or a share on your domain).
    hostname: String,
    /// Automated clients (scripts, headless browsers; verified bots like search engines
    /// are let through).
    #[arg(long, value_enum)]
    bots: Option<Bots>,
    /// Block AI crawlers (Cloudflare's verified "AI Crawler" bots).
    #[arg(long, conflicts_with = "allow_ai")]
    block_ai: bool,
    /// Stop blocking AI crawlers.
    #[arg(long)]
    allow_ai: bool,
    /// Requests per period per visitor, then block or challenge: `30/1m`,
    /// `100/10s:challenge` (Pro plans and up; periods 10s, 1m, 2m, 5m, 10m, 1h).
    #[arg(long, value_name = "N/PERIOD[:ACTION]", value_parser = parse_rate_limit, conflicts_with = "no_rate_limit")]
    rate_limit: Option<RateLimitSpec>,
    /// Remove the rate limit.
    #[arg(long)]
    no_rate_limit: bool,
    /// Set a header on requests before they reach your service: `Name:value`.
    #[arg(long, value_name = "NAME:VALUE", value_parser = parse_set)]
    set_request_header: Vec<HeaderRule>,
    /// Remove a header from requests.
    #[arg(long, value_name = "NAME", value_parser = parse_remove)]
    remove_request_header: Vec<HeaderRule>,
    /// Set a header on responses: `Name:value`, e.g. `X-Robots-Tag:noindex`.
    #[arg(long, value_name = "NAME:VALUE", value_parser = parse_set)]
    set_response_header: Vec<HeaderRule>,
    /// Add a header to responses (keeps existing values): `Name:value`.
    #[arg(long, value_name = "NAME:VALUE", value_parser = parse_add)]
    add_response_header: Vec<HeaderRule>,
    /// Remove a header from responses.
    #[arg(long, value_name = "NAME", value_parser = parse_remove)]
    remove_response_header: Vec<HeaderRule>,
    /// Start from no header rules (then apply the header options given).
    #[arg(long)]
    clear_headers: bool,
    /// Stop Cloudflare caching responses, so a dev server's changes show at once (needs
    /// the Cache Rules permission).
    #[arg(long, conflicts_with = "no_bypass_cache")]
    bypass_cache: bool,
    /// Let Cloudflare cache responses again.
    #[arg(long)]
    no_bypass_cache: bool,
    /// Remove every rule Teitunnel added for the hostname.
    #[arg(long, conflicts_with_all = ["bots", "block_ai", "rate_limit", "clear_headers", "bypass_cache"])]
    off: bool,
    #[command(flatten)]
    change: ChangeArgs,
}

/// Shared by every change.
#[derive(Debug, Default, Args)]
pub(crate) struct ChangeArgs {
    /// Account name or id.
    #[arg(long, short)]
    account: Option<String>,
    /// Apply without asking.
    #[arg(long, short)]
    yes: bool,
    /// Print JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ServiceTokenCommand {
    /// Create a token machines use to pass the hostname's login (CI, scripts, servers).
    /// Its secret is printed once.
    Create {
        /// The hostname.
        hostname: String,
        /// What it's for.
        #[arg(long, default_value = "CLI")]
        name: String,
        #[command(flatten)]
        change: ChangeArgs,
    },
    /// List Teitunnel's tokens for the hostname, with their expiry.
    Ls {
        /// The hostname.
        hostname: String,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Give a token a new secret (printed once; the old one stops working).
    Rotate {
        /// The hostname.
        hostname: String,
        /// The token's name or id.
        token: String,
        #[command(flatten)]
        change: ChangeArgs,
    },
    /// Revoke a token: machines using it are refused.
    Revoke {
        /// The hostname.
        hostname: String,
        /// The token's name or id.
        token: String,
        #[command(flatten)]
        change: ChangeArgs,
    },
}

/// `30/1m`, `100/10s:challenge`.
pub(crate) fn parse_rate_limit(input: &str) -> Result<RateLimitSpec, String> {
    let (limit, action) = match input.split_once(':') {
        Some((limit, "block")) => (limit, LimitAction::Block),
        Some((limit, "challenge")) => (limit, LimitAction::Challenge),
        Some((_, other)) => {
            return Err(format!(
                "`{other}` isn't an action; use block or challenge."
            ));
        }
        None => (input, LimitAction::Block),
    };
    let (requests, period) = limit
        .split_once('/')
        .ok_or("Give requests per period, e.g. 30/1m.")?;
    let requests: u32 = requests
        .trim()
        .parse()
        .map_err(|_| format!("`{requests}` isn't a number of requests."))?;
    let period = period.trim();
    let (number, unit) = period.split_at(
        period
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(period.len()),
    );
    let number: u32 = if number.is_empty() {
        1
    } else {
        number
            .parse()
            .map_err(|_| format!("`{period}` isn't a period."))?
    };
    let seconds = match unit {
        "s" | "" => number,
        "m" => number.saturating_mul(60),
        "h" => number.saturating_mul(3600),
        _ => {
            return Err(format!(
                "`{period}` isn't a period; use 10s, 1m, 2m, 5m, 10m or 1h."
            ));
        }
    };
    Ok(RateLimitSpec {
        requests,
        period: seconds,
        action,
    })
}

fn header(input: &str, op: EdgeHeaderOp) -> Result<HeaderRule, String> {
    let (name, value) = input
        .split_once(':')
        .ok_or_else(|| format!("Give `Name:value`, not `{input}`."))?;
    Ok(HeaderRule {
        name: name.trim().to_owned(),
        op,
        value: Some(value.trim().to_owned()),
    })
}

fn parse_set(input: &str) -> Result<HeaderRule, String> {
    header(input, EdgeHeaderOp::Set)
}

fn parse_add(input: &str) -> Result<HeaderRule, String> {
    header(input, EdgeHeaderOp::Add)
}

fn parse_remove(input: &str) -> Result<HeaderRule, String> {
    Ok(HeaderRule {
        name: input.trim().to_owned(),
        op: EdgeHeaderOp::Remove,
        value: None,
    })
}

/// Replaces rules with the same name (case-insensitively), keeping the others.
fn upsert(list: &mut Vec<HeaderRule>, rules: impl IntoIterator<Item = HeaderRule>) {
    for rule in rules {
        list.retain(|r| !r.name.eq_ignore_ascii_case(&rule.name));
        list.push(rule);
    }
}

impl ProtectArgs {
    /// Whether any setting was given (otherwise the command shows the current ones).
    fn changes_anything(&self) -> bool {
        self.off
            || self.bots.is_some()
            || self.block_ai
            || self.allow_ai
            || self.rate_limit.is_some()
            || self.no_rate_limit
            || self.clear_headers
            || !self.set_request_header.is_empty()
            || !self.remove_request_header.is_empty()
            || !self.set_response_header.is_empty()
            || !self.add_response_header.is_empty()
            || !self.remove_response_header.is_empty()
            || self.bypass_cache
            || self.no_bypass_cache
    }

    /// `current` with the options applied.
    pub(crate) fn apply_to(&self, current: &EdgeProtection) -> EdgeProtection {
        if self.off {
            return EdgeProtection::default();
        }
        let mut next = current.clone();
        if let Some(bots) = self.bots {
            next.bots = match bots {
                Bots::Off => BotMode::Off,
                Bots::Challenge => BotMode::Challenge,
                Bots::Block => BotMode::Block,
            };
        }
        if self.block_ai {
            next.ai_crawlers = true;
        }
        if self.allow_ai {
            next.ai_crawlers = false;
        }
        if let Some(limit) = self.rate_limit {
            next.rate_limit = Some(limit);
        }
        if self.no_rate_limit {
            next.rate_limit = None;
        }
        if self.bypass_cache {
            next.bypass_cache = true;
        }
        if self.no_bypass_cache {
            next.bypass_cache = false;
        }
        if self.clear_headers {
            next.request_headers.clear();
            next.response_headers.clear();
        }
        upsert(
            &mut next.request_headers,
            self.set_request_header
                .iter()
                .chain(&self.remove_request_header)
                .cloned(),
        );
        upsert(
            &mut next.response_headers,
            self.set_response_header
                .iter()
                .chain(&self.add_response_header)
                .chain(&self.remove_response_header)
                .cloned(),
        );
        next
    }
}

fn describe(view: &ProtectionView) -> Vec<String> {
    let p = &view.protection;
    let mut lines = vec![format!(
        "{} ({}, {:?} plan)",
        view.hostname, view.zone, view.plan
    )];
    lines.push(format!(
        "  bots:          {}",
        match p.bots {
            BotMode::Off => "allowed",
            BotMode::Challenge => "challenged",
            BotMode::Block => "blocked",
        }
    ));
    lines.push(format!(
        "  AI crawlers:   {}",
        if p.ai_crawlers { "blocked" } else { "allowed" }
    ));
    lines.push(format!(
        "  rate limit:    {}",
        p.rate_limit.map_or_else(
            || {
                if view.rate_limit_available {
                    "none".to_owned()
                } else {
                    "not available on the Free plan (it can't be limited to one hostname)"
                        .to_owned()
                }
            },
            |l| format!(
                "{} requests per {} s per visitor, then {}",
                l.requests,
                l.period,
                match l.action {
                    LimitAction::Block => "block",
                    LimitAction::Challenge => "challenge",
                }
            ),
        )
    ));
    lines.push(format!(
        "  cache:         {}",
        match (p.bypass_cache, view.cache_rules) {
            (true, _) => "bypassed (Cloudflare never caches responses)",
            (false, true) => "as usual",
            (false, false) => "as usual (bypassing it needs the Cache Rules permission)",
        }
    ));
    for (label, list) in [
        ("request", &p.request_headers),
        ("response", &p.response_headers),
    ] {
        for rule in list {
            let action = match rule.op {
                EdgeHeaderOp::Set => "set",
                EdgeHeaderOp::Add => "add",
                EdgeHeaderOp::Remove => "remove",
            };
            let value = rule
                .value
                .as_deref()
                .map(|v| format!(" = {v}"))
                .unwrap_or_default();
            lines.push(format!("  {label} header: {action} {}{value}", rule.name));
        }
    }
    for quota in &view.quotas {
        lines.push(format!(
            "  {} on {}: {} of {} used",
            quota_name(quota.quota),
            view.zone,
            quota.used,
            quota.limit
        ));
    }
    lines
}

/// A quota's name, as Cloudflare's dashboard says it.
pub(crate) fn quota_name(quota: QuotaKind) -> &'static str {
    match quota {
        QuotaKind::Custom => "custom rules",
        QuotaKind::RateLimit => "rate limiting rules",
        QuotaKind::Transform => "Transform Rules",
        QuotaKind::Cache => "Cache Rules",
    }
}

fn engine_error(err: &teitunnel_core::engine::EngineError) -> String {
    err.text().english()
}

/// `teitunnel protect`.
pub(crate) async fn protect(app: &App, args: ProtectArgs) -> Result<ExitCode, String> {
    let account = app.account(args.change.account.as_deref()).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let view = protection::view(&app.engine, &api, app.context(&account), &args.hostname)
        .await
        .map_err(|e| engine_error(&e))?;
    if !args.changes_anything() {
        if args.change.json {
            out!(
                "{}",
                serde_json::to_string_pretty(&view).map_err(|e| e.to_string())?
            )?;
        } else {
            for line in describe(&view) {
                out!("{line}")?;
            }
        }
        return Ok(ExitCode::SUCCESS);
    }
    let change = ProtectionChange::Protect {
        hostname: args.hostname.clone(),
        protection: args.apply_to(&view.protection),
    };
    let (applied, _) = apply(app, &account, &change, &args.change).await?;
    Ok(exit(applied))
}

/// `teitunnel service-token …`.
pub(crate) async fn service_token(
    app: &App,
    command: ServiceTokenCommand,
) -> Result<ExitCode, String> {
    match command {
        ServiceTokenCommand::Ls {
            hostname,
            account,
            json,
        } => {
            let account = app.account(account.as_deref()).await?;
            let tokens = tokens(app, &account, &hostname).await?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string_pretty(&tokens).map_err(|e| e.to_string())?
                )?;
            } else if tokens.is_empty() {
                out!("No service tokens for {hostname}.")?;
            } else {
                for token in tokens {
                    let expiry = token.expires_at.as_deref().unwrap_or("never");
                    let gone = if token.gone {
                        "\tdeleted in the dashboard"
                    } else {
                        ""
                    };
                    out!(
                        "{}\t{}\texpires {expiry}{gone}",
                        token.label,
                        token.client_id
                    )?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ServiceTokenCommand::Create {
            hostname,
            name,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let request = ProtectionChange::CreateToken {
                hostname,
                label: name,
            };
            let (applied, issued) = apply(app, &account, &request, &change).await?;
            print_secrets(&issued, change.json)?;
            Ok(exit(applied))
        }
        ServiceTokenCommand::Rotate {
            hostname,
            token,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let token_id = find(app, &account, &hostname, &token).await?;
            let request = ProtectionChange::RotateToken { hostname, token_id };
            let (applied, issued) = apply(app, &account, &request, &change).await?;
            print_secrets(&issued, change.json)?;
            Ok(exit(applied))
        }
        ServiceTokenCommand::Revoke {
            hostname,
            token,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let token_id = find(app, &account, &hostname, &token).await?;
            let request = ProtectionChange::RevokeToken { hostname, token_id };
            let (applied, _) = apply(app, &account, &request, &change).await?;
            Ok(exit(applied))
        }
    }
}

async fn tokens(
    app: &App,
    account: &Account,
    hostname: &str,
) -> Result<Vec<ServiceTokenView>, String> {
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    protection::tokens(&app.engine, &api, &account.id, hostname)
        .await
        .map_err(|e| engine_error(&e))
}

/// A token's id from its name (label) or id.
async fn find(
    app: &App,
    account: &Account,
    hostname: &str,
    wanted: &str,
) -> Result<String, String> {
    tokens(app, account, hostname)
        .await?
        .into_iter()
        .find(|t| t.id == wanted || t.label.eq_ignore_ascii_case(wanted))
        .map(|t| t.id)
        .ok_or_else(|| format!("{hostname} has no service token called \"{wanted}\". See `teitunnel service-token ls {hostname}`."))
}

/// The secret, once: on stdout (for a pipe or a CI secret store), with a note on stderr.
fn print_secrets(issued: &[IssuedToken], json: bool) -> Result<(), String> {
    for token in issued {
        if json {
            out!(
                "{}",
                serde_json::json!({
                    "tokenId": token.token_id,
                    "name": token.name,
                    "clientId": token.client_id,
                    "clientSecret": token.client_secret.expose(),
                    "expiresAt": token.expires_at,
                })
            )?;
        } else {
            out!("{}", protection::headers(token))?;
            let _ = writeln!(
                io::stderr().lock(),
                "Save the secret now: Cloudflare won't show it again, and Teitunnel doesn't keep it."
            );
        }
    }
    Ok(())
}

fn exit(applied: bool) -> ExitCode {
    if applied {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Previews, asks, applies and reports a change; `true` when it was applied, with any
/// new token's credentials.
async fn apply(
    app: &App,
    account: &Account,
    change: &ProtectionChange,
    args: &ChangeArgs,
) -> Result<(bool, Vec<IssuedToken>), String> {
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let ctx = app.context(account);
    let plan = protection::preview(&app.engine, &api, ctx, change)
        .await
        .map_err(|e| engine_error(&e))?;
    if plan.steps.is_empty() {
        out!("Nothing to change.")?;
        return Ok((true, Vec::new()));
    }
    for warning in &plan.warnings {
        out!("! {}", crate::warning_text(warning))?;
    }
    for (index, step) in plan.steps.iter().enumerate() {
        out!("{:>2}. {}", index + 1, step.description.english())?;
    }
    if !args.yes && !crate::confirm("Apply?")? {
        out!("Nothing changed.")?;
        return Ok((false, Vec::new()));
    }
    let connectors = app.connectors(account).await;
    let steps = plan.steps.clone();
    let (outcome, issued) = protection::apply(
        &app.engine,
        &api,
        &connectors,
        ctx,
        change,
        Approval {
            fingerprint: &plan.fingerprint,
            confirmed: false,
        },
        |progress| {
            let Some(step) = steps.get(usize::try_from(progress.step).unwrap_or(usize::MAX)) else {
                return;
            };
            let mark = match progress.state {
                StepState::Done => "done",
                StepState::Failed { .. } => "failed",
                StepState::Undone => "undone",
                StepState::UndoFailed { .. } => "couldn't undo",
                _ => return,
            };
            let _ = writeln!(
                io::stdout().lock(),
                "    {mark}: {}",
                step.description.english()
            );
        },
    )
    .await
    .map_err(|e| engine_error(&e))?;
    match outcome {
        Outcome::Applied { .. } => Ok((true, issued)),
        Outcome::RolledBack { error, .. } => {
            out!("Failed: {}. Everything was undone.", error.english())?;
            Ok((false, Vec::new()))
        }
        Outcome::PartiallyApplied {
            error, leftovers, ..
        } => {
            out!("Failed: {}. These couldn't be undone:", error.english())?;
            for leftover in leftovers {
                out!("  - {}", leftover.english())?;
            }
            Ok((false, Vec::new()))
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Test {
        #[command(flatten)]
        args: ProtectArgs,
    }

    fn args(line: &[&str]) -> ProtectArgs {
        Test::try_parse_from(std::iter::once("protect").chain(line.iter().copied()))
            .unwrap()
            .args
    }

    #[test]
    fn parses_rate_limits() {
        assert_eq!(
            parse_rate_limit("30/1m").unwrap(),
            RateLimitSpec {
                requests: 30,
                period: 60,
                action: LimitAction::Block
            }
        );
        assert_eq!(
            parse_rate_limit("100/10s:challenge").unwrap(),
            RateLimitSpec {
                requests: 100,
                period: 10,
                action: LimitAction::Challenge
            }
        );
        assert_eq!(parse_rate_limit("5/1h").unwrap().period, 3600);
        assert!(parse_rate_limit("30").is_err());
        assert!(parse_rate_limit("30/1d").is_err());
        assert!(parse_rate_limit("30/1m:tarpit").is_err());
    }

    #[test]
    fn options_change_only_what_they_name() {
        let current = EdgeProtection {
            bots: BotMode::Challenge,
            ai_crawlers: true,
            response_headers: vec![HeaderRule {
                name: "X-Robots-Tag".into(),
                op: EdgeHeaderOp::Set,
                value: Some("noindex".into()),
            }],
            ..EdgeProtection::default()
        };
        let next = args(&[
            "app.xyz.com",
            "--bots",
            "block",
            "--rate-limit",
            "30/1m",
            "--set-request-header",
            "X-Env: preview",
            "--remove-response-header",
            "x-robots-tag",
        ])
        .apply_to(&current);
        assert_eq!(next.bots, BotMode::Block);
        assert!(next.ai_crawlers, "kept");
        assert_eq!(next.rate_limit.unwrap().requests, 30);
        assert_eq!(next.request_headers[0].value.as_deref(), Some("preview"));
        assert_eq!(next.response_headers.len(), 1, "replaced by name");
        assert_eq!(next.response_headers[0].op, EdgeHeaderOp::Remove);
        assert!(!next.bypass_cache, "kept");

        let bypassed = args(&["app.xyz.com", "--bypass-cache"]).apply_to(&current);
        assert!(bypassed.bypass_cache);
        assert_eq!(bypassed.bots, BotMode::Challenge, "kept");
        assert!(
            !args(&["app.xyz.com", "--no-bypass-cache"])
                .apply_to(&bypassed)
                .bypass_cache
        );
        assert!(
            Test::try_parse_from([
                "protect",
                "app.xyz.com",
                "--bypass-cache",
                "--no-bypass-cache"
            ])
            .is_err()
        );

        assert_eq!(
            args(&["app.xyz.com", "--off"]).apply_to(&current),
            EdgeProtection::default()
        );
        assert!(!args(&["app.xyz.com"]).changes_anything(), "shows instead");
        assert!(
            Test::try_parse_from(["protect", "app.xyz.com", "--off", "--bots", "block"]).is_err()
        );
    }
}
