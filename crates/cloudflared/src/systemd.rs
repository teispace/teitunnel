//! Always-on connectors on Linux: a systemd unit per tunnel (a [`ServiceSpec`] as a
//! `.service` file), and typed `systemctl` invocations (discrete arguments, never a
//! shell). On a desktop they're user units (`~/.config/systemd/user`, started with the
//! user's session); on a server, run as root, system units (`/etc/systemd/system`,
//! started at boot, sandboxed).
//!
//! User units stop at logout unless lingering is enabled (`loginctl enable-linger`);
//! Teitunnel says so rather than enabling it. `StandardOutput=append:` needs systemd
//! 240 or newer (2018).

use std::{ffi::OsStr, path::Path, process::Stdio};

use tokio::process::Command;

use crate::service::{AgentState, ServiceSpec};

/// Which systemd instance a unit belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The user's manager (`systemctl --user`), started with their session.
    User,
    /// The system manager, started at boot (servers; needs root).
    System,
}

/// Where system units are written.
pub const SYSTEM_UNITS_DIR: &str = "/etc/systemd/system";

impl ServiceSpec {
    /// The unit file name, e.g. `com.teispace.teitunnel.connector.<id>.service`.
    pub fn unit_name(&self) -> String {
        format!("{}.service", self.label)
    }

    /// The user unit: restarted when it exits, started with the user's session, low
    /// priority.
    pub fn unit(&self) -> String {
        self.unit_for(Scope::User)
    }

    /// The unit for `scope`. A system unit also starts at boot and runs sandboxed: no new
    /// privileges, a read-only system and no home directories, writing only its log.
    pub fn unit_for(&self, scope: Scope) -> String {
        let exec = std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(std::ffi::OsString::as_os_str))
            .map(quote)
            .collect::<Vec<_>>()
            .join(" ");
        let log = self.log_file.to_string_lossy().replace('%', "%%");
        let (sandbox, wanted_by) = match scope {
            Scope::User => (String::new(), "default.target"),
            Scope::System => {
                let dir = self
                    .log_file
                    .parent()
                    .map(|d| quote(d.as_os_str()))
                    .unwrap_or_default();
                (
                    format!(
                        "NoNewPrivileges=yes
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=yes
PrivateDevices=yes
ProtectKernelTunables=yes
ProtectKernelModules=yes
ProtectControlGroups=yes
RestrictSUIDSGID=yes
LockPersonality=yes
ReadWritePaths={dir}
"
                    ),
                    "multi-user.target",
                )
            }
        };
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
{sandbox}
[Install]
WantedBy={wanted_by}
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

fn systemctl(scope: Scope, args: &[&str]) -> Command {
    let mut command = Command::new("systemctl");
    crate::process::no_console(&mut command)
        .args(match scope {
            Scope::User => &["--user"][..],
            Scope::System => &[][..],
        })
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// `systemctl [--user] daemon-reload`: pick up written or removed unit files.
pub fn daemon_reload(scope: Scope) -> Command {
    systemctl(scope, &["daemon-reload"])
}

/// `systemctl [--user] enable --now <unit>`: start it, and at every login (or boot).
pub fn enable_now(scope: Scope, unit: &str) -> Command {
    systemctl(scope, &["enable", "--now", unit])
}

/// `systemctl [--user] disable --now <unit>`: stop it and don't start it again.
pub fn disable_now(scope: Scope, unit: &str) -> Command {
    systemctl(scope, &["disable", "--now", unit])
}

/// `systemctl [--user] show <unit> --property=LoadState,ActiveState,MainPID`.
pub fn show(scope: Scope, unit: &str) -> Command {
    systemctl(
        scope,
        &["show", unit, "--property=LoadState,ActiveState,MainPID"],
    )
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

        let system = spec.unit_for(Scope::System);
        assert!(system.contains("WantedBy=multi-user.target"));
        assert!(system.contains("NoNewPrivileges=yes"));
        assert!(system.contains("ProtectSystem=strict"));
        assert!(
            system.contains(r#"ReadWritePaths="/home/me/.local/state/teitunnel/connectors""#),
            "{system}"
        );
    }

    #[test]
    fn quotes_what_systemd_would_expand() {
        assert_eq!(quote(OsStr::new("a b")), r#""a b""#);
        assert_eq!(quote(OsStr::new(r#"$HOME\"x"#)), r#""\$HOME\\\"x""#);
        assert_eq!(quote(OsStr::new("100%")), r#""100%%""#);
    }

    #[test]
    fn builds_systemctl_invocations() {
        let args = |command: Command| -> Vec<String> {
            command
                .as_std()
                .get_args()
                .map(|a| a.to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            args(enable_now(Scope::User, "x.service")),
            ["--user", "enable", "--now", "x.service"]
        );
        assert_eq!(
            args(enable_now(Scope::System, "x.service")),
            ["enable", "--now", "x.service"]
        );
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
