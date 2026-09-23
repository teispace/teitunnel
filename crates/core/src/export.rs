//! Export this Mac's tunnel as configuration for other tools: a cloudflared
//! `config.yml`, a Docker Compose service, or Terraform (Cloudflare provider v5) with
//! `import` blocks, so the setup can move to a server or into infrastructure as code.
//!
//! Pure rendering over what the engine observed. Secrets are never part of an export:
//! the Compose file reads the run token from `${TUNNEL_TOKEN}`, and the comments say how
//! to get it.

use std::collections::{HashMap, HashSet};

use cf_api::IngressRule;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// What to export as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    /// A cloudflared `config.yml` (ingress rules).
    ConfigYaml,
    /// A Docker Compose service running the tunnel.
    DockerCompose,
    /// Terraform for the Cloudflare provider v5, with `import` blocks.
    Terraform,
}

/// A rendered export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExportFile {
    /// Suggested file name, e.g. `config.yml`.
    pub file_name: String,
    /// The file.
    pub contents: String,
}

/// A DNS record pointing a hostname at the tunnel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportRecord {
    /// Zone id.
    pub zone_id: String,
    /// Record id.
    pub record_id: String,
    /// Hostname.
    pub hostname: String,
    /// Its comment (Teitunnel's ownership marker), kept so an import plans no change.
    pub comment: Option<String>,
    /// TTL in seconds (1: automatic).
    pub ttl: u32,
    /// Proxied through Cloudflare.
    pub proxied: bool,
}

/// Everything an export is made from.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportInput {
    /// Account id.
    pub account_id: String,
    /// Tunnel id.
    pub tunnel_id: String,
    /// Tunnel name.
    pub tunnel_name: String,
    /// The tunnel's ingress rules, catch-all last.
    pub ingress: Vec<IngressRule>,
    /// The proxied CNAMEs to the tunnel.
    pub records: Vec<ExportRecord>,
    /// The cloudflared version to pin (the Compose image tag), if known.
    pub cloudflared_version: Option<String>,
}

/// Renders `input` as `format`.
pub fn render(input: &ExportInput, format: ExportFormat) -> ExportFile {
    match format {
        ExportFormat::ConfigYaml => ExportFile {
            file_name: "config.yml".into(),
            contents: config_yaml(input),
        },
        ExportFormat::DockerCompose => ExportFile {
            file_name: "compose.yaml".into(),
            contents: docker_compose(input),
        },
        ExportFormat::Terraform => ExportFile {
            file_name: "teitunnel.tf".into(),
            contents: terraform(input),
        },
    }
}

/// A string as a YAML (or HCL) double-quoted scalar: JSON quoting is valid in both.
fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into())
}

fn config_yaml(input: &ExportInput) -> String {
    let mut out = format!(
        "# cloudflared configuration for tunnel {name}, exported by Teitunnel.\n\
         # This tunnel is managed remotely (its routes live in Cloudflare), so cloudflared\n\
         # run with its token ignores the ingress below. Use this file to run the same\n\
         # routes as a locally-managed tunnel, or as a record of them.\n\
         tunnel: {id}\n\
         credentials-file: /etc/cloudflared/{id}.json\n\
         ingress:\n",
        name = quoted(&input.tunnel_name),
        id = input.tunnel_id,
    );
    for rule in &input.ingress {
        match &rule.hostname {
            Some(hostname) => out.push_str(&format!("  - hostname: {}\n", quoted(hostname))),
            None => out.push_str("  - service: "),
        }
        if rule.hostname.is_some() {
            if let Some(path) = &rule.path {
                out.push_str(&format!("    path: {}\n", quoted(path)));
            }
            out.push_str(&format!("    service: {}\n", quoted(&rule.service)));
            if !rule.origin_request.is_empty() {
                // A JSON object is a valid YAML flow mapping.
                let settings = Value::Object(rule.origin_request.clone());
                out.push_str(&format!("    originRequest: {settings}\n"));
            }
        } else {
            out.push_str(&format!("{}\n", quoted(&rule.service)));
        }
    }
    out
}

fn docker_compose(input: &ExportInput) -> String {
    let tag = input.cloudflared_version.as_deref().unwrap_or("latest");
    let routes: Vec<String> = input
        .ingress
        .iter()
        .filter_map(|r| {
            Some(format!(
                "#   {}{} -> {}",
                r.hostname.as_deref()?,
                r.path
                    .as_deref()
                    .map(|p| format!(" (path {p})"))
                    .unwrap_or_default(),
                r.service
            ))
        })
        .collect();
    format!(
        "# Runs tunnel {name} in Docker, exported by Teitunnel.\n\
         # Its routes live in Cloudflare:\n\
         {routes}\n\
         # Origins must be reachable from the container: \"localhost\" is the container\n\
         # itself, so use host.docker.internal or the other services' names instead.\n\
         #\n\
         # The run token isn't included. Get it with:\n\
         #   cloudflared tunnel token {id}\n\
         # and put it in an .env file next to this one as TUNNEL_TOKEN=...\n\
         # Stop the connector on the Mac once this one is connected, if it should move.\n\
         services:\n  \
           cloudflared:\n    \
             image: cloudflare/cloudflared:{tag}\n    \
             command: tunnel --no-autoupdate run\n    \
             environment:\n      \
               TUNNEL_TOKEN: ${{TUNNEL_TOKEN}}\n    \
             restart: unless-stopped\n",
        name = quoted(&input.tunnel_name),
        id = input.tunnel_id,
        routes = routes.join("\n"),
    )
}

