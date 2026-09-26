//! Putting the `teitunnel` command that ships inside the app on the PATH.
//!
//! The packages carry the CLI next to the app's own binary. How it reaches the PATH:
//! - macOS: a symlink in Homebrew's `bin` or `/usr/local/bin`, whichever the user can
//!   write; the app bundle's path is stable across updates, so the link stays right.
//!   Without a writable one, the app shows the `sudo ln -s …` to run.
//! - Windows: a copy in `%LOCALAPPDATA%\Microsoft\WindowsApps`, which is on every user's
//!   PATH. The installer makes it and the uninstaller removes it
//!   (`windows/installer-hooks.nsh`); the app refreshes it at launch after an
//!   update and can put it back.
//! - Linux: `.deb`/`.rpm` install it to `/usr/bin` already. The AppImage's copy goes to
//!   `~/.local/bin`, refreshed at launch.
//!
//! Only what Teitunnel put there is ever replaced or removed.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use serde::Serialize;

/// The command's file name on the PATH: people type `teitunnel`.
pub const CLI_NAME: &str = if cfg!(windows) {
    "teitunnel.exe"
} else {
    "teitunnel"
};

/// The CLI's file name inside the packages, next to the app. Linux packages carry it as
/// `teitunnel`; on macOS and Windows, whose file systems ignore case, it can't sit next to
/// the app's own `Teitunnel`, so there it's `teitunnel-cli`.
pub const BUNDLED_NAME: &str = if cfg!(windows) {
    "teitunnel-cli.exe"
} else if cfg!(target_os = "macos") {
    "teitunnel-cli"
} else {
    "teitunnel"
};

/// How the CLI gets onto the PATH on this system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Method {
    /// A symlink to the bundled CLI, in the first of these directories that exists and
    /// is writable.
    Symlink(Vec<PathBuf>),
    /// A copy of the bundled CLI in this directory (created if missing).
    Copy(PathBuf),
}

/// Where the bundled CLI is and how to install it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// The CLI inside the installed app.
    pub bundled: PathBuf,
    pub method: Method,
}

/// Where the CLI stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum CliState {
    /// This build doesn't include it (development builds).
    Unavailable,
    /// The package manager already put it on the PATH.
    #[serde(rename_all = "camelCase")]
    Packaged { path: String },
    /// Teitunnel installed it here.
    #[serde(rename_all = "camelCase")]
    Installed { path: String },
    /// Not installed. `path`: where installing puts it; `command`: what to run instead
    /// when Teitunnel can't write there itself.
    #[serde(rename_all = "camelCase")]
    NotInstalled {
        path: Option<String>,
        command: Option<String>,
    },
    /// Something else is at that path; Teitunnel leaves it alone.
    #[serde(rename_all = "camelCase")]
    Taken { path: String },
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

/// Whether the current user can create files in `dir`.
fn writable(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    let probe = dir.join(format!(".teitunnel-probe-{}", std::process::id()));
    let created = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .is_ok();
    if created {
        let _ = fs::remove_file(&probe);
    }
    created
}

