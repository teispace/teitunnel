//! What runs in front of routes: Workers (scripts, versions, deployments), Worker
//! routes, D1 databases, zone rulesets (edge rules) and Access service tokens, with the
//! shapes and refusals `cf-api` expects.

use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::{Request, err, ok};

/// The Workers side of the account.
#[derive(Default)]
pub(crate) struct Workers {
    /// Script name → its metadata as last uploaded (secret values dropped, as
    /// Cloudflare never returns them) and its deployed version.
    scripts: BTreeMap<String, (Value, String)>,
    /// Zone id → Worker routes.
    routes: BTreeMap<String, Vec<Value>>,
    /// D1 databases.
    databases: Vec<Value>,
    /// Database id → statements run, in order.
    statements: BTreeMap<String, Vec<String>>,
    /// (Zone id, phase) → entry point ruleset.
    rulesets: BTreeMap<(String, String), Value>,
    /// Access service tokens.
    service_tokens: Vec<Value>,
}

/// A fresh id from the shared counter.
fn fresh(next_id: &mut u32, prefix: &str) -> String {
    *next_id += 1;
    format!("{prefix}{:08}-0000-4000-8000-000000000000", *next_id)
}

fn no_worker() -> (u16, Value) {
    err(404, 10007, "This Worker does not exist on your account.")
}

/// The `metadata` part of a Worker upload (a `multipart/form-data` body).
fn metadata(raw: &[u8]) -> Option<Value> {
    let marker = b"name=\"metadata\"";
    let at = raw.windows(marker.len()).position(|w| w == marker)?;
    let rest = &raw[at..];
    let start = rest.windows(4).position(|w| w == b"\r\n\r\n")? + 4;
    serde_json::Deserializer::from_slice(&rest[start..])
        .into_iter::<Value>()
        .next()?
        .ok()
}

