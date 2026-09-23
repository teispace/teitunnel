//! Always-on connectors on macOS: a launchd agent per tunnel (a [`ServiceSpec`] as a
//! plist), and typed `launchctl` invocations (discrete arguments, never a shell).
//!
//! The agent runs the same `cloudflared tunnel run` Teitunnel builds for Session mode,
//! but with `--token-file` (a launchd job can't read the keychain), and its JSON log goes
//! to a file the app tails.

use std::{ffi::OsString, fmt::Write, path::Path, process::Stdio};

use tokio::process::Command;

pub use crate::service::{AgentError, AgentState, LABEL_PREFIX, label};
use crate::service::{ServiceSpec, xml_escape as escape};

impl ServiceSpec {
    /// The plist file name.
    pub fn plist_file_name(&self) -> String {
        format!("{}.plist", self.label)
    }

    /// The property list: started at login, kept alive, low priority.
    pub fn plist(&self) -> String {
        let mut arguments = String::new();
        for arg in std::iter::once(self.program.as_os_str())
            .chain(self.args.iter().map(OsString::as_os_str))
        {
            let _ = writeln!(
                arguments,
                "\t\t<string>{}</string>",
                escape(&arg.to_string_lossy())
            );
        }
        let log = escape(&self.log_file.to_string_lossy());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{label}</string>
	<key>ProgramArguments</key>
	<array>
{arguments}	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<true/>
	<key>ProcessType</key>
	<string>Background</string>
	<key>ThrottleInterval</key>
	<integer>10</integer>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#,
            label = escape(&self.label),
        )
    }
}

/// The per-user launchd domain, e.g. `gui/501`.
pub fn gui_domain(uid: u32) -> String {
    format!("gui/{uid}")
}

fn launchctl(args: &[&str]) -> Command {
    let mut command = Command::new("/bin/launchctl");
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// `launchctl bootstrap gui/<uid> <plist>`: load and start an agent.
pub fn bootstrap(domain: &str, plist: &Path) -> Command {
    let path = plist.to_string_lossy();
    launchctl(&["bootstrap", domain, &path])
}

/// `launchctl bootout gui/<uid>/<label>`: stop and unload an agent.
pub fn bootout(domain: &str, label: &str) -> Command {
    launchctl(&["bootout", &format!("{domain}/{label}")])
}

/// `launchctl print gui/<uid>/<label>`: an agent's state (exit status 113 if unknown).
pub fn print(domain: &str, label: &str) -> Command {
    launchctl(&["print", &format!("{domain}/{label}")])
}

/// Parses `launchctl print` output (`state = running`, `pid = 1234`).
pub fn parse_print(output: &str) -> AgentState {
    let mut state = AgentState {
        loaded: !output.trim().is_empty(),
        pid: None,
    };
    for line in output.lines().map(str::trim) {
        if let Some(pid) = line.strip_prefix("pid = ") {
            state.pid = pid.trim().parse().ok();
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::TokenSource;

    fn run(token: TokenSource) -> crate::CommandSpec {
        crate::service::tests::run(
            token,
            "/Users/me/Library/Application Support/com.teispace.teitunnel/bin/cloudflared",
        )
    }

    #[test]
    fn renders_the_agent() {
        let agent = ServiceSpec::new(
            "6ff42ae2",
            &run(TokenSource::File(PathBuf::from(
                "/Users/me/Library/Application Support/com.teispace.teitunnel/tokens/6ff42ae2",
            ))),
            PathBuf::from("/Users/me/Library/Logs/com.teispace.teitunnel/connectors/6ff42ae2.log"),
        )
        .unwrap();
        assert_eq!(
            agent.plist_file_name(),
            "com.teispace.teitunnel.connector.6ff42ae2.plist"
        );
        let plist = agent.plist();
        assert!(plist.contains("<string>com.teispace.teitunnel.connector.6ff42ae2</string>"));
        assert!(plist.contains("<string>--token-file</string>"));
        assert!(plist.contains("<string>127.0.0.1:20301</string>"));
        assert!(plist.contains("<key>KeepAlive</key>\n\t<true/>"));
        assert!(
            !plist.contains("TUNNEL_TOKEN"),
            "no token in the environment"
        );
        insta::assert_snapshot!(plist);
    }

    #[test]
    fn builds_launchctl_invocations() {
        let domain = gui_domain(501);
        let cmd = bootout(&domain, &label("t1"));
        let args: Vec<_> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            ["bootout", "gui/501/com.teispace.teitunnel.connector.t1"]
        );
        assert_eq!(cmd.as_std().get_program(), "/bin/launchctl");
    }

    #[test]
    fn parses_print_output() {
        let running = "gui/501/com.teispace.teitunnel.connector.t1 = {\n\tactive count = 1\n\tstate = running\n\tpid = 4242\n}";
        assert_eq!(
            parse_print(running),
            AgentState {
                loaded: true,
                pid: Some(4242)
            }
        );
        assert_eq!(parse_print(""), AgentState::default());
    }
}