impl Layout {
    /// This system's layout, when the app ships the CLI next to `app_exe`.
    pub fn detect(app_exe: &Path) -> Option<Self> {
        let bundled = app_exe.parent()?.join(BUNDLED_NAME);
        if !bundled.is_file() {
            return None;
        }
        let home = std::env::home_dir();
        let method = if cfg!(target_os = "macos") {
            Method::Symlink(vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/usr/local/bin"),
            ])
        } else if cfg!(windows) {
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
            Method::Copy(local.join("Microsoft").join("WindowsApps"))
        } else {
            Method::Copy(home?.join(".local").join("bin"))
        };
        Some(Self { bundled, method })
    }

    fn symlink_target(&self) -> Option<(PathBuf, bool)> {
        let Method::Symlink(dirs) = &self.method else {
            return None;
        };
        // An existing link of ours wins, wherever it is.
        if let Some(dir) = dirs.iter().find(|d| self.is_ours(&d.join(CLI_NAME))) {
            return Some((dir.join(CLI_NAME), true));
        }
        dirs.iter()
            .find(|d| writable(d))
            .map(|d| (d.join(CLI_NAME), true))
            .or_else(|| dirs.last().map(|d| (d.join(CLI_NAME), false)))
    }

    /// Where the installed CLI goes, and whether the app can write there itself.
    fn target(&self) -> Option<(PathBuf, bool)> {
        match &self.method {
            Method::Symlink(_) => self.symlink_target(),
            Method::Copy(dir) => Some((dir.join(CLI_NAME), true)),
        }
    }

    /// Whether `path` is the CLI Teitunnel put there.
    fn is_ours(&self, path: &Path) -> bool {
        match &self.method {
            Method::Symlink(_) => fs::read_link(path).is_ok_and(|to| to == self.bundled),
            Method::Copy(_) => same_contents(path, &self.bundled),
        }
    }

    /// Where the CLI stands.
    pub fn state(&self) -> CliState {
        if self.bundled.starts_with("/usr/bin") {
            return CliState::Packaged {
                path: display(&self.bundled),
            };
        }
        let Some((target, can_write)) = self.target() else {
            return CliState::Unavailable;
        };
        if self.is_ours(&target) || (matches!(self.method, Method::Copy(_)) && is_copy(&target)) {
            return CliState::Installed {
                path: display(&target),
            };
        }
        if fs::symlink_metadata(&target).is_ok() {
            return CliState::Taken {
                path: display(&target),
            };
        }
        CliState::NotInstalled {
            path: Some(display(&target)),
            command: (!can_write).then(|| {
                format!(
                    "sudo ln -s '{}' '{}'",
                    self.bundled.display(),
                    target.display()
                )
            }),
        }
    }

    /// Installs it (or refreshes an outdated copy).
    ///
    /// # Errors
    /// When the target can't be written, or something that isn't Teitunnel's is there.
    pub fn install(&self) -> io::Result<CliState> {
        let Some((target, true)) = self.target() else {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "no writable directory on the PATH",
            ));
        };
        let exists = fs::symlink_metadata(&target).is_ok();
        if exists && !self.is_ours(&target) && !is_copy(&target) {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} isn't Teitunnel's", target.display()),
            ));
        }
        match &self.method {
            Method::Symlink(_) => {
                if !self.is_ours(&target) {
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(&self.bundled, &target)?;
                }
            }
            Method::Copy(dir) => {
                fs::create_dir_all(dir)?;
                copy_atomically(&self.bundled, &target)?;
            }
        }
        Ok(self.state())
    }

    /// Refreshes an installed copy after the app was updated (no-op otherwise).
    ///
    /// # Errors
    /// When the copy can't be replaced.
    pub fn refresh(&self) -> io::Result<bool> {
        let Method::Copy(dir) = &self.method else {
            return Ok(false);
        };
        let target = dir.join(CLI_NAME);
        if !is_copy(&target) || same_contents(&target, &self.bundled) {
            return Ok(false);
        }
        copy_atomically(&self.bundled, &target)?;
        Ok(true)
    }

    /// Removes it, if it's Teitunnel's.
    ///
    /// # Errors
    /// When it can't be removed.
    pub fn uninstall(&self) -> io::Result<CliState> {
        if let Some((target, _)) = self.target()
            && (self.is_ours(&target)
                || (matches!(self.method, Method::Copy(_)) && is_copy(&target)))
        {
            fs::remove_file(&target)?;
        }
        Ok(self.state())
    }
}

/// A copy Teitunnel made carries a marker file next to it, so an outdated copy is still
/// recognised as ours (and a `teitunnel` from elsewhere never is).
fn marker(target: &Path) -> PathBuf {
    target.with_extension("teitunnel")
}

