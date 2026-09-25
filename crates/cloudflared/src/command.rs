//! Typed builders for every cloudflared invocation Teitunnel makes.
//!
//! Commands are discrete arguments for `tokio::process::Command`, never a shell string.
//! Run tokens travel in the environment (Session mode) or a 0600 file (Always-on), never
//! in argv, where any local user could read them from the process list. [`TunnelToken`]
//! has no conversion to `OsString`, so putting it in an argument doesn't compile.

use std::{
    ffi::{OsStr, OsString},
    fmt,
    path::{Path, PathBuf},
    process::Stdio,
};

use tokio::process::Command;

/// A tunnel run token. Redacted in `Debug` and never convertible to an argument.
#[derive(Clone, PartialEq, Eq)]
pub struct TunnelToken(String);

impl TunnelToken {
    /// Wraps a token received from the Cloudflare API or the keychain.
    pub fn new(token: String) -> Self {
        Self(token)
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for TunnelToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TunnelToken([redacted])")
    }
}

/// How `cloudflared tunnel run` receives its token.
#[derive(Debug, Clone)]
pub enum TokenSource {
    /// Session mode: `TUNNEL_TOKEN` in the child's environment.
    Env(TunnelToken),
    /// Always-on mode: `--token-file` pointing at a 0600 file.
    File(PathBuf),
}

/// Transport protocol between cloudflared and the edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Protocol {
    /// Let cloudflared choose (QUIC, falling back to HTTP/2).
    #[default]
    Auto,
    /// HTTP/2 over TCP, for networks that block UDP.
    Http2,
    /// QUIC only.
    Quic,
}

impl Protocol {
    fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Http2 => "http2",
            Self::Quic => "quic",
        }
    }
}

/// Log verbosity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogLevel {
    /// Connection events and errors.
    #[default]
    Info,
    /// Everything, for troubleshooting.
    Debug,
}

/// A fully described process launch: program, arguments and extra environment.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    program: PathBuf,
    args: Vec<OsString>,
    token: Option<TunnelToken>,
}

const TOKEN_ENV: &str = "TUNNEL_TOKEN";

impl CommandSpec {
    /// The executable.
    pub fn program(&self) -> &Path {
        &self.program
    }

    /// The arguments (never containing a token).
    pub fn args(&self) -> &[OsString] {
        &self.args
    }

    /// Names of the environment variables set for the child (values not exposed).
    pub fn env_names(&self) -> Vec<&'static str> {
        self.token.as_ref().map(|_| TOKEN_ENV).into_iter().collect()
    }

    /// A `tokio` command ready to spawn: piped output, null stdin, killed if dropped.
    pub fn to_command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // Inherited values must never shadow ours, and a stray token in the parent's
        // environment must never leak into a child that shouldn't have one.
        command.env_remove(TOKEN_ENV);
        if let Some(token) = &self.token {
            command.env(TOKEN_ENV, token.expose());
        }
        #[cfg(unix)]
        command.process_group(0);
        crate::process::no_console(&mut command);
        command
    }

    /// A copy-pasteable shell rendering for "Copy as command". Secrets appear as
    /// `$TUNNEL_TOKEN`; arguments are quoted when needed.
    pub fn to_display_command(&self) -> String {
        let mut parts = Vec::with_capacity(self.args.len() + 2);
        if self.token.is_some() {
            parts.push(format!("{TOKEN_ENV}=\"$TUNNEL_TOKEN\""));
        }
        parts.push(quote(self.program.as_os_str()));
        parts.extend(self.args.iter().map(|arg| quote(arg)));
        parts.join(" ")
    }
}

fn quote(arg: &OsStr) -> String {
    let text = arg.to_string_lossy();
    let safe = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@%+,".contains(c));
    if safe {
        text.into_owned()
    } else {
        format!("'{}'", text.replace('\'', r"'\''"))
    }
}

/// Flags every long-running invocation gets: we manage updates, parse JSON logs and
/// read the local metrics server on a port we chose.
fn common_args(metrics_port: u16, log_level: LogLevel) -> Vec<OsString> {
    [
        "tunnel",
        "--no-autoupdate",
        "--output",
        "json",
        "--loglevel",
        match log_level {
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
        },
        "--metrics",
    ]
    .into_iter()
    .map(OsString::from)
    .chain([OsString::from(format!("127.0.0.1:{metrics_port}"))])
    .collect()
}

