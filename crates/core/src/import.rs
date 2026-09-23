//! Existing cloudflared setups on this Mac (`config.yml` + credentials), so their routes
//! can be brought into Teitunnel. Only reads: files are never changed, and a
//! credentials file's secret is never read into memory (only its account and tunnel id).

use std::path::{Path, PathBuf};

use cf_api::IngressRule;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::text::{Text, msg};
use crate::{domain::RouteOrigin, engine::RouteInput};

/// Where cloudflared looks for its configuration.
fn directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        dirs.push(home.join(".cloudflared"));
    }
    dirs.extend(
        [
            "/etc/cloudflared",
            "/usr/local/etc/cloudflared",
            "/opt/homebrew/etc/cloudflared",
        ]
        .map(PathBuf::from),
    );
    dirs
}

/// One route found in a config file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FoundRoute {
    /// Hostname.
    pub hostname: String,
    /// Path regex.
    pub path: Option<String>,
    /// Service, e.g. `http://localhost:3000`.
    pub service: String,
    /// Why it can't be imported, if it can't.
    pub unsupported: Option<Text>,
}

impl FoundRoute {
    /// The route as it would be added.
    pub fn to_input(&self) -> RouteInput {
        RouteInput {
            hostname: self.hostname.clone(),
            path: self.path.clone(),
            origin: self.service.clone(),
            access: None,
        }
    }
}

/// A cloudflared configuration found on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocalSetup {
    /// The config file.
    pub config_path: String,
    /// `tunnel:` (a UUID or a name).
    pub tunnel: Option<String>,
    /// The Cloudflare account the credentials belong to.
    pub account_id: Option<String>,
    /// The tunnel id from the credentials file.
    pub tunnel_id: Option<String>,
    /// Routes (hostname rules) in order.
    pub routes: Vec<FoundRoute>,
    /// Settings that apply to every route (`originRequest`), which Teitunnel can't
    /// carry over per route yet.
    pub has_global_options: bool,
    /// Problems reading the file.
    pub problem: Option<Text>,
}

#[derive(Deserialize)]
struct ConfigFile {
    #[serde(default)]
    tunnel: Option<Value>,
    #[serde(rename = "credentials-file", default)]
    credentials_file: Option<String>,
    #[serde(default)]
    ingress: Vec<IngressRule>,
    #[serde(rename = "originRequest", default)]
    origin_request: serde_json::Map<String, Value>,
}

/// Only the non-secret fields of a credentials file.
#[derive(Deserialize)]
struct Credentials {
    #[serde(rename = "AccountTag", default)]
    account_tag: Option<String>,
    #[serde(rename = "TunnelID", default)]
    tunnel_id: Option<String>,
}

fn expand(path: &str, base: &Path) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = std::env::var_os("HOME")
    {
        return PathBuf::from(home).join(rest);
    }
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn route(rule: &IngressRule) -> Option<FoundRoute> {
    let hostname = rule.hostname.clone()?;
    let unsupported = if hostname.contains('*') && !hostname.starts_with("*.") {
        Some(msg::import::wildcard())
    } else if RouteOrigin::parse(&rule.service).is_err() {
        Some(msg::import::service(&rule.service))
    } else if !rule.origin_request.is_empty() {
        Some(msg::import::origin_request())
    } else {
        None
    };
    Some(FoundRoute {
        hostname,
        path: rule.path.clone(),
        service: rule.service.clone(),
        unsupported,
    })
}

