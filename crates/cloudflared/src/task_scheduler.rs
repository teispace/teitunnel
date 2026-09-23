//! Always-on connectors on Windows: a scheduled task per tunnel (a [`ServiceSpec`] as
//! Task Scheduler XML, started at the user's logon and restarted on failure), and typed
//! `schtasks` invocations (discrete arguments, never a shell).
//!
//! Task Scheduler can't redirect a task's output, so on Windows the connector command
//! must write its own log (`RunCmd::log_dir`); `ServiceSpec::log_file` is where the app
//! reads it. `schtasks /XML` reads UTF-16 files reliably, hence [`ServiceSpec::task_file`].

use std::{ffi::OsStr, path::Path, process::Stdio};

use tokio::process::Command;

use crate::service::{AgentState, ServiceSpec, xml_escape};

/// The Task Scheduler folder Teitunnel's tasks live in.
pub const FOLDER: &str = r"\Teitunnel\";

impl ServiceSpec {
    /// The task's full name, e.g. `\Teitunnel\com.teispace.teitunnel.connector.<id>`.
    pub fn task_name(&self) -> String {
        format!("{FOLDER}{}", self.label)
    }

    /// The task definition for `user` (e.g. `DESKTOP-1\me`): runs at their logon with
    /// their rights, restarts on failure, never times out or stops on battery.
    pub fn task_xml(&self, user: &str) -> String {
        let arguments = self
            .args
            .iter()
            .map(|a| quote(a))
            .collect::<Vec<_>>()
            .join(" ");
        format!(
            r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Teitunnel connector ({label})</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <RestartOnFailure>
      <Interval>PT1M</Interval>
      <Count>999</Count>
    </RestartOnFailure>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#,
            label = xml_escape(&self.label),
            user = xml_escape(user),
            command = xml_escape(&self.program.to_string_lossy()),
            arguments = xml_escape(&arguments),
        )
    }

    /// [`Self::task_xml`] as a UTF-16LE file with a byte-order mark, for `schtasks /XML`.
    pub fn task_file(&self, user: &str) -> Vec<u8> {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in self.task_xml(user).encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }
}

/// Quotes one argument the way `CommandLineToArgvW` splits them: quoted when it has
/// whitespace or quotes (or is empty); backslashes doubled only before a quote.
fn quote(arg: &OsStr) -> String {
    let text = arg.to_string_lossy();
    if !text.is_empty() && !text.contains([' ', '\t', '"']) {
        return text.into_owned();
    }
    let mut out = String::from('"');
    let mut backslashes = 0;
    for c in text.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2 + 1));
                out.push('"');
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

fn schtasks(args: &[&str]) -> Command {
    let mut command = Command::new("schtasks");
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// `schtasks /Create /TN <name> /XML <file> /F`: create (or replace) the task.
pub fn create(name: &str, xml: &Path) -> Command {
    let path = xml.to_string_lossy();
    schtasks(&["/Create", "/TN", name, "/XML", &path, "/F"])
}

/// `schtasks /Run /TN <name>`: start it now.
pub fn run(name: &str) -> Command {
    schtasks(&["/Run", "/TN", name])
}

/// `schtasks /End /TN <name>`: stop it.
pub fn end(name: &str) -> Command {
    schtasks(&["/End", "/TN", name])
}

/// `schtasks /Delete /TN <name> /F`: remove it.
pub fn delete(name: &str) -> Command {
    schtasks(&["/Delete", "/TN", name, "/F"])
}

/// `schtasks /Query /TN <name> /FO LIST`: its status (fails if unknown).
pub fn query(name: &str) -> Command {
    schtasks(&["/Query", "/TN", name, "/FO", "LIST"])
}

/// Parses [`query`] output. Task Scheduler doesn't report a process id, so a running
/// task has none here; health comes from the connector's `/ready` anyway.
pub fn parse_query(output: &str) -> AgentState {
    AgentState {
        loaded: output
            .lines()
            .any(|l| l.trim_start().starts_with("TaskName:")),
        pid: None,
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::TokenSource;

    fn spec() -> ServiceSpec {
        ServiceSpec::new(
            "6ff42ae2",
            &crate::service::tests::run(
                TokenSource::File(PathBuf::from(
                    r"C:\Users\me\AppData\Roaming\com.teispace.teitunnel\tokens\6ff42ae2",
                )),
                r"C:\Users\me\AppData\Roaming\com.teispace.teitunnel\bin\cloudflared.exe",
            ),
            PathBuf::from(r"C:\Users\me\AppData\Local\com.teispace.teitunnel\logs\6ff42ae2.log"),
        )
        .unwrap()
    }

    #[test]
    fn renders_the_task() {
        let spec = spec();
        assert_eq!(
            spec.task_name(),
            r"\Teitunnel\com.teispace.teitunnel.connector.6ff42ae2"
        );
        let xml = spec.task_xml(r"DESKTOP-1\me");
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
        assert!(!xml.contains("TUNNEL_TOKEN"));
        insta::assert_snapshot!(xml);
        let file = spec.task_file(r"DESKTOP-1\me");
        assert_eq!(&file[..2], &[0xFF, 0xFE]);
        assert_eq!(file.len(), 2 + xml.encode_utf16().count() * 2);
    }

    #[test]
    fn quotes_like_command_line_to_argv() {
        let q = |s: &str| quote(OsStr::new(s));
        assert_eq!(q("plain"), "plain");
        assert_eq!(q(""), r#""""#);
        assert_eq!(q(r"C:\Program Files\x"), r#""C:\Program Files\x""#);
        assert_eq!(q(r#"say "hi""#), r#""say \"hi\"""#);
        assert_eq!(q(r"dir with space\"), r#""dir with space\\""#);
        assert_eq!(q(r"a\b"), r"a\b");
    }

    #[test]
    fn builds_schtasks_invocations_and_reads_status() {
        let args: Vec<_> = delete(r"\Teitunnel\x")
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, ["/Delete", "/TN", r"\Teitunnel\x", "/F"]);
        assert!(parse_query("\r\nFolder: \\Teitunnel\r\nHostName: PC\r\nTaskName: \\Teitunnel\\x\r\nStatus: Running\r\n").loaded);
        assert!(!parse_query("").loaded);
    }
}
