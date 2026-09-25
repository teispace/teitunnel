//! Existing cloudflared setups on this Mac (`config.yml` + credentials), so their routes
//! can be brought into Teitunnel. Only reads: files are never changed, and a
//! credentials file's secret is never read into memory (only its account and tunnel id).

use std::path::{Path, PathBuf};

use cf_api::IngressRule;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::text::{Text, msg};
use crate::{
    domain::{OriginOptions, RouteOrigin},
    engine::RouteInput,
};

/// Where cloudflared itself looks for `config.yml` (its default search directories), plus
/// Homebrew's and, on Windows, the folder its service runs from.
fn directories() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(home) = std::env::home_dir() {
        for name in [".cloudflared", ".cloudflare-warp", "cloudflare-warp"] {
            dirs.push(home.join(name));
        }
    }
    #[cfg(unix)]
    dirs.extend(
        [
            "/etc/cloudflared",
            "/usr/local/etc/cloudflared",
            "/opt/homebrew/etc/cloudflared",
        ]
        .map(PathBuf::from),
    );
    #[cfg(windows)]
    if let Some(root) = std::env::var_os("SystemRoot") {
        dirs.push(PathBuf::from(root).join(r"System32\config\systemprofile\.cloudflared"));
    }
    dirs
}

/// Config files are small; anything bigger isn't one (and isn't read into memory).
const MAX_FILE: u64 = 1024 * 1024;

fn read_small(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(MAX_FILE + 1)
        .read_to_string(&mut text)?;
    if text.len() as u64 > MAX_FILE {
        return Err(std::io::Error::other("larger than 1 MB"));
    }
    Ok(text)
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
    /// Its origin settings (the file's own `originRequest` merged with the rule's).
    pub options: OriginOptions,
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
            options: Some(Box::new(self.options.clone())),
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
    /// The file has settings for every route (`originRequest`) that Teitunnel can't
    /// carry over (the ones it can are merged into each route).
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
        && let Some(home) = std::env::home_dir()
    {
        return home.join(rest);
    }
    let path = PathBuf::from(path);
    if path.is_absolute() {
        path
    } else {
        base.join(path)
    }
}