fn is_copy(target: &Path) -> bool {
    target.is_file() && marker(target).is_file()
}

fn same_contents(a: &Path, b: &Path) -> bool {
    match (fs::read(a), fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Copies next to the target, then renames over it, so a running CLI is never half
/// written.
fn copy_atomically(from: &Path, to: &Path) -> io::Result<()> {
    let staged = to.with_extension("partial");
    fs::copy(from, &staged)?;
    fs::rename(&staged, to)?;
    fs::write(
        marker(to),
        "Installed by Teitunnel; updated when the app updates.\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled(dir: &Path, contents: &str) -> PathBuf {
        let app = dir.join("app");
        fs::create_dir_all(&app).unwrap();
        let path = app.join(BUNDLED_NAME);
        fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn copies_refreshes_and_removes_only_its_own_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        let layout = Layout {
            bundled: bundled(tmp.path(), "v1"),
            method: Method::Copy(bin.clone()),
        };
        let target = bin.join(CLI_NAME);
        assert!(matches!(
            layout.state(),
            CliState::NotInstalled { command: None, .. }
        ));

        assert!(matches!(
            layout.install().unwrap(),
            CliState::Installed { .. }
        ));
        assert_eq!(fs::read_to_string(&target).unwrap(), "v1");

        // The app updates: the copy follows at the next launch.
        fs::write(&layout.bundled, "v2").unwrap();
        assert!(matches!(layout.state(), CliState::Installed { .. }));
        assert!(layout.refresh().unwrap());
        assert_eq!(fs::read_to_string(&target).unwrap(), "v2");
        assert!(!layout.refresh().unwrap(), "nothing to do when current");

        assert!(matches!(
            layout.uninstall().unwrap(),
            CliState::NotInstalled { .. }
        ));
        assert!(!target.exists());
    }

    #[test]
    fn leaves_someone_elses_cli_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let bin = tmp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join(CLI_NAME), "built from source").unwrap();
        let layout = Layout {
            bundled: bundled(tmp.path(), "v1"),
            method: Method::Copy(bin.clone()),
        };
        assert!(matches!(layout.state(), CliState::Taken { .. }));
        assert!(layout.install().is_err());
        layout.uninstall().unwrap();
        assert_eq!(
            fs::read_to_string(bin.join(CLI_NAME)).unwrap(),
            "built from source"
        );
        assert!(!layout.refresh().unwrap());
    }

    #[cfg(unix)]
    #[test]
    fn links_into_the_first_writable_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("missing");
        let writable_dir = tmp.path().join("local-bin");
        fs::create_dir_all(&writable_dir).unwrap();
        let layout = Layout {
            bundled: bundled(tmp.path(), "v1"),
            method: Method::Symlink(vec![missing, writable_dir.clone()]),
        };
        layout.install().unwrap();
        let link = writable_dir.join(CLI_NAME);
        assert_eq!(fs::read_link(&link).unwrap(), layout.bundled);
        assert!(matches!(layout.state(), CliState::Installed { .. }));
        layout.uninstall().unwrap();
        assert!(fs::symlink_metadata(&link).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn offers_a_command_when_nothing_is_writable() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout {
            bundled: bundled(tmp.path(), "v1"),
            method: Method::Symlink(vec![tmp.path().join("nope")]),
        };
        match layout.state() {
            CliState::NotInstalled {
                command: Some(command),
                ..
            } => assert!(command.starts_with("sudo ln -s ")),
            other => panic!("unexpected {other:?}"),
        }
        assert!(layout.install().is_err());
    }

    #[test]
    fn a_package_manager_install_needs_nothing() {
        let layout = Layout {
            bundled: PathBuf::from("/usr/bin/teitunnel"),
            method: Method::Copy(PathBuf::from("/nonexistent")),
        };
        assert!(matches!(layout.state(), CliState::Packaged { .. }));
    }
}
