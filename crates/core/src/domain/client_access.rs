//! How a visitor reaches a non-HTTP route: SSH, RDP, SMB and raw TCP go through
//! `cloudflared access` on the visitor's computer, which opens a local port (or, for
//! SSH, acts as the proxy command) and carries the traffic to the hostname.
//!
//! Commands per Cloudflare's "Connect with cloudflared access" guides (SSH, RDP, SMB,
//! arbitrary TCP), checked 2026-09-23.

use serde::Serialize;

use super::{Hostname, RouteOrigin};

/// The protocol a non-HTTP route carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ClientProtocol {
    /// SSH.
    Ssh,
    /// Remote Desktop.
    Rdp,
    /// Windows file sharing.
    Smb,
    /// Any TCP service (a database, …).
    Tcp,
}

/// What a visitor runs to reach a non-HTTP route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ClientAccess {
    /// The protocol.
    pub protocol: ClientProtocol,
    /// The command to run (SSH: to connect; others: to open the local port).
    pub command: String,
    /// Where the visitor's app connects once the command runs (`None` for SSH).
    pub local_address: Option<String>,
    /// An `~/.ssh/config` entry, so plain `ssh <hostname>` works (SSH only).
    pub ssh_config: Option<String>,
}

/// SMB's port is taken on most Macs and Windows PCs; Cloudflare's guide uses this one.
const SMB_LOCAL_PORT: u16 = 8445;

impl ClientAccess {
    /// How to reach `hostname`, served by `origin`; `None` for HTTP(S) and other origins
    /// a browser opens directly.
    pub fn of(hostname: &Hostname, origin: &RouteOrigin) -> Option<Self> {
        let (scheme, _) = origin.as_str().split_once("://")?;
        let host = hostname.as_str();
        let local = |port: u16| format!("localhost:{port}");
        let port = origin.port();
        Some(match scheme {
            "ssh" => Self {
                protocol: ClientProtocol::Ssh,
                command: format!(
                    "ssh -o ProxyCommand=\"cloudflared access ssh --hostname %h\" {host}"
                ),
                local_address: None,
                ssh_config: Some(format!(
                    "Host {host}\n  ProxyCommand cloudflared access ssh --hostname %h"
                )),
            },
            "rdp" => {
                let address = local(port.unwrap_or(3389));
                Self {
                    protocol: ClientProtocol::Rdp,
                    command: format!(
                        "cloudflared access rdp --hostname {host} --url rdp://{address}"
                    ),
                    local_address: Some(address),
                    ssh_config: None,
                }
            }
            "smb" => {
                let address = local(SMB_LOCAL_PORT);
                Self {
                    protocol: ClientProtocol::Smb,
                    command: format!("cloudflared access smb --hostname {host} --url {address}"),
                    local_address: Some(address),
                    ssh_config: None,
                }
            }
            "tcp" => {
                let address = local(port?);
                Self {
                    protocol: ClientProtocol::Tcp,
                    command: format!("cloudflared access tcp --hostname {host} --url {address}"),
                    local_address: Some(address),
                    ssh_config: None,
                }
            }
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access(host: &str, origin: &str) -> Option<ClientAccess> {
        ClientAccess::of(
            &Hostname::parse(host).unwrap(),
            &RouteOrigin::parse(origin).unwrap(),
        )
    }

    #[test]
    fn web_routes_need_nothing() {
        assert_eq!(access("app.xyz.com", "3000"), None);
        assert_eq!(access("app.xyz.com", "https://localhost:8443"), None);
        assert_eq!(access("app.xyz.com", "unix:/tmp/app.sock"), None);
    }

    #[test]
    fn each_protocol_gets_its_command() {
        let ssh = access("ssh.xyz.com", "ssh://localhost:22").unwrap();
        assert_eq!(
            ssh.command,
            "ssh -o ProxyCommand=\"cloudflared access ssh --hostname %h\" ssh.xyz.com"
        );
        assert_eq!(
            ssh.ssh_config.as_deref(),
            Some("Host ssh.xyz.com\n  ProxyCommand cloudflared access ssh --hostname %h")
        );
        let rdp = access("pc.xyz.com", "rdp://10.0.0.5:3389").unwrap();
        assert_eq!(
            rdp.command,
            "cloudflared access rdp --hostname pc.xyz.com --url rdp://localhost:3389"
        );
        assert_eq!(rdp.local_address.as_deref(), Some("localhost:3389"));
        let db = access("db.xyz.com", "tcp://localhost:5432").unwrap();
        assert_eq!(
            db.command,
            "cloudflared access tcp --hostname db.xyz.com --url localhost:5432"
        );
        let smb = access("files.xyz.com", "smb://localhost:445").unwrap();
        assert_eq!(smb.local_address.as_deref(), Some("localhost:8445"));
    }
}
