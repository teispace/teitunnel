//! Steps that need administrator rights, returned as data.
//!
//! Teitunnel never elevates on its own. A step is shown to the user as a fix-in-place
//! (with a copy button), or, with the user's consent, run through `pkexec` on Linux
//! ([`pkexec_invocations`]), or applied directly by a caller that already has the rights
//! ([`PrivilegedAction::apply`]).

use std::{
    io,
    path::{Path, PathBuf},
};

use crate::process::{Invocation, Runner, posix_quote};

/// One privileged step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivilegedAction {
    /// Create or replace a file (parent directories are created).
    WriteFile {
        /// Destination.
        path: PathBuf,
        /// Contents.
        contents: String,
        /// Unix mode.
        mode: u32,
    },
    /// Copy a file Teitunnel already wrote (e.g. the CA certificate) into place.
    CopyFile {
        /// Source, readable by the user.
        from: PathBuf,
        /// Destination.
        to: PathBuf,
        /// Unix mode.
        mode: u32,
    },
    /// Delete a file; a missing file is fine.
    RemoveFile {
        /// The file.
        path: PathBuf,
    },
    /// Run a program.
    Run(Invocation),
}

/// A privileged step failed.
#[derive(Debug, thiserror::Error)]
pub enum PrivilegedError {
    /// A file operation failed (often: not running with the needed rights).
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The error.
        source: io::Error,
    },
    /// A program exited with an error.
    #[error("{program} failed: {stderr}")]
    Failed {
        /// The program.
        program: String,
        /// Its standard error.
        stderr: String,
    },
}

impl PrivilegedAction {
    /// POSIX shell lines (with `sudo`) for the user to copy. Display only.
    #[must_use]
    pub fn copyable_posix(&self) -> String {
        match self {
            Self::WriteFile {
                path,
                contents,
                mode,
            } => {
                let path_s = path.to_string_lossy();
                let dir = path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default();
                format!(
                    "sudo mkdir -p {dir}\nprintf '%s' {contents} | sudo tee {path} > /dev/null\nsudo chmod {mode:o} {path}",
                    dir = posix_quote(&dir),
                    contents = posix_quote(contents),
                    path = posix_quote(&path_s),
                )
            }
            Self::CopyFile { from, to, mode } => format!(
                "sudo install -m {mode:o} {} {}",
                posix_quote(&from.to_string_lossy()),
                posix_quote(&to.to_string_lossy())
            ),
            Self::RemoveFile { path } => {
                format!("sudo rm -f {}", posix_quote(&path.to_string_lossy()))
            }
            Self::Run(inv) => format!("sudo {}", inv.display_posix()),
        }
    }

    /// Applies the step directly, for a caller that already runs with the needed rights.
    ///
    /// # Errors
    /// The file operation or program failed.
    pub async fn apply<R: Runner>(&self, runner: &R) -> Result<(), PrivilegedError> {
        let io_err = |path: &Path| {
            let path = path.to_path_buf();
            move |source| PrivilegedError::Io { path, source }
        };
        match self {
            Self::WriteFile {
                path,
                contents,
                mode,
            } => crate::fsutil::write_with_mode(path, contents.as_bytes(), *mode)
                .map_err(io_err(path)),
            Self::CopyFile { from, to, mode } => {
                let bytes = std::fs::read(from).map_err(io_err(from))?;
                crate::fsutil::write_with_mode(to, &bytes, *mode).map_err(io_err(to))
            }
            Self::RemoveFile { path } => match std::fs::remove_file(path) {
                Ok(()) => Ok(()),
                Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
                Err(err) => Err(io_err(path)(err)),
            },
            Self::Run(inv) => {
                let out = runner.run(inv).await.map_err(io_err(inv.program()))?;
                if out.success {
                    Ok(())
                } else {
                    Err(PrivilegedError::Failed {
                        program: inv.program().to_string_lossy().into_owned(),
                        stderr: out.stderr,
                    })
                }
            }
        }
    }
}