/// `noTLSVerify` → `no_tls_verify`, `matchSNItoHost` → `match_sn_ito_host`: the
/// provider's attribute names for the API's `originRequest` keys.
fn snake_case(key: &str) -> String {
    let chars: Vec<char> = key.chars().collect();
    let mut out = String::with_capacity(key.len() + 4);
    for (i, c) in chars.iter().enumerate() {
        if c.is_ascii_uppercase() {
            let prev_lower =
                i > 0 && (chars[i - 1].is_ascii_lowercase() || chars[i - 1].is_ascii_digit());
            let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            let prev_upper = i > 0 && chars[i - 1].is_ascii_uppercase();
            if i > 0 && (prev_lower || (prev_upper && next_lower)) {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(*c);
        }
    }
    out
}

/// A string as an HCL literal: JSON quoting, with template sequences escaped.
fn hcl_string(text: &str) -> String {
    quoted(text).replace("${", "$${").replace("%{", "%%{")
}

fn hcl_value(value: &Value, indent: usize, snake_keys: bool) -> String {
    match value {
        Value::String(s) => hcl_string(s),
        Value::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|v| hcl_value(v, indent, snake_keys))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::Object(map) => hcl_object(map, indent, snake_keys),
        other => other.to_string(),
    }
}

fn hcl_object(map: &Map<String, Value>, indent: usize, snake_keys: bool) -> String {
    let fields: Vec<(String, String)> = map
        .iter()
        .map(|(key, value)| {
            let key = if snake_keys {
                snake_case(key)
            } else {
                key.clone()
            };
            (key, hcl_value(value, indent + 1, snake_keys))
        })
        .collect();
    format!(
        "{{\n{}{}}}",
        aligned(&fields, (indent + 1) * 2),
        "  ".repeat(indent)
    )
}

/// `key = value` lines with the `=` aligned, as `terraform fmt` writes them.
fn aligned(fields: &[(String, String)], indent: usize) -> String {
    let width = fields.iter().map(|(key, _)| key.len()).max().unwrap_or(0);
    let pad = " ".repeat(indent);
    fields
        .iter()
        .map(|(key, value)| format!("{pad}{key:<width$} = {value}\n"))
        .collect()
}

/// A Terraform identifier from a hostname, unique within `taken`.
fn resource_name(hostname: &str, taken: &mut HashSet<String>) -> String {
    let mut base: String = hostname
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if !base.starts_with(|c: char| c.is_ascii_alphabetic()) {
        base.insert_str(0, "r_");
    }
    let mut name = base.clone();
    let mut n = 2;
    while !taken.insert(name.clone()) {
        name = format!("{base}_{n}");
        n += 1;
    }
    name
}