/// What a neutral config file holds: valid YAML with no settings. (An empty file works
/// too, but cloudflared logs "Configuration file … was empty" as an error.)
pub const NEUTRAL_CONFIG: &str =
    "# Written by Teitunnel. Quick Shares read no cloudflared settings from files.\n{}\n";

/// `cloudflared tunnel --url <origin>`: an anonymous `trycloudflare.com` Quick Share.
#[derive(Debug, Clone)]
pub struct QuickTunnelCmd {
    /// Local origin, e.g. `http://localhost:3000`.
    pub origin: String,
    /// Port for the local metrics server (and `/quicktunnel`).
    pub metrics_port: u16,
    /// A file holding [`NEUTRAL_CONFIG`], passed as `--config`. Without it cloudflared
    /// reads `~/.cloudflared/config.yml` (or `/etc/cloudflared/…`) if there is one, and
    /// that file's `ingress` rules take precedence over `--url`, so a leftover named
    /// tunnel config would make the share serve something else or fail.
    pub config: PathBuf,
    /// Host header sent to the origin (`--http-host-header`), for dev servers that only
    /// answer their own address.
    pub host_header: Option<String>,
}

impl QuickTunnelCmd {
    /// Builds the launch spec for `binary`.
    pub fn build(&self, binary: &Path) -> CommandSpec {
        let mut args = vec![
            OsString::from("tunnel"),
            OsString::from("--config"),
            self.config.clone().into_os_string(),
        ];
        args.extend(
            common_args(self.metrics_port, LogLevel::Info)
                .into_iter()
                .skip(1),
        );
        if let Some(host) = &self.host_header {
            args.extend([OsString::from("--http-host-header"), OsString::from(host)]);
        }
        args.extend([OsString::from("--url"), OsString::from(&self.origin)]);
        CommandSpec {
            program: binary.to_path_buf(),
            args,
            token: None,
        }
    }
}

/// `cloudflared tunnel diag`: cloudflared's own diagnostic report of a running
/// connector (2024.12.2+). It writes `cloudflared-diag-<time>.zip` into the working
/// directory, so run it in a directory of its own.
#[derive(Debug, Clone)]
pub struct DiagCmd {
    /// The connector's local metrics server port.
    pub metrics_port: u16,
}

impl DiagCmd {
    /// Builds the launch spec for `binary`.
    pub fn build(&self, binary: &Path) -> CommandSpec {
        CommandSpec {
            program: binary.to_path_buf(),
            args: [
                "tunnel".to_owned(),
                "diag".to_owned(),
                "--metrics".to_owned(),
                format!("127.0.0.1:{}", self.metrics_port),
            ]
            .map(OsString::from)
            .to_vec(),
            token: None,
        }
    }
}

/// `cloudflared tunnel run`: a connector for a remotely managed named tunnel.
#[derive(Debug, Clone)]
pub struct RunCmd {
    /// How the token is delivered.
    pub token: TokenSource,
    /// Port for the local metrics server.
    pub metrics_port: u16,
    /// Edge transport.
    pub protocol: Protocol,
    /// Log verbosity.
    pub log_level: LogLevel,
    /// Rotating log directory (Always-on mode, where no one reads stderr).
    pub log_dir: Option<PathBuf>,
}

