//! `teitunnel-cli doctor`: the app's checks, from the terminal. Issues ignored in the
//! app stay hidden; `--fix` applies only the fixes the app's "Fix Safe Issues" would
//! (each through a fresh plan that touches nothing Teitunnel doesn't own).

use std::{collections::HashSet, fmt::Write as _, io::Write as _, process::ExitCode};

use teitunnel_core::{
    doctor::{self, Issue, Severity, safe_change},
    text::Text,
};

use crate::{confirm, context::App};

/// A one-word label for a severity.
fn label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Info => "info",
    }
}

/// Whether `--fix` may apply `issue`'s fix.
fn fixable(issue: &Issue) -> bool {
    safe_change(issue).is_some()
}

/// The issues as text, most severe first.
pub(crate) fn render(issues: &[Issue]) -> String {
    if issues.is_empty() {
        return "No problems found.\n".to_owned();
    }
    let mut text = String::new();
    for issue in issues {
        let _ = writeln!(
            text,
            "{:<7} {}  ({})\n        {}",
            label(issue.severity),
            issue.title.english(),
            issue.check,
            issue.detail.english()
        );
        for evidence in &issue.evidence {
            let _ = writeln!(text, "        · {}", evidence.english());
        }
    }
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    let _ = write!(
        text,
        "\n{} problem{}, {errors} error{}.",
        issues.len(),
        if issues.len() == 1 { "" } else { "s" },
        if errors == 1 { "" } else { "s" },
    );
    if issues.iter().any(fixable) {
        text.push_str(" Run with --fix to apply the safe fixes.");
    }
    text.push('\n');
    text
}

/// An issue for `--json`: its messages as English text, the rest as the app sees it.
fn issue_json(issue: &Issue) -> serde_json::Value {
    serde_json::json!({
        "id": issue.id,
        "check": issue.check,
        "severity": issue.severity,
        "accountId": issue.account_id,
        "subject": issue.subject,
        "title": issue.title.english(),
        "detail": issue.detail.english(),
        "evidence": issue.evidence.iter().map(Text::english).collect::<Vec<_>>(),
        "fixes": issue.fixes,
    })
}

/// Exit code for scripts: failure when there's an error.
pub(crate) fn exit_code(issues: &[Issue]) -> ExitCode {
    if issues.iter().any(|i| i.severity == Severity::Error) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub(crate) async fn run(app: &App, json: bool, fix: bool, yes: bool) -> Result<ExitCode, String> {
    let connectors = app.all_connectors().await;
    let ignored: HashSet<String> = app.ignored_issues().await.into_iter().collect();
    let issues: Vec<Issue> = doctor::run(
        &app.accounts,
        &app.engine,
        &connectors,
        &app.binary,
        &app.machine_name,
    )
    .await
    .into_iter()
    .filter(|i| !ignored.contains(&i.id))
    .collect();

    if json {
        let list: Vec<_> = issues.iter().map(issue_json).collect();
        out!(
            "{}",
            serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?
        )?;
    } else {
        write!(std::io::stdout().lock(), "{}", render(&issues)).map_err(|e| e.to_string())?;
    }
    if !fix {
        return Ok(exit_code(&issues));
    }
    let candidates = issues.iter().filter(|i| fixable(i)).count();
    if candidates == 0 {
        return Ok(exit_code(&issues));
    }
    let question = format!(
        "Apply the safe fix{} for {candidates} issue{}?",
        if candidates == 1 { "" } else { "es" },
        if candidates == 1 { "" } else { "s" }
    );
    if !yes && !confirm(&question)? {
        return Ok(ExitCode::FAILURE);
    }
    let mut failed = Vec::new();
    let mut fixed = 0;
    for account in app.accounts.list().await.map_err(|e| e.to_string())? {
        let Ok(api) = app.accounts.client(&account.id).await else {
            continue;
        };
        let report = doctor::fix_safe(
            &app.engine,
            &api,
            &connectors,
            app.context(&account),
            &issues,
        )
        .await;
        fixed += report.fixed;
        failed.extend(report.failed);
    }
    out!("Fixed {fixed}.")?;
    for failure in &failed {
        out!("Couldn't fix {failure}")?;
    }
    // Check again, so the exit code reflects what's left.
    let left: Vec<Issue> = doctor::run(
        &app.accounts,
        &app.engine,
        &connectors,
        &app.binary,
        &app.machine_name,
    )
    .await
    .into_iter()
    .filter(|i| !ignored.contains(&i.id))
    .collect();
    Ok(if failed.is_empty() {
        exit_code(&left)
    } else {
        ExitCode::FAILURE
    })
}

#[cfg(test)]
mod tests {
    use teitunnel_core::{doctor::Fix, engine::Change, text::msg};

    use super::*;

    fn issue(severity: Severity, fix: bool) -> Issue {
        Issue {
            id: "dns.missing:acc:app.xyz.com".into(),
            check: "dns.missing".into(),
            severity,
            account_id: Some("acc".into()),
            subject: "app.xyz.com".into(),
            label: msg::raw("app.xyz.com"),
            title: msg::raw("app.xyz.com has no DNS record"),
            detail: msg::raw("Visitors can't reach it."),
            evidence: vec![msg::raw("No A, AAAA or CNAME record")],
            fixes: if fix {
                vec![Fix::Change {
                    label: msg::raw("Remove the Login"),
                    change: Change::RemoveLogin {
                        domain: "old.xyz.com".into(),
                    },
                }]
            } else {
                Vec::new()
            },
        }
    }

    #[test]
    fn prints_issues_and_a_summary() {
        let text = render(&[issue(Severity::Error, true), issue(Severity::Info, false)]);
        assert!(text.starts_with("error   app.xyz.com has no DNS record  (dns.missing)\n"));
        assert!(text.contains("        · No A, AAAA or CNAME record\n"));
        assert!(text.ends_with("2 problems, 1 error. Run with --fix to apply the safe fixes.\n"));
        assert_eq!(render(&[]), "No problems found.\n");
    }

    #[test]
    fn fails_only_on_errors() {
        assert_eq!(
            exit_code(&[issue(Severity::Warning, false)]),
            ExitCode::SUCCESS
        );
        assert_eq!(
            exit_code(&[issue(Severity::Error, false)]),
            ExitCode::FAILURE
        );
    }
}