fn terraform(input: &ExportInput) -> String {
    let (account, tunnel) = (&input.account_id, &input.tunnel_id);
    let mut out = format!(
        "# Tunnel {name} and its routes, exported by Teitunnel for the Cloudflare\n\
         # Terraform provider v5. The import blocks adopt what already exists, so\n\
         # `terraform plan` shows no changes; nothing is recreated.\n\
         # Needs an API token with Cloudflare Tunnel: Edit and DNS: Edit.\n\
         # Terraform takes over these resources: change them there, not in Teitunnel.\n\n\
         terraform {{\n  \
           required_providers {{\n    \
             cloudflare = {{\n      \
               source  = \"cloudflare/cloudflare\"\n      \
               version = \"~> 5\"\n    \
             }}\n  \
           }}\n\
         }}\n\n\
         locals {{\n  \
           account_id = {account}\n\
         }}\n\n\
         resource \"cloudflare_zero_trust_tunnel_cloudflared\" \"mac\" {{\n  \
           account_id = local.account_id\n  \
           name       = {name}\n  \
           config_src = \"cloudflare\"\n\
         }}\n\n\
         import {{\n  \
           to = cloudflare_zero_trust_tunnel_cloudflared.mac\n  \
           id = \"{account_raw}/{tunnel}\"\n\
         }}\n\n\
         resource \"cloudflare_zero_trust_tunnel_cloudflared_config\" \"mac\" {{\n  \
           account_id = local.account_id\n  \
           tunnel_id  = cloudflare_zero_trust_tunnel_cloudflared.mac.id\n  \
           config = {{\n    \
             ingress = [\n",
        name = hcl_string(&input.tunnel_name),
        account = hcl_string(account),
        account_raw = account,
    );
    for rule in &input.ingress {
        let mut fields: Vec<(String, String)> = Vec::new();
        if let Some(hostname) = &rule.hostname {
            fields.push(("hostname".into(), hcl_string(hostname)));
        }
        if let Some(path) = &rule.path {
            fields.push(("path".into(), hcl_string(path)));
        }
        fields.push(("service".into(), hcl_string(&rule.service)));
        if !rule.origin_request.is_empty() {
            fields.push((
                "origin_request".into(),
                hcl_object(&rule.origin_request, 4, true),
            ));
        }
        out.push_str("      {\n");
        out.push_str(&aligned(&fields, 8));
        out.push_str("      },\n");
    }
    out.push_str(&format!(
        "    ]\n  }}\n}}\n\n\
         import {{\n  \
           to = cloudflare_zero_trust_tunnel_cloudflared_config.mac\n  \
           id = \"{account}/{tunnel}\"\n\
         }}\n"
    ));

    let mut taken = HashSet::from(["mac".to_owned()]);
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for record in &input.records {
        if seen.insert(record.hostname.as_str(), ()).is_some() {
            continue;
        }
        let name = resource_name(&record.hostname, &mut taken);
        let mut fields = vec![
            ("zone_id".to_owned(), hcl_string(&record.zone_id)),
            ("name".to_owned(), hcl_string(&record.hostname)),
            ("type".to_owned(), "\"CNAME\"".to_owned()),
            (
                "content".to_owned(),
                "\"${cloudflare_zero_trust_tunnel_cloudflared.mac.id}.cfargotunnel.com\""
                    .to_owned(),
            ),
            ("proxied".to_owned(), record.proxied.to_string()),
            ("ttl".to_owned(), record.ttl.to_string()),
        ];
        if let Some(comment) = &record.comment {
            fields.push(("comment".to_owned(), hcl_string(comment)));
        }
        out.push_str(&format!(
            "\nresource \"cloudflare_dns_record\" \"{name}\" {{\n{fields}}}\n\n\
             import {{\n  \
               to = cloudflare_dns_record.{name}\n  \
               id = \"{zone_raw}/{record_id}\"\n\
             }}\n",
            fields = aligned(&fields, 2),
            zone_raw = record.zone_id,
            record_id = record.record_id,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn input() -> ExportInput {
        ExportInput {
            account_id: "023e105f4ecef8ad9ca31a8372d0c353".into(),
            tunnel_id: "6ff42ae2-765d-4adf-8112-31c55c1551ef".into(),
            tunnel_name: "MacBook-Pro".into(),
            ingress: serde_json::from_value(json!([
                { "hostname": "app.xyz.com", "service": "http://localhost:3000" },
                { "hostname": "api.xyz.com", "path": "^/v1/", "service": "http://localhost:8000",
                  "originRequest": { "noTLSVerify": true, "httpHostHeader": "api.local",
                                     "matchSNItoHost": false, "connectTimeout": 10 } },
                { "service": "http_status:404" }
            ]))
            .unwrap(),
            records: vec![
                ExportRecord {
                    zone_id: "z1".into(),
                    record_id: "r1".into(),
                    hostname: "app.xyz.com".into(),
                    comment: Some("teitunnel:route=a1".into()),
                    ttl: 1,
                    proxied: true,
                },
                ExportRecord {
                    zone_id: "z1".into(),
                    record_id: "r2".into(),
                    hostname: "api.xyz.com".into(),
                    comment: None,
                    ttl: 300,
                    proxied: true,
                },
            ],
            cloudflared_version: Some("2026.9.1".into()),
        }
    }

    #[test]
    fn renders_config_yaml() {
        let file = render(&input(), ExportFormat::ConfigYaml);
        assert_eq!(file.file_name, "config.yml");
        insta::assert_snapshot!(file.contents);
    }

    #[test]
    fn renders_docker_compose_without_the_token() {
        let file = render(&input(), ExportFormat::DockerCompose);
        assert!(file.contents.contains("TUNNEL_TOKEN: ${TUNNEL_TOKEN}"));
        insta::assert_snapshot!(file.contents);
    }

    #[test]
    fn renders_terraform_with_imports() {
        let file = render(&input(), ExportFormat::Terraform);
        insta::assert_snapshot!(file.contents);
    }

    #[test]
    fn maps_origin_keys_like_the_provider() {
        assert_eq!(snake_case("noTLSVerify"), "no_tls_verify");
        assert_eq!(snake_case("httpHostHeader"), "http_host_header");
        assert_eq!(snake_case("matchSNItoHost"), "match_sn_ito_host");
        assert_eq!(snake_case("caPool"), "ca_pool");
        assert_eq!(snake_case("http2Origin"), "http2_origin");
        assert_eq!(snake_case("audTag"), "aud_tag");
    }

    #[test]
    fn escapes_what_hcl_would_interpolate() {
        assert_eq!(hcl_string("a${b}%{c}\"d"), r#""a$${b}%%{c}\"d""#);
        let mut taken = HashSet::new();
        assert_eq!(resource_name("app.xyz.com", &mut taken), "app_xyz_com");
        assert_eq!(resource_name("app-xyz.com", &mut taken), "app_xyz_com_2");
        assert_eq!(resource_name("1.xyz.com", &mut taken), "r_1_xyz_com");
    }
}
