use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, ProcessesToUpdate, System};

use super::{signal, state::ConnectorId};

const MARKER: &str = "teitunnel";

/// A record of a process we started, so it can be found again after a force-quit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PidRecord {
    marker: String,
    pid: u32,
    /// Process start time (seconds since the epoch), which makes PID reuse harmless.
    start_time: u64,
    connector: String,
}

/// Pidfiles under `<app_data>/run/` for every process the supervisor starts.
///
/// On launch, [`PidRegistry::reap_orphans`] stops processes left behind by a crash or
/// force-quit of the app. A process is only touched if its pid **and** start time match
/// a record we wrote, so PIDs we didn't start are never killed.
#[derive(Debug, Clone)]
pub struct PidRegistry {
    dir: PathBuf,
}

impl PidRegistry {
    /// A registry in `dir` (created on first write).
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn path(&self, id: &ConnectorId) -> PathBuf {
        let safe: String =
            id.0.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '-' {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
        self.dir.join(format!("{safe}.json"))
    }

    /// Records a freshly spawned process.
    pub fn record(&self, id: &ConnectorId, pid: u32) {
        let Some(start_time) = start_time(pid) else {
            return;
        };
        let record = PidRecord {
            marker: MARKER.into(),
            pid,
            start_time,
            connector: id.0.clone(),
        };
        // Written to a temporary name and renamed into place, so another process
        // reaping records never reads (and discards) a half-written one.
        let write = || -> std::io::Result<()> {
            fs::create_dir_all(&self.dir)?;
            let path = self.path(id);
            let partial = path.with_extension("json.partial");
            fs::write(&partial, serde_json::to_vec(&record)?)?;
            fs::rename(&partial, &path)
        };
        if let Err(err) = write() {
            tracing::warn!(connector = %id, error = %err, "failed to write pidfile");
        }
    }

    /// Forgets a process that has exited or been stopped.
    pub fn remove(&self, id: &ConnectorId) {
        let _ = fs::remove_file(self.path(id));
    }

    /// Stops processes recorded by a previous run that are still alive. Returns the
    /// pids that were stopped.
    pub async fn reap_orphans(&self) -> Vec<u32> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut reaped = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            // Another process may be writing one right now; it's renamed into place when
            // complete.
            if path.extension().is_some_and(|e| e == "partial") {
                continue;
            }
            if let Some(record) = read_record(&path)
                && start_time(record.pid) == Some(record.start_time)
            {
                tracing::info!(
                    pid = record.pid,
                    connector = record.connector,
                    "stopping orphaned connector"
                );
                stop_pid(record.pid).await;
                reaped.push(record.pid);
            }
            let _ = fs::remove_file(&path);
        }
        reaped
    }
}

/// Registries of short-lived processes (the CLI): one directory per owner under a
/// shared root, named after the owner's pid and start time, so an owner that died
/// without stopping its connectors can be told apart from one still running.
impl PidRegistry {
    /// This process's registry under `root`.
    pub fn for_this_process(root: &Path) -> Self {
        let pid = std::process::id();
        let started = start_time(pid).unwrap_or_default();
        Self::new(root.join(format!("{pid}-{started}")))
    }

    /// Stops the connectors of owners under `root` that are no longer running, and
    /// removes their registries. Returns the pids stopped.
    pub async fn reap_abandoned(root: &Path) -> Vec<u32> {
        let Ok(entries) = fs::read_dir(root) else {
            return Vec::new();
        };
        let mut reaped = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some((pid, started)) = name
                .to_str()
                .and_then(|n| n.split_once('-'))
                .and_then(|(p, s)| Some((p.parse::<u32>().ok()?, s.parse::<u64>().ok()?)))
            else {
                continue;
            };
            if start_time(pid) == Some(started) {
                continue; // The owner is alive: its connectors are its business.
            }
            let registry = Self::new(entry.path());
            reaped.extend(registry.reap_orphans().await);
            let _ = fs::remove_dir(entry.path());
        }
        reaped
    }
}

fn read_record(path: &Path) -> Option<PidRecord> {
    let record: PidRecord = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (record.marker == MARKER).then_some(record)
}

/// This process, as `<pid>-<start time>`: stable for its lifetime, never reused.
pub fn this_process() -> String {
    let pid = std::process::id();
    format!("{pid}-{}", start_time(pid).unwrap_or_default())
}

/// Whether the process `identity` (from [`this_process`]) is still running.
pub fn is_running(identity: &str) -> bool {
    identity
        .split_once('-')
        .and_then(|(pid, started)| Some((pid.parse::<u32>().ok()?, started.parse::<u64>().ok()?)))
        .is_some_and(|(pid, started)| start_time(pid) == Some(started))
}

fn start_time(pid: u32) -> Option<u64> {
    let mut system = System::new();
    let pid = Pid::from_u32(pid);
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).map(sysinfo::Process::start_time)
}

pub(crate) async fn stop_pid(pid: u32) {
    signal::signal_process(pid, false);
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if start_time(pid).is_none() {
            return;
        }
    }
    signal::signal_process(pid, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ignores_stale_and_foreign_records() {
        let dir = tempfile::tempdir().unwrap();
        let registry = PidRegistry::new(dir.path().to_path_buf());
        // Our own pid with a wrong start time must never be killed.
        let record = PidRecord {
            marker: MARKER.into(),
            pid: std::process::id(),
            start_time: 1,
            connector: "x".into(),
        };
        fs::write(
            dir.path().join("x.json"),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        fs::write(dir.path().join("junk.json"), b"not json").unwrap();
        assert!(registry.reap_orphans().await.is_empty());
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            0,
            "records are cleaned up"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reaps_connectors_only_of_owners_that_are_gone() {
        let root = tempfile::tempdir().unwrap();
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let child_pid = child.id();
        // An owner that's gone (pid 1's start time never matches 1), and a live one: us.
        let gone = PidRegistry::new(root.path().join("1-1"));
        gone.record(&ConnectorId("qs".into()), child_pid);
        let alive = PidRegistry::for_this_process(root.path());
        alive.record(&ConnectorId("qs".into()), child_pid);
        std::fs::create_dir_all(root.path().join("not-an-owner")).unwrap();

        assert_eq!(PidRegistry::reap_abandoned(root.path()).await, [child_pid]);
        let _ = child.wait();
        let left: Vec<String> = fs::read_dir(root.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(!left.contains(&"1-1".to_owned()), "{left:?}");
        assert!(left.contains(&"not-an-owner".to_owned()));
        assert_eq!(left.len(), 2, "the live owner's registry stays: {left:?}");
    }

    #[test]
    fn sanitises_file_names() {
        let registry = PidRegistry::new(PathBuf::from("/run"));
        assert_eq!(
            registry.path(&ConnectorId("../qs/1".into())),
            PathBuf::from("/run/___qs_1.json")
        );
    }
}