impl RunCmd {
    /// Builds the launch spec for `binary`.
    pub fn build(&self, binary: &Path) -> CommandSpec {
        let mut args = common_args(self.metrics_port, self.log_level);
        if self.protocol != Protocol::Auto {
            args.extend(["--protocol", self.protocol.as_str()].map(OsString::from));
        }
        if let Some(dir) = &self.log_dir {
            args.extend([
                OsString::from("--log-directory"),
                dir.clone().into_os_string(),
            ]);
        }
        args.push(OsString::from("run"));
        let token = match &self.token {
            TokenSource::Env(token) => Some(token.clone()),
            TokenSource::File(path) => {
                args.extend([
                    OsString::from("--token-file"),
                    path.clone().into_os_string(),
                ]);
                None
            }
        };
        CommandSpec {
            program: binary.to_path_buf(),
            args,
            token,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "eyJhIjoiYWNjIiwidCI6InR1bm5lbCIsInMiOiJzZWNyZXQifQ==";

    fn args(spec: &CommandSpec) -> Vec<String> {
        spec.args()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn diag_targets_the_connectors_metrics_server() {
        let spec = DiagCmd {
            metrics_port: 20411,
        }
        .build(Path::new("/opt/homebrew/bin/cloudflared"));
        assert_eq!(
            args(&spec),
            ["tunnel", "diag", "--metrics", "127.0.0.1:20411"]
        );
        assert!(spec.env_names().is_empty());
    }

    #[test]
    fn quick_tunnel_args() {
        let spec = QuickTunnelCmd {
            origin: "http://localhost:3000".into(),
            metrics_port: 20301,
            config: "/data/quick-share.yml".into(),
            host_header: None,
        }
        .build(Path::new("/opt/homebrew/bin/cloudflared"));
        assert_eq!(
            args(&spec),
            [
                "tunnel",
                "--config",
                "/data/quick-share.yml",
                "--no-autoupdate",
                "--output",
                "json",
                "--loglevel",
                "info",
                "--metrics",
                "127.0.0.1:20301",
                "--url",
                "http://localhost:3000"
            ]
        );
        assert!(spec.env_names().is_empty());
    }

    #[test]
    fn quick_tunnel_never_reads_a_default_config_file() {
        // A leftover ~/.cloudflared/config.yml has ingress rules that win over --url.
        // `--config` must come with our neutral file, before any other flag.
        let spec = QuickTunnelCmd {
            origin: "http://localhost:5173".into(),
            metrics_port: 20302,
            config: "/Users/me/Library/Application Support/t/quick-share.yml".into(),
            host_header: None,
        }
        .build(Path::new("cloudflared"));
        let a = args(&spec);
        assert_eq!(
            a[..3],
            [
                "tunnel",
                "--config",
                "/Users/me/Library/Application Support/t/quick-share.yml"
            ]
        );
        assert_eq!(a.iter().filter(|x| *x == "--config").count(), 1);
        assert!(
            NEUTRAL_CONFIG.lines().any(|line| line == "{}"),
            "valid, empty YAML"
        );
    }

    #[test]
    fn quick_tunnel_sends_a_host_header_when_asked() {
        let spec = QuickTunnelCmd {
            origin: "http://localhost:5173".into(),
            metrics_port: 20303,
            config: "/c.yml".into(),
            host_header: Some("localhost:5173".into()),
        }
        .build(Path::new("cloudflared"));
        let a = args(&spec);
        assert_eq!(
            a[a.len() - 4..],
            [
                "--http-host-header",
                "localhost:5173",
                "--url",
                "http://localhost:5173"
            ]
        );
    }

    #[test]
    fn run_with_env_token_keeps_it_out_of_args() {
        let spec = RunCmd {
            token: TokenSource::Env(TunnelToken::new(SECRET.into())),
            metrics_port: 20300,
            protocol: Protocol::Http2,
            log_level: LogLevel::Info,
            log_dir: None,
        }
        .build(Path::new("/usr/local/bin/cloudflared"));
        let rendered = args(&spec).join(" ");
        assert!(!rendered.contains(SECRET));
        assert!(rendered.ends_with("--protocol http2 run"));
        assert_eq!(spec.env_names(), ["TUNNEL_TOKEN"]);
        assert!(!format!("{spec:?}").contains(SECRET));
        let display = spec.to_display_command();
        assert!(!display.contains(SECRET));
        assert!(
            display.starts_with("TUNNEL_TOKEN=\"$TUNNEL_TOKEN\" /usr/local/bin/cloudflared tunnel")
        );
    }

    #[test]
    fn run_with_token_file_and_log_dir() {
        let spec = RunCmd {
            token: TokenSource::File("/Users/me/Library/Application Support/t/tokens/x".into()),
            metrics_port: 20302,
            protocol: Protocol::Auto,
            log_level: LogLevel::Debug,
            log_dir: Some("/tmp/logs".into()),
        }
        .build(Path::new("cloudflared"));
        let a = args(&spec);
        assert!(a.contains(&"debug".to_owned()));
        assert!(!a.contains(&"--protocol".to_owned()));
        assert_eq!(
            a[a.len() - 3..],
            [
                "run",
                "--token-file",
                "/Users/me/Library/Application Support/t/tokens/x"
            ]
        );
        assert!(spec.env_names().is_empty());
        assert!(
            spec.to_display_command()
                .contains("'/Users/me/Library/Application Support/t/tokens/x'")
        );
    }

    #[test]
    fn quoting() {
        assert_eq!(
            quote(OsStr::new("http://localhost:3000")),
            "http://localhost:3000"
        );
        assert_eq!(quote(OsStr::new("it's")), r"'it'\''s'");
        assert_eq!(quote(OsStr::new("")), "''");
        assert_eq!(quote(OsStr::new("a;rm -rf")), "'a;rm -rf'");
    }
}
