//! Diagnostics export: a small `.tar.gz` a user can attach to a bug report. Everything
//! in it passes through [`crate::redact`], and the UI shows the file list before it's
//! written. Secrets live in the keychain and are never read here.

use std::{
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use flate2::{Compression, write::GzEncoder};
use serde::Serialize;

use crate::redact::redact;

/// Newest log files included, and how much of each (the tail).
const LOG_FILES: usize = 3;
const LOG_TAIL_BYTES: usize = 2 * 1024 * 1024;

/// One file in the bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleFile {
    /// Path inside the archive.
    pub name: String,
    /// Redacted contents.
    pub text: String,
}

/// What the preview shows for a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FileSummary {
    /// Path inside the archive.
    pub name: String,
    /// Size in bytes.
    pub size: u32,
    /// The first lines, for a quick look.
    pub excerpt: String,
}

/// What goes into the bundle, gathered by the app.
#[derive(Debug, Clone)]
pub struct Inputs {
    /// Lines for `summary.txt` (versions, OS, binary, accounts by kind).
    pub summary: Vec<String>,
    /// The Doctor report.
    pub issues: Vec<crate::doctor::Issue>,
    /// The app settings as JSON.
    pub settings: serde_json::Value,
    /// Where the app writes its logs.
    pub log_dir: Option<PathBuf>,
}

fn tail(text: &str, max: usize) -> &str {
    if text.len() <= max {
        return text;
    }
    let mut start = text.len() - max;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    // Start at a line boundary.
    text[start..]
        .find('\n')
        .map_or(&text[start..], |i| &text[start + i + 1..])
}

fn newest_logs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut logs: Vec<(SystemTime, PathBuf)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "log"))
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    logs.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    logs.into_iter().take(LOG_FILES).map(|(_, p)| p).collect()
}

/// Builds the (redacted) bundle.
pub fn build(inputs: &Inputs) -> Vec<BundleFile> {
    let mut files = vec![
        BundleFile {
            name: "summary.txt".into(),
            text: inputs.summary.join("\n") + "\n",
        },
        BundleFile {
            name: "doctor.json".into(),
            text: serde_json::to_string_pretty(&inputs.issues).unwrap_or_default(),
        },
        BundleFile {
            name: "settings.json".into(),
            text: serde_json::to_string_pretty(&inputs.settings).unwrap_or_default(),
        },
    ];
    if let Some(dir) = &inputs.log_dir {
        for path in newest_logs(dir) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                let name = path.file_name().map_or_else(
                    || "app.log".to_owned(),
                    |n| n.to_string_lossy().into_owned(),
                );
                files.push(BundleFile {
                    name: format!("logs/{name}"),
                    text: tail(&text, LOG_TAIL_BYTES).to_owned(),
                });
            }
        }
    }
    for file in &mut files {
        file.text = redact(&file.text).into_owned();
    }
    files
}

/// Summaries for the preview.
pub fn summarize(files: &[BundleFile]) -> Vec<FileSummary> {
    files
        .iter()
        .map(|f| FileSummary {
            name: f.name.clone(),
            size: u32::try_from(f.text.len()).unwrap_or(u32::MAX),
            excerpt: f.text.lines().take(6).collect::<Vec<_>>().join("\n"),
        })
        .collect()
}

/// A file added as it is: cloudflared's own report, which Teitunnel can't redact (the
/// preview says so).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attachment {
    /// Path inside the archive.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// How long `cloudflared tunnel diag` may take (it runs traceroutes).
pub const CLOUDFLARED_REPORT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// Runs `cloudflared tunnel diag` (the `binary` at hand) against the connector whose
/// metrics server listens on `metrics_port`, in a directory of its own, and returns the
/// zip it writes there. The directory is removed after.
///
/// # Errors
/// Why there's no report, in a sentence for `cloudflared-diag.txt`.
pub async fn cloudflared_report(
    binary: &Path,
    metrics_port: u16,
    timeout: std::time::Duration,
) -> Result<Attachment, String> {
    let spec = cloudflared::DiagCmd { metrics_port }.build(binary);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = std::env::temp_dir().join(format!("teitunnel-diag-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make a folder for it: {e}"))?;
    let result = report_in(spec.to_command(), &dir, timeout).await;
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Runs `command` in `dir` and picks up the report it leaves there.
async fn report_in(
    mut command: tokio::process::Command,
    dir: &Path,
    timeout: std::time::Duration,
) -> Result<Attachment, String> {
    command
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let status = tokio::time::timeout(timeout, command.status())
        .await
        .map_err(|_| format!("cloudflared took longer than {} s", timeout.as_secs()))?
        .map_err(|e| format!("couldn't start cloudflared: {e}"))?;
    let zip = std::fs::read_dir(dir)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("cloudflared-diag") && n.ends_with(".zip"))
        });
    match zip {
        Some(path) => std::fs::read(&path)
            .map(|bytes| Attachment {
                name: "cloudflared-diag.zip".into(),
                bytes,
            })
            .map_err(|e| e.to_string()),
        None => Err(format!("cloudflared made no report (exit status {status})")),
    }
}