fn route(rule: &IngressRule, global: &serde_json::Map<String, Value>) -> Option<FoundRoute> {
    let hostname = rule.hostname.clone()?;
    // As cloudflared does: the file's settings apply to every rule, which can override them.
    let mut settings = global.clone();
    settings.extend(rule.origin_request.clone());
    let unknown = OriginOptions::unknown_keys(&settings);
    let unsupported = if hostname.contains('*') && !hostname.starts_with("*.") {
        Some(msg::import::wildcard())
    } else if RouteOrigin::parse(&rule.service).is_err() {
        Some(msg::import::service(&rule.service))
    } else if !unknown.is_empty() {
        Some(msg::import::origin_request(unknown.join(", ")))
    } else {
        None
    };
    Some(FoundRoute {
        hostname,
        path: rule.path.clone(),
        service: rule.service.clone(),
        options: OriginOptions::from_map(&settings),
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
    let text = match read_small(path) {
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
    setup.has_global_options = !OriginOptions::unknown_keys(&config.origin_request).is_empty();
    setup.routes = config
        .ingress
        .iter()
        .filter_map(|rule| route(rule, &config.origin_request))
        .collect();
    let base = path.parent().unwrap_or(Path::new("/"));
    let credentials = config
        .credentials_file
        .map(|file| expand(&file, base))
        .or_else(|| {
            let id = setup.tunnel.as_deref()?;
            Some(base.join(format!("{id}.json")))
        });
    if let Some(file) = credentials
        && let Ok(text) = read_small(&file)
        && let Ok(creds) = serde_json::from_str::<Credentials>(&text)
    {
        setup.account_id = creds.account_tag;
        setup.tunnel_id = creds.tunnel_id;
    }
    setup
}

/// Every cloudflared configuration in the usual places, and the files `extra` names (the
/// `--config` of cloudflared processes that are running). Blocking file reads.
pub fn scan(extra: &[PathBuf]) -> Vec<LocalSetup> {
    scan_in(&directories(), extra)
}

pub(crate) fn scan_in(dirs: &[PathBuf], extra: &[PathBuf]) -> Vec<LocalSetup> {
    let mut seen = std::collections::HashSet::new();
    let mut setups = Vec::new();
    let mut add = |path: &Path, known: bool| {
        let key = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if !path.is_file() || !seen.insert(key) {
            return;
        }
        let setup = read_setup(path);
        // `config.yml` and a running cloudflared's file are configs, so their problems are
        // worth showing; any other YAML file in the folder counts only if it has routes.
        if known || (setup.problem.is_none() && !setup.routes.is_empty()) {
            setups.push(setup);
        }
    };
    for path in extra {
        add(path, true);
    }
    for dir in dirs {
        for name in ["config.yml", "config.yaml"] {
            add(&dir.join(name), true);
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        let mut others: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|ext| ext == "yml" || ext == "yaml")
            })
            .collect();
        others.sort();
        for path in others {
            add(&path, false);
        }
    }
    setups
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
  - hostname: bastion.xyz.com
    service: bastion
    originRequest:
      bastionMode: true
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
        let setups = scan_in(&[dir.path().to_path_buf()], &[]);
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
                "bastion.xyz.com",
                "hi.xyz.com"
            ]
        );
        assert_eq!(setup.routes[1].path.as_deref(), Some("^/v1/"));
        assert_eq!(setup.routes[0].unsupported, None);
        assert_eq!(
            setup.routes[2].unsupported, None,
            "ssh origins can be routed"
        );
        assert_eq!(
            setup.routes[3].unsupported, None,
            "settings it knows carry over"
        );
        assert!(setup.routes[3].options.no_tls_verify);
        assert!(
            setup.routes[3].to_input().options.unwrap().no_tls_verify,
            "and are part of what's added"
        );
        assert!(
            setup.routes[4].unsupported.is_some(),
            "settings it doesn't know stop the import of that route"
        );
        assert_eq!(
            setup.routes[5].unsupported, None,
            "cloudflared's built-in hello_world"
        );
        let json = serde_json::to_string(&setups).unwrap();
        assert!(
            !json.contains("c2VjcmV0"),
            "the secret never leaves the file"
        );
    }

    #[test]
    fn applies_the_files_settings_to_every_route() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.yml"),
            "tunnel: t\noriginRequest:\n  connectTimeout: 30s\n  noTLSVerify: true\ningress:\n  - hostname: a.xyz.com\n    service: https://localhost:1\n  - hostname: b.xyz.com\n    service: https://localhost:2\n    originRequest:\n      noTLSVerify: false\n  - service: http_status:404\n",
        )
        .unwrap();
        let setup = &scan_in(&[dir.path().to_path_buf()], &[])[0];
        assert!(!setup.has_global_options, "everything in it carries over");
        assert_eq!(setup.routes[0].options.connect_timeout, Some(30));
        assert!(setup.routes[0].options.no_tls_verify);
        assert!(
            !setup.routes[1].options.no_tls_verify,
            "a rule overrides the file"
        );
        assert_eq!(setup.routes[1].options.connect_timeout, Some(30));
    }

    #[test]
    fn reports_unreadable_configs() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("config.yaml"), "ingress: [ {").unwrap();
        let setups = scan_in(&[dir.path().to_path_buf()], &[]);
        assert!(
            setups[0]
                .problem
                .as_ref()
                .unwrap()
                .english()
                .starts_with("It isn't valid YAML")
        );
        assert!(scan_in(&[dir.path().join("missing")], &[]).is_empty());
    }

    #[test]
    fn finds_configs_by_any_name_and_those_of_running_cloudflareds() {
        let dir = tempfile::tempdir().unwrap();
        let routes = "tunnel: t\ningress:\n  - hostname: a.xyz.com\n    service: http://localhost:1\n  - service: http_status:404\n";
        std::fs::write(dir.path().join("staging.yml"), routes).unwrap();
        std::fs::write(
            dir.path().join("docker-compose.yaml"),
            "services:\n  web: {}\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("broken.yml"), "ingress: [ {").unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        let running = elsewhere.path().join("tunnel.yml");
        std::fs::write(&running, routes).unwrap();

        let setups = scan_in(&[dir.path().to_path_buf()], &[running.clone(), running]);
        let files: Vec<_> = setups
            .iter()
            .map(|s| Path::new(&s.config_path).file_name().unwrap().to_owned())
            .collect();
        assert_eq!(
            files,
            ["tunnel.yml", "staging.yml"],
            "other YAML files only count when they are configs with routes; each file once"
        );
    }

    #[test]
    fn refuses_files_too_big_to_be_a_config() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.yml");
        std::fs::write(&config, "#".repeat(MAX_FILE as usize + 1)).unwrap();
        let setup = read_setup(&config);
        assert!(setup.problem.is_some());
        assert!(setup.routes.is_empty());
    }
}