/// All steps as one copyable POSIX snippet.
#[must_use]
pub fn copyable_posix(actions: &[PrivilegedAction]) -> String {
    actions
        .iter()
        .map(PrivilegedAction::copyable_posix)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Turns the steps into `pkexec` invocations (Linux, with the user's consent; each shows
/// the system's authentication dialog). File contents are staged in `staging_dir` (owned by
/// the user) and installed with `install -D -m`.
///
/// # Errors
/// Staging a file failed.
pub fn pkexec_invocations(
    actions: &[PrivilegedAction],
    pkexec: &Path,
    staging_dir: &Path,
) -> io::Result<Vec<Invocation>> {
    let mut out = Vec::with_capacity(actions.len());
    for (index, action) in actions.iter().enumerate() {
        let inv = match action {
            PrivilegedAction::WriteFile {
                path,
                contents,
                mode,
            } => {
                let staged = staging_dir.join(format!("staged-{index}"));
                crate::fsutil::write_with_mode(&staged, contents.as_bytes(), 0o644)?;
                install(&staged, path, *mode)
            }
            PrivilegedAction::CopyFile { from, to, mode } => install(from, to, *mode),
            PrivilegedAction::RemoveFile { path } => Invocation::new("/usr/bin/rm")
                .arg("-f")
                .arg(path.as_os_str()),
            PrivilegedAction::Run(inv) => inv.clone(),
        };
        out.push(inv.prefixed(pkexec));
    }
    Ok(out)
}

fn install(from: &Path, to: &Path, mode: u32) -> Invocation {
    Invocation::new("/usr/bin/install")
        .arg("-D")
        .arg("-m")
        .arg(format!("{mode:o}"))
        .arg(from.as_os_str())
        .arg(to.as_os_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::tests::FakeRunner;

    #[test]
    fn copyable_text() {
        let write = PrivilegedAction::WriteFile {
            path: "/etc/resolver/test".into(),
            contents: "nameserver 127.0.0.1\nport 53535\n".into(),
            mode: 0o644,
        };
        assert_eq!(
            write.copyable_posix(),
            "sudo mkdir -p /etc/resolver\nprintf '%s' 'nameserver 127.0.0.1\nport 53535\n' | sudo tee /etc/resolver/test > /dev/null\nsudo chmod 644 /etc/resolver/test"
        );
        let run = PrivilegedAction::Run(Invocation::new("update-ca-certificates"));
        assert_eq!(run.copyable_posix(), "sudo update-ca-certificates");
    }

    #[test]
    fn pkexec_stages_and_installs() {
        let staging = tempfile::tempdir().unwrap();
        let actions = [
            PrivilegedAction::WriteFile {
                path: "/etc/x.conf".into(),
                contents: "a=1\n".into(),
                mode: 0o644,
            },
            PrivilegedAction::RemoveFile {
                path: "/etc/y".into(),
            },
            PrivilegedAction::Run(
                Invocation::new("/usr/bin/systemctl")
                    .arg("restart")
                    .arg("systemd-resolved"),
            ),
        ];
        let invs =
            pkexec_invocations(&actions, Path::new("/usr/bin/pkexec"), staging.path()).unwrap();
        let staged = staging.path().join("staged-0");
        assert_eq!(std::fs::read_to_string(&staged).unwrap(), "a=1\n");
        assert_eq!(
            invs[0].argv(),
            [
                "/usr/bin/pkexec",
                "/usr/bin/install",
                "-D",
                "-m",
                "644",
                staged.to_str().unwrap(),
                "/etc/x.conf"
            ]
        );
        assert_eq!(
            invs[1].argv(),
            ["/usr/bin/pkexec", "/usr/bin/rm", "-f", "/etc/y"]
        );
        assert_eq!(
            invs[2].argv(),
            [
                "/usr/bin/pkexec",
                "/usr/bin/systemctl",
                "restart",
                "systemd-resolved"
            ]
        );
    }

    #[tokio::test]
    async fn apply_writes_copies_and_removes() {
        let dir = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        let target = dir.path().join("a/b.conf");
        PrivilegedAction::WriteFile {
            path: target.clone(),
            contents: "x".into(),
            mode: 0o644,
        }
        .apply(&runner)
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "x");
        let copy = dir.path().join("c/d.pem");
        PrivilegedAction::CopyFile {
            from: target.clone(),
            to: copy.clone(),
            mode: 0o644,
        }
        .apply(&runner)
        .await
        .unwrap();
        assert_eq!(std::fs::read_to_string(&copy).unwrap(), "x");
        for _ in 0..2 {
            PrivilegedAction::RemoveFile { path: copy.clone() }
                .apply(&runner)
                .await
                .unwrap();
        }
        runner.reply(false, "");
        let err = PrivilegedAction::Run(Invocation::new("tool"))
            .apply(&runner)
            .await
            .unwrap_err();
        assert!(matches!(err, PrivilegedError::Failed { .. }));
    }
}