/// Writes the bundle as `.tar.gz` (readable only by the user).
///
/// # Errors
/// File system errors.
pub fn write(files: &[BundleFile], attachments: &[Attachment], path: &Path) -> std::io::Result<()> {
    let file = std::fs::File::create(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    let mut archive = tar::Builder::new(GzEncoder::new(file, Compression::default()));
    let mtime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let entries = files
        .iter()
        .map(|f| (f.name.as_str(), f.text.as_bytes()))
        .chain(
            attachments
                .iter()
                .map(|a| (a.name.as_str(), a.bytes.as_slice())),
        );
    for (name, bytes) in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o600);
        header.set_mtime(mtime);
        header.set_cksum();
        archive.append_data(&mut header, format!("teitunnel-diagnostics/{name}"), bytes)?;
    }
    archive.into_inner()?.finish()?.flush()
}

/// `teitunnel-diagnostics-2026-09-23-1405.tar.gz` for the current UTC time.
pub fn file_name() -> String {
    timestamped_name("teitunnel-diagnostics", "tar.gz")
}

/// `<prefix>-2026-09-23-1405.<extension>` for the current UTC time: names for files the
/// app saves, sorted by when they were made.
pub fn timestamped_name(prefix: &str, extension: &str) -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    let minutes = (secs % 86_400) / 60;
    format!(
        "{prefix}-{y:04}-{m:02}-{d:02}-{:02}{:02}.{extension}",
        minutes / 60,
        minutes % 60
    )
}

/// Days since 1970-01-01 → (year, month, day), proleptic Gregorian (H. Hinnant).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;

    #[test]
    fn bundles_redacted_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("teitunnel.2026-09-23.log"),
            "INFO started\nERROR api said Bearer abc.def.ghi and TUNNEL_TOKEN=eyJhIjoiMTIzNDU2Nzg5MDEyMzQ1Njc4In0\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("notes.txt"), "not a log").unwrap();
        let files = build(&Inputs {
            summary: vec!["Teitunnel 0.4.0".into(), "macOS 27.0 (arm64)".into()],
            issues: Vec::new(),
            settings: serde_json::json!({ "theme": "system" }),
            log_dir: Some(dir.path().to_path_buf()),
        });
        let names: Vec<_> = files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "summary.txt",
                "doctor.json",
                "settings.json",
                "logs/teitunnel.2026-09-23.log"
            ]
        );
        let log = &files[3].text;
        assert!(
            !log.contains("abc.def") && !log.contains("eyJhIjoi"),
            "{log}"
        );

        let out = dir.path().join("bundle.tar.gz");
        let report = Attachment {
            name: "cloudflared-diag.zip".into(),
            bytes: vec![0x50, 0x4b, 0x03, 0x04],
        };
        write(&files, std::slice::from_ref(&report), &out).unwrap();
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(
            std::fs::File::open(&out).unwrap(),
        ));
        let mut seen = Vec::new();
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).unwrap();
            seen.push(entry.path().unwrap().display().to_string());
            assert!(!String::from_utf8_lossy(&bytes).contains("eyJhIjoi"));
        }
        assert_eq!(seen.len(), 5);
        assert_eq!(seen[4], "teitunnel-diagnostics/cloudflared-diag.zip");
        assert!(seen[0].starts_with("teitunnel-diagnostics/"));
        assert_eq!(
            summarize(&files)[0].excerpt,
            "Teitunnel 0.4.0\nmacOS 27.0 (arm64)"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn picks_up_cloudflareds_report_and_gives_up_in_time() {
        let dir = tempfile::tempdir().unwrap();
        let mut touch = tokio::process::Command::new("touch");
        touch.arg("cloudflared-diag-2026-09-23T10-00-00.zip");
        let report = report_in(touch, dir.path(), std::time::Duration::from_secs(10))
            .await
            .unwrap();
        assert_eq!(report.name, "cloudflared-diag.zip");

        let empty = tempfile::tempdir().unwrap();
        let mut nothing = tokio::process::Command::new("true");
        nothing.arg("--");
        let err = report_in(nothing, empty.path(), std::time::Duration::from_secs(10))
            .await
            .unwrap_err();
        assert!(err.contains("no report"), "{err}");

        let mut sleep = tokio::process::Command::new("sleep");
        sleep.arg("5");
        let err = report_in(sleep, empty.path(), std::time::Duration::from_millis(200))
            .await
            .unwrap_err();
        assert!(err.contains("longer than"), "{err}");
    }

    #[test]
    fn keeps_the_end_of_long_logs_at_a_line_boundary() {
        let text = "aaaa\nbbbb\ncccc\n";
        assert_eq!(tail(text, 7), "cccc\n");
        assert_eq!(tail(text, 100), text);
    }

    #[test]
    fn dates_are_civil() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(20_719), (2026, 9, 23));
        assert!(file_name().starts_with("teitunnel-diagnostics-20"));
        let log = timestamped_name("teitunnel-log", "txt");
        assert!(log.starts_with("teitunnel-log-20") && log.ends_with(".txt"));
    }
}
