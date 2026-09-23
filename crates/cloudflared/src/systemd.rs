//! Always-on connectors on Linux: a systemd user unit per tunnel (a
//! [`ServiceSpec`] as a `.service` file in `~/.config/systemd/user`), and typed
//! `systemctl --user` invocations (discrete arguments, never a shell).
//!
//! User units stop at logout unless lingering is enabled (`loginctl enable-linger`);
//! Teitunnel says so rather than enabling it. `StandardOutput=append:` needs systemd
//! 240 or newer (2018).

use std::{ffi::OsStr, path::Path, process::Stdio};

use tokio::process::Command;

use crate::service::{AgentState, ServiceSpec};

impl ServiceSpec {
    /// The unit file name, e.g. `com.teispace.teitunnel.connector.<id>.service`.
    pub fn unit_name(&self) -> String {
        format!("{}.service", self.label)
    }

    /// The unit: restarted when it exits, started with the user's session, low priority.
    pub fn unit(&self) -> String {
        let exec = std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(std::ffi::OsString::as_os_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ");
        let log = self.log_file.to_string_lossy().replace('%', "%%");
        format!(
            "[Unit]
Description=Teitunnel connector ({label})
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={exec}
Restart=always
RestartSec=10
Nice=10
StandardOutput=append:{log}
StandardError=append:{log}

[Install]
WantedBy=default.target
",
            label = self.label,
        )
    }
}

/// Quotes one `ExecStart=` word: double quotes, with `\`, `"` and `$` escaped (systemd
/// would expand `$VAR`) and `%` doubled (a specifier otherwise).
fn quote(word: &OsStr) -> String {
    let text = word.to_string_lossy();
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '\\' | '"' | '$' => {
                out.push('\\');
                out.push(c);
            }
            '%' => out.push_str("%%"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

fn systemctl(args: &[&str]) -> Command {
    let mut command = Command::new("systemctl");
    command
        .arg("--user")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// `systemctl --user daemon-reload`: pick up written or removed unit files.
pub fn daemon_reload() -> Command {
    systemctl(&["daemon-reload"])
}

/// `systemctl --user enable --now <unit>`: start it, and at every login.
pub fn enable_now(unit: &str) -> Command {
    systemctl(&["enable", "--now", unit])
}

/// `systemctl --user disable --now <unit>`: stop it and don't start it again.
pub fn disable_now(unit: &str) -> Command {
    systemctl(&["disable", "--now", unit])
}

/// `systemctl --user show <unit> --property=LoadState,ActiveState,MainPID`.
pub fn show(unit: &str) -> Command {
    systemctl(&["show", unit, "--property=LoadState,ActiveState,MainPID"])
}

/// Parses [`show`] output (`KEY=value` lines). An unknown unit reports
/// `LoadState=not-found`; `MainPID=0` means no process.
pub fn parse_show(output: &str) -> AgentState {
    let value = |key: &str| {
        output
            .lines()
            .find_map(|line| line.trim().strip_prefix(key)?.strip_prefix('='))
    };
    let loaded = matches!(value("LoadState"), Some(state) if state != "not-found");
    let active = value("ActiveState") == Some("active");
    let pid = value("MainPID")
        .and_then(|pid| pid.parse::<u32>().ok())
        .filter(|pid| *pid != 0 && active);
    AgentState { loaded, pid }
}

/// The directory user units live in, under `home`.
pub fn units_dir(home: &Path) -> std::path::PathBuf {
    home.join(".config/systemd/user")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::TokenSource;

    #[test]
    fn renders_the_unit() {
        let spec = ServiceSpec::new(
            "6ff42ae2",
            &crate::service::tests::run(
                TokenSource::File(PathBuf::from(
                    "/home/me/.local/share/teitunnel/tokens/6ff42ae2",
                )),
                "/home/me/.local/share/teitunnel/bin/cloudflared",
            ),
            PathBuf::from("/home/me/.local/state/teitunnel/connectors/6ff42ae2.log"),
        )
        .unwrap();
        assert_eq!(
            spec.unit_name(),
            "com.teispace.teitunnel.connector.6ff42ae2.service"
        );
        let unit = spec.unit();
        assert!(unit.contains("\"--token-file\""));
        assert!(!unit.contains("TUNNEL_TOKEN"));
        insta::assert_snapshot!(unit);
    }

    #[test]
    fn quotes_what_systemd_would_expand() {
        assert_eq!(quote(OsStr::new("a b")), r#""a b""#);
        assert_eq!(quote(OsStr::new(r#"$HOME\"x"#)), r#""\$HOME\\\"x""#);
        assert_eq!(quote(OsStr::new("100%")), r#""100%%""#);
    }

    #[test]
    fn builds_systemctl_invocations() {
        let args: Vec<_> = enable_now("x.service")
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["--user", "enable", "--now", "x.service"]);
    }

    #[test]
    fn parses_show_output() {
        assert_eq!(
            parse_show("LoadState=loaded\nActiveState=active\nMainPID=4242\n"),
            AgentState {
                loaded: true,
                pid: Some(4242)
            }
        );
        assert_eq!(
            parse_show("LoadState=loaded\nActiveState=activating\nMainPID=0\n"),
            AgentState {
                loaded: true,
                pid: None
            }
        );
        assert_eq!(
            parse_show("LoadState=not-found\nActiveState=inactive\nMainPID=0\n"),
            AgentState::default()
        );
    }
}