/// A Worker's bindings as uploaded, with `keep_bindings` carrying over the previous
/// version's bindings of those types, and secret values never kept.
fn merged_bindings(previous: Option<&Value>, metadata: &Value) -> Value {
    let mut bindings: Vec<Value> = metadata["bindings"].as_array().cloned().unwrap_or_default();
    let keep: Vec<&str> = metadata["keep_bindings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    for old in previous
        .and_then(|p| p["bindings"].as_array())
        .into_iter()
        .flatten()
    {
        let kept = old["type"].as_str().is_some_and(|t| keep.contains(&t));
        if kept && !bindings.iter().any(|b| b["name"] == old["name"]) {
            bindings.push(old.clone());
        }
    }
    for binding in &mut bindings {
        if binding["type"] == "secret_text" {
            binding["text"] = Value::Null;
        }
    }
    Value::Array(bindings)
}

fn deployment(version: &str) -> Value {
    json!({
        "id": format!("dep-{version}"), "created_on": "2026-09-25T00:00:00Z",
        "versions": [{ "version_id": version, "percentage": 100.0 }]
    })
}

/// A rule as Cloudflare stores it: with an id, and `position` dropped.
fn stored_rule(next_id: &mut u32, rule: &Value) -> Value {
    let mut rule = rule.clone();
    if let Some(object) = rule.as_object_mut() {
        object.remove("position");
        object.insert("id".into(), json!(fresh(next_id, "rule")));
        object.entry("enabled").or_insert(json!(true));
    }
    rule
}

impl Workers {
    fn ruleset_mut(&mut self, zone: &str, id: &str) -> Option<&mut Value> {
        self.rulesets
            .iter_mut()
            .find(|((z, _), r)| z == zone && r["id"] == id)
            .map(|(_, r)| r)
    }

    fn database(&self, id: &str) -> bool {
        self.databases.iter().any(|d| d["uuid"] == id)
    }

    /// Answers the request if it's one of these endpoints.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn handle(
        &mut self,
        next_id: &mut u32,
        req: &Request,
        parts: &[&str],
    ) -> Option<(u16, Value)> {
        Some(match (req.method.as_str(), parts) {
            // Workers.
            ("PUT", ["accounts", _, "workers", "scripts", script]) => {
                let Some(metadata) = metadata(&req.raw) else {
                    return Some(err(400, 10021, "No metadata part in the upload."));
                };
                if metadata["main_module"].as_str().is_none_or(str::is_empty) {
                    return Some(err(400, 10021, "No main module."));
                }
                let previous = self.scripts.get(*script).map(|(m, _)| m);
                let mut stored = metadata.clone();
                stored["bindings"] = merged_bindings(previous, &metadata);
                if let Some(object) = stored.as_object_mut() {
                    object.remove("keep_bindings");
                }
                let version = fresh(next_id, "ver");
                self.scripts
                    .insert((*script).to_owned(), (stored, version.clone()));
                ok(json!({ "id": script, "etag": version }))
            }
            ("POST", ["accounts", _, "workers", "scripts", script, "versions"]) => {
                if metadata(&req.raw).is_none() {
                    return Some(err(400, 10021, "No metadata part in the upload."));
                }
                if !self.scripts.contains_key(*script) {
                    return Some(no_worker());
                }
                ok(json!({ "id": fresh(next_id, "ver"), "number": *next_id }))
            }
            ("GET", ["accounts", _, "workers", "scripts", script, "deployments"]) => {
                match self.scripts.get(*script) {
                    Some((_, version)) => ok(json!({ "deployments": [deployment(version)] })),
                    None => no_worker(),
                }
            }
            ("POST", ["accounts", _, "workers", "scripts", script, "deployments"]) => {
                let Some(version) = req.body["versions"][0]["version_id"].as_str() else {
                    return Some(err(400, 10021, "No version to deploy."));
                };
                match self.scripts.get_mut(*script) {
                    Some((_, deployed)) => {
                        version.clone_into(deployed);
                        ok(deployment(version))
                    }
                    None => no_worker(),
                }
            }
            ("DELETE", ["accounts", _, "workers", "scripts", script]) => {
                if self.scripts.remove(*script).is_none() {
                    return Some(no_worker());
                }
                // A Worker's routes stay, pointing at nothing, as on Cloudflare.
                ok(json!({ "id": script }))
            }
            ("GET", ["zones", zone, "workers", "routes"]) => ok(Value::Array(
                self.routes.get(*zone).cloned().unwrap_or_default(),
            )),
            ("POST", ["zones", zone, "workers", "routes"]) => {
                let pattern = req.body["pattern"].clone();
                let script = req.body["script"].as_str().unwrap_or_default();
                if !script.is_empty() && !self.scripts.contains_key(script) {
                    return Some(no_worker());
                }
                let list = self.routes.entry((*zone).to_owned()).or_default();
                if list.iter().any(|r| r["pattern"] == pattern) {
                    return Some(err(409, 10020, "A route with this pattern already exists."));
                }
                let route = json!({
                    "id": fresh(next_id, "wr"), "pattern": pattern, "script": script,
                    "request_limit_fail_open": req.body["request_limit_fail_open"].clone(),
                });
                list.push(route.clone());
                ok(route)
            }
            ("DELETE", ["zones", zone, "workers", "routes", id]) => {
                let list = self.routes.entry((*zone).to_owned()).or_default();
                let before = list.len();
                list.retain(|r| r["id"] != *id);
                if list.len() < before {
                    ok(json!({ "id": id }))
                } else {
                    err(404, 10009, "Route not found.")
                }
            }
            // D1.
            ("GET", ["accounts", _, "d1", "database"]) => ok(Value::Array(
                self.databases
                    .iter()
                    .filter(|d| req.query.get("name").is_none_or(|n| d["name"] == *n))
                    .cloned()
                    .collect(),
            )),
            ("POST", ["accounts", _, "d1", "database"]) => {
                let name = req.body["name"].clone();
                if self.databases.iter().any(|d| d["name"] == name) {
                    return Some(err(400, 7502, "A database with that name already exists."));
                }
                let database = json!({
                    "uuid": fresh(next_id, "db"), "name": name,
                    "created_at": "2026-09-25T00:00:00Z"
                });
                self.databases.push(database.clone());
                ok(database)
            }
            ("DELETE", ["accounts", _, "d1", "database", id]) => {
                let before = self.databases.len();
                self.databases.retain(|d| d["uuid"] != *id);
                if self.databases.len() < before {
                    self.statements.remove(*id);
                    ok(Value::Null)
                } else {
                    err(404, 7404, "The database could not be found.")
                }
            }
            ("POST", ["accounts", _, "d1", "database", id, "query"]) => {
                if !self.database(id) {
                    return Some(err(404, 7404, "The database could not be found."));
                }
                let statements: Vec<Value> = match req.body["batch"].as_array() {
                    Some(batch) => batch.clone(),
                    None => vec![req.body.clone()],
                };
                if statements
                    .iter()
                    .any(|s| s["sql"].as_str().is_none_or(str::is_empty))
                {
                    return Some(err(400, 7500, "Empty SQL statement."));
                }
                let log = self.statements.entry((*id).to_owned()).or_default();
                log.extend(
                    statements
                        .iter()
                        .filter_map(|s| s["sql"].as_str().map(str::to_owned)),
                );
                // Nothing is stored: reads find no rows, writes change none.
                ok(Value::Array(
                    statements
                        .iter()
                        .map(
                            |_| json!({ "results": [], "success": true, "meta": { "changes": 0 } }),
                        )
                        .collect(),
                ))
            }
            // Rulesets: one entry point per zone and phase.
            ("GET", ["zones", zone, "rulesets", "phases", phase, "entrypoint"]) => {
                match self
                    .rulesets
                    .get(&((*zone).to_owned(), (*phase).to_owned()))
                {
                    Some(ruleset) => ok(ruleset.clone()),
                    None => err(
                        404,
                        10003,
                        "Could not find entrypoint ruleset in the phase.",
                    ),
                }
            }
            ("POST", ["zones", zone, "rulesets"]) => {
                let phase = req.body["phase"].as_str().unwrap_or_default().to_owned();
                let key = ((*zone).to_owned(), phase.clone());
                if self.rulesets.contains_key(&key) {
                    return Some(err(
                        400,
                        20217,
                        "A zone entry point ruleset already exists.",
                    ));
                }
                let rules: Vec<Value> = req.body["rules"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|r| stored_rule(next_id, r))
                    .collect();
                let ruleset = json!({ "id": fresh(next_id, "rs"), "phase": phase, "rules": rules });
                self.rulesets.insert(key, ruleset.clone());
                ok(ruleset)
            }
            ("POST", ["zones", zone, "rulesets", id, "rules"]) => {
                let index = req.body["position"]["index"].as_u64();
                let rule = stored_rule(next_id, &req.body);
                let Some(ruleset) = self.ruleset_mut(zone, id) else {
                    return Some(err(404, 10003, "Ruleset not found."));
                };
                let Some(rules) = ruleset["rules"].as_array_mut() else {
                    return Some(err(500, 10000, "Corrupt ruleset."));
                };
                let at = index
                    .and_then(|i| usize::try_from(i).ok())
                    .map_or(rules.len(), |i| i.saturating_sub(1).min(rules.len()));
                rules.insert(at, rule);
                ok(ruleset.clone())
            }
            ("PATCH", ["zones", zone, "rulesets", id, "rules", rule]) => {
                let Some(ruleset) = self.ruleset_mut(zone, id) else {
                    return Some(err(404, 10003, "Ruleset not found."));
                };
                let Some(target) = ruleset["rules"]
                    .as_array_mut()
                    .and_then(|rules| rules.iter_mut().find(|r| r["id"] == *rule))
                else {
                    return Some(err(404, 10003, "Rule not found."));
                };
                let mut replaced = req.body.clone();
                replaced["id"] = json!(rule);
                *target = replaced;
                ok(ruleset.clone())
            }
            ("DELETE", ["zones", zone, "rulesets", id, "rules", rule]) => {
                let Some(ruleset) = self.ruleset_mut(zone, id) else {
                    return Some(err(404, 10003, "Ruleset not found."));
                };
                let Some(rules) = ruleset["rules"].as_array_mut() else {
                    return Some(err(500, 10000, "Corrupt ruleset."));
                };
                let before = rules.len();
                rules.retain(|r| r["id"] != *rule);
                if rules.len() < before {
                    ok(ruleset.clone())
                } else {
                    err(404, 10003, "Rule not found.")
                }
            }
            // Access service tokens.
            ("GET", ["accounts", _, "access", "service_tokens"]) => {
                ok(Value::Array(self.service_tokens.clone()))
            }
            ("POST", ["accounts", _, "access", "service_tokens"]) => {
                let token = json!({
                    "id": fresh(next_id, "st"), "name": req.body["name"].clone(),
                    "client_id": format!("{}.access", fresh(next_id, "")),
                    "expires_at": "2027-09-25T00:00:00Z", "created_at": "2026-09-25T00:00:00Z",
                });
                self.service_tokens.push(token.clone());
                let mut issued = token;
                issued["client_secret"] = json!(fresh(next_id, "secret"));
                ok(issued)
            }
            ("POST", ["accounts", _, "access", "service_tokens", id, "rotate"]) => {
                match self.service_tokens.iter().find(|t| t["id"] == *id) {
                    Some(token) => {
                        let mut issued = token.clone();
                        issued["client_secret"] = json!(fresh(next_id, "secret"));
                        ok(issued)
                    }
                    None => err(404, 12130, "Service token not found"),
                }
            }
            ("DELETE", ["accounts", _, "access", "service_tokens", id]) => {
                let before = self.service_tokens.len();
                self.service_tokens.retain(|t| t["id"] != *id);
                if self.service_tokens.len() < before {
                    ok(json!({ "id": id }))
                } else {
                    err(404, 12130, "Service token not found")
                }
            }
            _ => return None,
        })
    }

    /// The Worker (if any) whose route matches `host` + `path` (the most specific
    /// pattern wins, as on Cloudflare), with its bindings.
    pub(crate) fn in_front(&self, host: &str, path: &str) -> Option<(&str, &Value)> {
        let target = format!("{host}{path}");
        self.routes
            .values()
            .flatten()
            .filter_map(|r| {
                let pattern = r["pattern"].as_str()?;
                let prefix = pattern.strip_suffix('*').unwrap_or(pattern);
                let matches = target.eq_ignore_ascii_case(pattern)
                    || (pattern.ends_with('*')
                        && target
                            .to_ascii_lowercase()
                            .starts_with(&prefix.to_ascii_lowercase()));
                matches.then_some((prefix.len(), r["script"].as_str()?))
            })
            .max_by_key(|(len, _)| *len)
            .and_then(|(_, script)| {
                self.scripts
                    .get_key_value(script)
                    .map(|(name, (metadata, _))| (name.as_str(), metadata))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_metadata_part_and_keeps_secrets_out() {
        let body = b"--b\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n{\"main_module\":\"worker.js\",\"bindings\":[{\"type\":\"secret_text\",\"name\":\"S\",\"text\":\"x\"}]}\r\n--b--\r\n";
        let metadata = metadata(body).unwrap();
        assert_eq!(metadata["main_module"], "worker.js");
        let kept = merged_bindings(None, &metadata);
        assert_eq!(kept[0]["text"], Value::Null);
        // A later upload keeping secrets keeps the earlier one.
        let later = json!({ "bindings": [], "keep_bindings": ["secret_text"] });
        let previous = json!({ "bindings": kept });
        assert_eq!(merged_bindings(Some(&previous), &later)[0]["name"], "S");
        assert_eq!(
            merged_bindings(Some(&previous), &json!({ "bindings": [] })),
            json!([])
        );
    }
}
