//! Typed process invocations (the same rule as `crates/cloudflared`): a program path and
//! discrete arguments for `tokio::process::Command`, never a shell string. Everything that
//! runs a tool (`security`, `certutil`, `update-ca-certificates`, …) builds an
//! [`Invocation`] with a dedicated builder and runs it through a [`Runner`], so tests can
//! check the exact argv without touching real trust stores.

use std::{
    ffi::{OsStr, OsString},
    fmt,
    future::Future,
    io,
    path::{Path, PathBuf},
    process::Stdio,
};

/// A program and its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    program: PathBuf,
    args: Vec<OsString>,
}

impl Invocation {
    /// Starts an invocation of `program` (prefer an absolute path).
    #[must_use]
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
        }
    }

    /// Appends one argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// The program.
    #[must_use]
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The arguments.
    #[must_use]
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// The argv as strings (lossy), for tests and logs.
    #[must_use]
    pub fn argv(&self) -> Vec<String> {
        std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(OsString::as_os_str))
            .map(|s| s.to_string_lossy().into_owned())
            .collect()
    }

    /// The same invocation run through `prefix` (e.g. `pkexec` or `sudo`).
    #[must_use]
    pub fn prefixed(&self, prefix: &Path) -> Self {
        let mut args = vec![self.program.clone().into_os_string()];
        args.extend(self.args.iter().cloned());
        Self {
            program: prefix.to_path_buf(),
            args,
        }
    }

    /// A POSIX-shell rendering for the user to copy (each word single-quoted when needed).
    /// Display only: Teitunnel never runs this string.
    #[must_use]
    pub fn display_posix(&self) -> String {
        self.argv()
            .iter()
            .map(|a| posix_quote(a))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A Windows command-line rendering for the user to copy. Display only.
    #[must_use]
    pub fn display_windows(&self) -> String {
        self.argv()
            .iter()
            .map(|a| {
                if a.is_empty() || a.contains([' ', '\t', '"']) {
                    format!("\"{}\"", a.replace('"', "\\\""))
                } else {
                    a.clone()
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// A `tokio` command with discrete arguments, no stdin, and killed if dropped.
    #[must_use]
    pub fn to_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        cmd
    }
}

impl fmt::Display for Invocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display_posix())
    }
}

/// Quotes a word for a POSIX shell when it has anything but safe characters.
pub(crate) fn posix_quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./:=,+@%".contains(&b));
    if safe {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

/// What a finished process produced.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProcessOutput {
    /// Exit status 0.
    pub success: bool,
    /// The exit code, if the process exited normally.
    pub code: Option<i32>,
    /// Standard output (lossy UTF-8).
    pub stdout: String,
    /// Standard error (lossy UTF-8).
    pub stderr: String,
}

/// Runs invocations. [`SystemRunner`] runs real processes; tests use a recording fake.
pub trait Runner: fmt::Debug + Send + Sync {
    /// Runs `invocation` to completion.
    fn run(
        &self,
        invocation: &Invocation,
    ) -> impl Future<Output = io::Result<ProcessOutput>> + Send;
}

/// Runs real processes.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    async fn run(&self, invocation: &Invocation) -> io::Result<ProcessOutput> {
        tracing::debug!(argv = ?invocation.argv(), "running");
        let output = invocation.to_command().output().await?;
        Ok(ProcessOutput {
            success: output.status.success(),
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

/// Finds `name` on `PATH`, then in `extra_dirs` (e.g. `/usr/sbin`, Homebrew prefixes).
#[must_use]
pub fn find_program(name: &str, path_var: Option<&OsStr>, extra_dirs: &[&Path]) -> Option<PathBuf> {
    let from_path = path_var
        .map(|p| std::env::split_paths(p).collect::<Vec<_>>())
        .unwrap_or_default();
    from_path
        .iter()
        .map(PathBuf::as_path)
        .chain(extra_dirs.iter().copied())
        .flat_map(|dir| {
            let plain = dir.join(name);
            let exe = dir.join(format!("{name}.exe"));
            [plain, exe]
        })
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use super::*;

    /// Records every invocation and replies with scripted outputs (success by default).
    #[derive(Debug, Default)]
    pub(crate) struct FakeRunner {
        pub(crate) calls: Mutex<Vec<Vec<String>>>,
        pub(crate) replies: Mutex<VecDeque<ProcessOutput>>,
    }

    impl FakeRunner {
        pub(crate) fn reply(&self, success: bool, stdout: &str) {
            self.replies.lock().unwrap().push_back(ProcessOutput {
                success,
                code: Some(if success { 0 } else { 1 }),
                stdout: stdout.into(),
                stderr: String::new(),
            });
        }

        pub(crate) fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl Runner for FakeRunner {
        async fn run(&self, invocation: &Invocation) -> io::Result<ProcessOutput> {
            self.calls.lock().unwrap().push(invocation.argv());
            Ok(self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or(ProcessOutput {
                    success: true,
                    code: Some(0),
                    ..ProcessOutput::default()
                }))
        }
    }

    #[test]
    fn argv_and_display() {
        let inv = Invocation::new("/usr/bin/security")
            .arg("add-trusted-cert")
            .arg("/Users/a b/it's.pem");
        assert_eq!(
            inv.argv(),
            [
                "/usr/bin/security",
                "add-trusted-cert",
                "/Users/a b/it's.pem"
            ]
        );
        assert_eq!(
            inv.display_posix(),
            r"/usr/bin/security add-trusted-cert '/Users/a b/it'\''s.pem'"
        );
        assert_eq!(
            Invocation::new("certutil.exe")
                .arg("-user")
                .arg("C:\\a b\\c.crt")
                .display_windows(),
            "certutil.exe -user \"C:\\a b\\c.crt\""
        );
        let sudo = inv.prefixed(Path::new("/usr/bin/pkexec"));
        assert_eq!(sudo.argv()[..2], ["/usr/bin/pkexec", "/usr/bin/security"]);
    }

    #[test]
    fn finds_programs_on_path_and_extra_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("tool"), "").unwrap();
        std::fs::write(extra.path().join("other"), "").unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        assert_eq!(
            find_program("tool", Some(&path), &[]),
            Some(dir.path().join("tool"))
        );
        assert_eq!(
            find_program("other", Some(&path), &[extra.path()]),
            Some(extra.path().join("other"))
        );
        assert_eq!(find_program("missing", Some(&path), &[extra.path()]), None);
    }

    #[tokio::test]
    async fn system_runner_runs_without_a_shell() {
        #[cfg(unix)]
        {
            let out = SystemRunner
                .run(&Invocation::new("/bin/echo").arg("a;b $HOME"))
                .await
                .unwrap();
            assert!(out.success);
            assert_eq!(out.stdout, "a;b $HOME\n");
        }
    }
}
