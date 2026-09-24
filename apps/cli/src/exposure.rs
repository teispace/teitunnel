//! The exposure check before something goes public (`share`, `route add`, a project's
//! shares): the findings are printed as warnings and the command carries on, unless
//! `--strict` asks it to stop. Off when turned off in the app's settings.

use teitunnel_core::{
    exposure::{self, ExposureReport, ExposureSeverity},
    store::Store,
};

/// Whether the check is on (the app's setting; on when there's no app).
pub(crate) async fn enabled(store: Option<&Store>) -> bool {
    match store {
        Some(store) => teitunnel_core::settings::load(store)
            .await
            .map_or(true, |s| s.exposure_check),
        None => true,
    }
}

/// The warning lines for a report (nothing when it's clean).
pub(crate) fn describe(report: &ExposureReport) -> Vec<String> {
    if report.is_clean() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "! {} may expose more than you mean to share:",
        report.origin
    )];
    for finding in &report.findings {
        let level = match finding.severity {
            ExposureSeverity::High => "high",
            ExposureSeverity::Medium => "medium",
            ExposureSeverity::Low => "low",
        };
        let detail = finding
            .detail
            .as_deref()
            .map(|d| format!(" ({d})"))
            .unwrap_or_default();
        lines.push(format!(
            "  [{level}] {}{detail} at {}",
            finding.title.english(),
            finding.path
        ));
        lines.push(format!("         {}", finding.advice.english()));
    }
    lines
}

/// Checks `origin` when the check is on and prints what it finds. With `strict`,
/// findings are an error (nothing is shared).
///
/// # Errors
/// A message when `strict` and something was found.
pub(crate) async fn check(origin: &str, store: Option<&Store>, strict: bool) -> Result<(), String> {
    if !enabled(store).await {
        return Ok(());
    }
    let report = exposure::check(origin).await;
    for line in describe(&report) {
        crate::share::status(&line);
    }
    if strict && !report.is_clean() {
        return Err(format!(
            "Not shared: the exposure check found {} problem(s) (--strict).",
            report.findings.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use teitunnel_core::exposure::{ExposureFinding, ExposureKind};

    use super::*;

    #[test]
    fn describes_findings_without_values() {
        let clean = ExposureReport {
            origin: "http://localhost:3000".into(),
            findings: Vec::new(),
            requests: 20,
            incomplete: false,
            elapsed_ms: 12,
        };
        assert!(describe(&clean).is_empty());
        let kind = ExposureKind::EnvFile;
        let report = ExposureReport {
            findings: vec![ExposureFinding {
                kind,
                severity: kind.severity(),
                path: "/.env".into(),
                title: kind.title(),
                advice: kind.advice(),
                detail: Some("APP_KEY, DB_PASSWORD".into()),
            }],
            ..clean
        };
        let lines = describe(&report);
        assert!(lines[0].contains("http://localhost:3000"));
        assert!(
            lines[1].starts_with(
                "  [high] Its .env file can be downloaded (APP_KEY, DB_PASSWORD) at /.env"
            ),
            "{lines:?}"
        );
    }
}
