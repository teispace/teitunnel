//! Finding a usable `cloudflared` on this machine.
//!
//! Precedence: an explicit override (`TEITUNNEL_CLOUDFLARED`, used by tests and E2E),
//! then the managed copy in the app data directory, then `$PATH`, then well-known
//! install locations. The first candidate whose `--version` we can read wins.

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::Serialize;
use tokio::process::Command;

use crate::{Error, Result, Version};

/// Oldest release with everything we rely on (`--output json`, `--token-file`).
pub const MIN_SUPPORTED: Version = Version {
    year: 2025,
    month: 6,
    patch: 1,
};

/// Environment variable that forces a specific binary (tests, E2E, power users).
pub const OVERRIDE_ENV: &str = "TEITUNNEL_CLOUDFLARED";

const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

#[cfg(windows)]
const EXE: &str = "cloudflared.exe";
#[cfg(not(windows))]
const EXE: &str = "cloudflared";

/// Where a binary came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BinarySource {
    /// Set explicitly through [`OVERRIDE_ENV`].
    Override,
    /// Installed and updated by Teitunnel.
    Managed,
    /// Installed by the user (Homebrew, package manager, manual).
    System,
}

/// A located binary and what we know about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryStatus {
    /// Absolute path to the executable.
    pub path: PathBuf,
    /// Where it came from.
    pub source: BinarySource,
    /// Its version, if `--version` output could be parsed (dev builds can't).
    pub version: Option<Version>,
}

impl BinaryStatus {
    /// Whether this binary is new enough for every feature Teitunnel uses.
    pub fn is_supported(&self) -> bool {
        self.version.is_some_and(|version| version >= MIN_SUPPORTED)
    }
}

/// Finds cloudflared binaries in precedence order.
#[derive(Debug, Clone)]
pub struct Locator {
    override_path: Option<PathBuf>,
    managed_dir: PathBuf,
    search_dirs: Vec<PathBuf>,
}

impl Locator {
    /// A locator for this process: [`OVERRIDE_ENV`], then `$PATH`, then well-known
    /// install locations (GUI apps on macOS get a minimal `$PATH` without Homebrew).
    pub fn from_env(managed_dir: PathBuf) -> Self {
        let mut search_dirs = path_dirs(std::env::var_os("PATH").as_deref());
        search_dirs.extend(well_known_dirs());
        Self::new(
            managed_dir,
            std::env::var_os(OVERRIDE_ENV).map(PathBuf::from),
            search_dirs,
        )
    }

    /// A locator with explicit inputs.
    pub fn new(
        managed_dir: PathBuf,
        override_path: Option<PathBuf>,
        search_dirs: Vec<PathBuf>,
    ) -> Self {
        Self {
            override_path,
            managed_dir,
            search_dirs,
        }
    }

    /// Directory holding the managed binary.
    pub fn managed_dir(&self) -> &Path {
        &self.managed_dir
    }

    /// Path where the managed binary lives (whether or not it's installed).
    pub fn managed_path(&self) -> PathBuf {
        self.managed_dir.join(EXE)
    }

    /// Existing executables in precedence order, without duplicates.
    pub fn candidates(&self) -> Vec<(PathBuf, BinarySource)> {
        let ordered = self
            .override_path
            .iter()
            .map(|path| (path.clone(), BinarySource::Override))
            .chain([(self.managed_path(), BinarySource::Managed)])
            .chain(
                self.search_dirs
                    .iter()
                    .map(|dir| (dir.join(EXE), BinarySource::System)),
            );

        let mut seen = HashSet::new();
        ordered
            .filter(|(path, _)| is_executable(path))
            .filter(|(path, _)| {
                seen.insert(std::fs::canonicalize(path).unwrap_or_else(|_| path.clone()))
            })
            .collect()
    }

    /// The first candidate whose version can be read.
    ///
    /// # Errors
    /// Returns [`Error::NotFound`] when no candidate works.
    pub async fn locate(&self) -> Result<BinaryStatus> {
        for (path, source) in self.candidates() {
            match read_version(&path).await {
                Ok(version) => {
                    return Ok(BinaryStatus {
                        path,
                        source,
                        version,
                    });
                }
                Err(err) => {
                    tracing::debug!(path = %path.display(), error = %err, "skipping cloudflared candidate");
                }
            }
        }
        Err(Error::NotFound)
    }
}

