//! Quick Shares started by `teitunnel-cli share` in a terminal, as the app sees them: the
//! CLI records its live share next to its process registry (`run-cli/<pid>-<start>/`),
//! and the app lists the ones whose CLI is still running and can stop them (M10-03).

use std::{
    path::Path,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::runtime;

const FILE: &str = "share.json";

/// A terminal's live Quick Share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CliShare {
    /// The CLI process (`<pid>-<start time>`).
    pub owner: String,
    /// What's shared.
    pub origin: String,
    /// Its public URL.
    pub url: String,
    /// When it started (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub started_at: u64,
    /// When it stops by itself (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub stop_at: Option<u64>,
}

/// Records this process's live share in its registry directory (atomically).
///
/// # Errors
/// File system errors.
pub fn record(owner_dir: &Path, share: &CliShare) -> std::io::Result<()> {
    std::fs::create_dir_all(owner_dir)?;
    let partial = owner_dir.join("share.json.partial");
    std::fs::write(&partial, serde_json::to_vec(share)?)?;
    std::fs::rename(partial, owner_dir.join(FILE))
}

/// Forgets this process's share.
pub fn forget(owner_dir: &Path) {
    let _ = std::fs::remove_file(owner_dir.join(FILE));
}

/// The live shares of CLI processes still running, oldest first.
pub fn list(runs: &Path) -> Vec<CliShare> {
    let Ok(entries) = std::fs::read_dir(runs) else {
        return Vec::new();
    };
    let mut shares: Vec<CliShare> = entries
        .flatten()
        .filter_map(|entry| {
            let owner = entry.file_name().to_str()?.to_owned();
            if !runtime::is_running(&owner) {
                return None;
            }
            let share: CliShare =
                serde_json::from_slice(&std::fs::read(entry.path().join(FILE)).ok()?).ok()?;
            (share.owner == owner).then_some(share)
        })
        .collect();
    shares.sort_by(|a, b| (a.started_at, &a.owner).cmp(&(b.started_at, &b.owner)));
    shares
}

/// Stops a terminal's share: asks its CLI to end (it removes the route and its
/// connector), then stops whatever a CLI that didn't end cleanly left behind. Returns
/// whether it was running.
pub async fn stop(runs: &Path, owner: &str) -> bool {
    // Only a CLI that's running and has a share: never a reused pid.
    if !list(runs).iter().any(|s| s.owner == owner) {
        return false;
    }
    let Some(pid) = owner
        .split_once('-')
        .and_then(|(pid, _)| pid.parse::<u32>().ok())
    else {
        return false;
    };
    runtime::interrupt(pid);
    let deadline = Instant::now() + Duration::from_secs(5);
    while runtime::is_running(owner) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    runtime::PidRegistry::reap_abandoned(runs).await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_only_running_clis() {
        let runs = tempfile::tempdir().unwrap();
        let me = runtime::this_process();
        let share = |owner: &str| CliShare {
            owner: owner.into(),
            origin: "http://localhost:3000".into(),
            url: "https://a-b.trycloudflare.com".into(),
            started_at: 1,
            stop_at: None,
        };
        record(&runs.path().join(&me), &share(&me)).unwrap();
        // A CLI that's gone (pid 1's start time is never 1).
        record(&runs.path().join("1-1"), &share("1-1")).unwrap();
        let listed = list(runs.path());
        assert_eq!(listed, [share(&me)]);
        forget(&runs.path().join(&me));
        assert!(list(runs.path()).is_empty());
    }
}