/// Parses one config file.
pub fn read_setup(path: &Path) -> LocalSetup {
    let mut setup = LocalSetup {
        config_path: path.display().to_string(),
        tunnel: None,
        account_id: None,
        tunnel_id: None,
        routes: Vec::new(),
        has_global_options: false,
        problem: None,
    };
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) => {
            setup.problem = Some(msg::import::unreadable(err));
            return setup;
        }
    };
    let config: ConfigFile = match serde_saphyr::from_str(&text) {
        Ok(config) => config,
        Err(err) => {
            setup.problem = Some(msg::import::invalid_yaml(err));
            return setup;
        }
    };
    setup.tunnel = config.tunnel.map(|t| match t {
        Value::String(s) => s,
        other => other.to_string(),
    });
    setup.has_global_options = !config.origin_request.is_empty();
    setup.routes = config.ingress.iter().filter_map(route).collect();
    let base = path.parent().unwrap_or(Path::new("/"));
    let credentials = config
        .credentials_file
        .map(|file| expand(&file, base))
        .or_else(|| {
            let id = setup.tunnel.as_deref()?;
            Some(base.join(format!("{id}.json")))
        });
    if let Some(file) = credentials
        && let Ok(text) = std::fs::read_to_string(&file)
        && let Ok(creds) = serde_json::from_str::<Credentials>(&text)
    {
        setup.account_id = creds.account_tag;
        setup.tunnel_id = creds.tunnel_id;
    }
    setup
}

/// Every cloudflared configuration in the usual places (blocking file reads).
pub fn scan() -> Vec<LocalSetup> {
    scan_in(&directories())
}

pub(crate) fn scan_in(dirs: &[PathBuf]) -> Vec<LocalSetup> {
    dirs.iter()
        .flat_map(|dir| ["config.yml", "config.yaml"].map(|name| dir.join(name)))
        .filter(|path| path.is_file())
        .map(|path| read_setup(&path))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"
tunnel: 2b8a3f54-0c0d-4c1e-9f7a-1d2c3b4a5e6f
credentials-file: creds.json
warp-routing:
  enabled: false
ingress:
  - hostname: app.xyz.com
    service: http://localhost:3000
  - hostname: api.xyz.com
    path: ^/v1/
    service: http://localhost:8080
  - hostname: ssh.xyz.com
    service: ssh://localhost:22
  - hostname: tls.xyz.com
    service: https://localhost:8443
    originRequest:
      noTLSVerify: true
  - hostname: hi.xyz.com
    service: hello_world
  - service: http_status:404
"#;

    #[test]
    fn reads_routes_and_non_secret_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.yml");
        std::fs::write(&config, CONFIG).unwrap();
        std::fs::write(
            dir.path().join("creds.json"),
            r#"{"AccountTag":"acc123","TunnelSecret":"c2VjcmV0","TunnelID":"2b8a3f54-0c0d-4c1e-9f7a-1d2c3b4a5e6f"}"#,
        )
        .unwrap();
        let setups = scan_in(&[dir.path().to_path_buf()]);
        assert_eq!(setups.len(), 1);
        let setup = &setups[0];
        assert_eq!(setup.problem, None);
        assert_eq!(setup.account_id.as_deref(), Some("acc123"));
        assert_eq!(
            setup.tunnel_id.as_deref(),
            Some("2b8a3f54-0c0d-4c1e-9f7a-1d2c3b4a5e6f")
        );
        let names: Vec<_> = setup.routes.iter().map(|r| r.hostname.as_str()).collect();
        assert_eq!(
            names,
            [
                "app.xyz.com",
                "api.xyz.com",
                "ssh.xyz.com",
                "tls.xyz.com",
                "hi.xyz.com"
            ]
        );
        assert_eq!(setup.routes[1].path.as_deref(), Some("^/v1/"));
        assert_eq!(setup.routes[0].unsupported, None);
        assert_eq!(
            setup.routes[2].unsupported, None,
            "ssh origins can be routed"
        );
        assert!(setup.routes[3].unsupported.is_some(), "per-route settings");
        assert_eq!(
            setup.routes[4].unsupported, None,
            "cloudflared's built-in hello_world"
        );
        let json = serde_json::to_string(&setups).unwrap();
        assert!(
            !json.contains("c2VjcmV0"),
            "the secret never leaves the file"
        );
    }

    #[test]
    fn reports_unreadable_configs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), "ingress: [ {").unwrap();
        let setups = scan_in(&[dir.path().to_path_buf()]);
        assert!(
            setups[0]
                .problem
                .as_ref()
                .unwrap()
                .english()
                .starts_with("It isn't valid YAML")
        );
        assert!(scan_in(&[dir.path().join("missing")]).is_empty());
    }
}