/// Runs `<path> --version`. `Ok(None)` means it ran but printed an unrecognised version
/// (e.g. a development build).
///
/// # Errors
/// Fails if the binary can't be run, times out, or exits unsuccessfully.
pub async fn read_version(path: &Path) -> Result<Option<Version>> {
    let output = tokio::time::timeout(
        VERSION_TIMEOUT,
        crate::process::no_console(&mut Command::new(path))
            .arg("--version")
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| Error::Timeout("cloudflared --version"))??;
    if !output.status.success() {
        return Err(Error::Exited(output.status.code()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(Version::from_version_output(&text).ok())
}

/// Splits a `$PATH`-style value into directories.
pub fn path_dirs(path_env: Option<&std::ffi::OsStr>) -> Vec<PathBuf> {
    path_env
        .map(|path| std::env::split_paths(path).collect())
        .unwrap_or_default()
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

fn well_known_dirs() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    let dirs = ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"];
    #[cfg(all(unix, not(target_os = "macos")))]
    let dirs = ["/usr/local/bin", "/usr/bin", "/snap/bin"];
    #[cfg(windows)]
    let dirs: [&str; 0] = [];
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut out: Vec<PathBuf> = dirs.iter().map(PathBuf::from).collect();
    #[cfg(windows)]
    {
        for var in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(base) = std::env::var_os(var) {
                out.push(PathBuf::from(base).join("cloudflared"));
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            out.push(PathBuf::from(local).join("cloudflared"));
        }
    }
    out
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt};

    use super::*;

    fn fake_binary(dir: &Path, version_line: &str) -> PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(EXE);
        fs::write(&path, format!("#!/bin/sh\necho '{version_line}'\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[tokio::test]
    async fn prefers_override_then_managed_then_path() {
        let root = tempfile::tempdir().unwrap();
        let managed = root.path().join("managed");
        let system = root.path().join("system");
        let custom = root.path().join("custom");
        fake_binary(&system, "cloudflared version 2025.8.0 (built x)");

        let locator = Locator::new(managed.clone(), None, vec![system.clone()]);
        let found = locator.locate().await.unwrap();
        assert_eq!(found.source, BinarySource::System);
        assert_eq!(found.version, Some("2025.8.0".parse().unwrap()));

        fake_binary(&managed, "cloudflared version 2026.9.1 (built x)");
        let found = locator.locate().await.unwrap();
        assert_eq!(found.source, BinarySource::Managed);
        assert!(found.is_supported());

        let over = fake_binary(&custom, "cloudflared version 2026.1.0 (built x)");
        let locator = Locator::new(managed, Some(over), vec![system]);
        assert_eq!(
            locator.locate().await.unwrap().source,
            BinarySource::Override
        );
    }

    #[tokio::test]
    async fn skips_non_executable_and_dedupes() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("bin");
        let path = fake_binary(&dir, "cloudflared version 2026.9.1 (built x)");
        let twice = std::env::join_paths([&dir, &dir]).unwrap();
        let locator = Locator::new(root.path().join("none"), None, path_dirs(Some(&twice)));
        assert_eq!(locator.candidates().len(), 1);

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(locator.candidates().is_empty());
        assert!(matches!(locator.locate().await, Err(Error::NotFound)));
    }

    #[tokio::test]
    async fn dev_builds_are_found_but_unsupported() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("bin");
        fake_binary(&dir, "cloudflared version DEV (built unknown)");
        let locator = Locator::new(root.path().join("none"), None, vec![dir]);
        let found = locator.locate().await.unwrap();
        assert_eq!(found.version, None);
        assert!(!found.is_supported());
    }

    #[test]
    fn minimum_version_boundary() {
        let status = |v: &str| BinaryStatus {
            path: PathBuf::new(),
            source: BinarySource::System,
            version: Some(v.parse().unwrap()),
        };
        assert!(!status("2025.6.0").is_supported());
        assert!(status("2025.6.1").is_supported());
        assert!(status("2026.9.1").is_supported());
    }
}
