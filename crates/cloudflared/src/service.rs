//! Always-on connectors as OS services, platform-neutral: what runs ([`ServiceSpec`])
//! and what the service manager reports ([`AgentState`]). Each platform renders the
//! spec its own way: [`crate::launchd`] (macOS), [`crate::systemd`] (Linux, user
//! units) and [`crate::task_scheduler`] (Windows).
//!
//! A service can't read the keychain, so it reads its token from a file; a spec that
//! would put the token in the environment (and so in a plist, unit or task definition
//! on disk) is refused.

use std::{ffi::OsString, path::PathBuf};

use crate::CommandSpec;

/// Name prefix of Teitunnel's services.
pub const LABEL_PREFIX: &str = "com.teispace.teitunnel.connector.";

/// The service name for a tunnel's connector.
pub fn label(tunnel_id: &str) -> String {
    format!("{LABEL_PREFIX}{tunnel_id}")
}

/// Why a service can't be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    /// The command carries its token in the environment, which the service definition
    /// would store.
    #[error("always-on connectors must read their token from a file")]
    TokenInEnvironment,
}

/// A service that runs one connector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    /// `com.teispace.teitunnel.connector.<tunnel-id>`.
    pub label: String,
    /// The cloudflared binary.
    pub program: PathBuf,
    /// Its arguments.
    pub args: Vec<OsString>,
    /// Where cloudflared's JSON log goes.
    pub log_file: PathBuf,
}

impl ServiceSpec {
    /// A service for `tunnel_id` running `command` (built with `TokenSource::File`).
    ///
    /// # Errors
    /// [`AgentError::TokenInEnvironment`] if the command passes its token by environment.
    pub fn new(
        tunnel_id: &str,
        command: &CommandSpec,
        log_file: PathBuf,
    ) -> Result<Self, AgentError> {
        if !command.env_names().is_empty() {
            return Err(AgentError::TokenInEnvironment);
        }
        Ok(Self {
            label: label(tunnel_id),
            program: command.program().to_path_buf(),
            args: command.args().to_vec(),
            log_file,
        })
    }
}

/// What a service manager says about a service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgentState {
    /// The manager knows the service.
    pub loaded: bool,
    /// Its process, when running.
    pub pid: Option<u32>,
}

/// Escapes text for XML content and attribute values (plists, task definitions).
pub(crate) fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::{LogLevel, Protocol, RunCmd, TokenSource, TunnelToken};

    pub(crate) fn run(token: TokenSource, program: &str) -> CommandSpec {
        RunCmd {
            token,
            metrics_port: 20301,
            protocol: Protocol::Auto,
            log_level: LogLevel::Info,
            log_dir: None,
        }
        .build(Path::new(program))
    }

    #[test]
    fn refuses_tokens_in_the_environment() {
        let err = ServiceSpec::new(
            "t",
            &run(
                TokenSource::Env(TunnelToken::new("secret".into())),
                "/bin/cloudflared",
            ),
            PathBuf::from("/tmp/log"),
        )
        .unwrap_err();
        assert_eq!(err, AgentError::TokenInEnvironment);
    }
}
