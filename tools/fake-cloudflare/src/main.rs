//! A stand-in for the Cloudflare API (and edge) for end-to-end tests of the routes flow.
//!
//! It keeps one account with two zones in memory and implements the endpoints Teitunnel
//! uses: token verify, accounts, zones, tunnels, remote configuration, tunnel token, DNS
//! records, Access (a Zero Trust organization, login methods, applications) and private
//! networks (routes, virtual networks, WARP device settings), with
//! Cloudflare's response envelope. Requests whose `Host` isn't the server itself are
//! answered as the edge would: a redirect to the login page for a hostname with an
//! Access application, otherwise `200` from a working route, so the verifier can run
//! against it too.
//!
//! Usage: `fake-cloudflare [port]` (0 or absent: any free port). Prints
//! `listening on 127.0.0.1:<port>` once ready. Any token is accepted except `bad`.

use std::{
    collections::BTreeMap,
    process::ExitCode,
    sync::{Arc, Mutex, PoisonError},
};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const ACCOUNT: &str = "e2e-account";

#[derive(Default)]
struct State {
    next_id: u32,
    /// Tunnel id → (name, config version, config).
    tunnels: BTreeMap<String, (String, u64, Value)>,
    /// Zone id → records.
    records: BTreeMap<String, Vec<Value>>,
    /// Access applications, with their policies by reference (`{id, precedence}`).
    access_apps: Vec<Value>,
    /// Reusable Access policies.
    policies: Vec<Value>,
    /// Login methods (identity providers).
    login_methods: Vec<Value>,
    /// Private network routes.
    network_routes: Vec<Value>,
}

impl State {
    /// An application as Cloudflare returns it: its policies in full, each with how many
    /// applications use it.
    fn expanded(&self, app: &Value) -> Value {
        let mut app = app.clone();
        let policies: Vec<Value> = app["policies"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|reference| {
                let Some(mut policy) = self
                    .policies
                    .iter()
                    .find(|p| p["id"] == reference["id"])
                    .cloned()
                else {
                    // Inline (application-scoped) policies, as 0.1 wrote them.
                    return reference.clone();
                };
                policy["precedence"] = reference["precedence"].clone();
                policy["app_count"] = json!(self.uses(&policy["id"]));
                policy
            })
            .collect();
        app["policies"] = json!(policies);
        app
    }

    fn uses(&self, policy: &Value) -> usize {
        self.access_apps
            .iter()
            .filter(|a| {
                a["policies"]
                    .as_array()
                    .is_some_and(|list| list.iter().any(|p| p["id"] == *policy))
            })
            .count()
    }

    fn id(&mut self, prefix: &str) -> String {
        self.next_id += 1;
        format!("{prefix}{:08}-0000-4000-8000-000000000000", self.next_id)
    }
}

fn zones() -> Vec<Value> {
    [("z-xyz", "xyz.com"), ("z-yx", "yx.com")]
        .iter()
        .map(|(id, name)| {
            json!({
                "id": id, "name": name, "status": "active", "paused": false, "type": "full",
                "name_servers": ["ada.ns.cloudflare.com", "bob.ns.cloudflare.com"],
                "original_name_servers": null,
                "account": { "id": ACCOUNT, "name": "E2E" },
                "plan": { "name": "Free Website" }
            })
        })
        .collect()
}

struct Request {
    method: String,
    path: String,
    query: BTreeMap<String, String>,
    host: String,
    auth: String,
    body: Value,
}

#[allow(clippy::needless_pass_by_value)]
fn ok(result: Value) -> (u16, Value) {
    let info = result.as_array().map(|list| {
        json!({ "page": 1, "per_page": 50, "total_pages": 1, "count": list.len(), "total_count": list.len() })
    });
    (
        200,
        json!({ "success": true, "errors": [], "messages": [], "result": result, "result_info": info }),
    )
}

fn err(status: u16, code: u32, message: &str) -> (u16, Value) {
    (
        status,
        json!({ "success": false, "errors": [{ "code": code, "message": message }], "messages": [], "result": null }),
    )
}

fn handle(state: &Mutex<State>, req: &Request) -> (u16, Value) {
    if req.auth != "Bearer bad" && !req.auth.starts_with("Bearer ") {
        return err(401, 10000, "Authentication error");
    }
    if req.auth == "Bearer bad" {
        return err(401, 1000, "Invalid API Token");
    }
    let mut s = state.lock().unwrap_or_else(PoisonError::into_inner);
    let parts: Vec<&str> = req.path.trim_matches('/').split('/').collect();
    let method = req.method.as_str();
    match (method, parts.as_slice()) {
        ("GET", ["user", "tokens", "verify"]) => ok(json!({ "id": "tok", "status": "active" })),
        ("GET", ["accounts"]) => ok(json!([{ "id": ACCOUNT, "name": "E2E" }])),
        ("GET", ["zones"]) => ok(Value::Array(zones())),
        ("GET", ["zones", zone]) => zones()
            .into_iter()
            .find(|z| z["id"] == *zone)
            .map_or_else(|| err(404, 1001, "Invalid zone"), ok),
        ("GET", ["accounts", _, "cfd_tunnel"]) => {
            let list = s
                .tunnels
                .iter()
                .map(|(id, (name, ..))| tunnel_json(id, name))
                .collect();
            ok(Value::Array(list))
        }
        ("POST", ["accounts", _, "cfd_tunnel"]) => {
            let name = req.body["name"].as_str().unwrap_or("tunnel").to_owned();
            if s.tunnels.values().any(|(n, ..)| *n == name) {
                return err(409, 1013, "You already have a tunnel with this name");
            }
            let id = s.id("");
            s.tunnels.insert(id.clone(), (name.clone(), 0, Value::Null));
            ok(tunnel_json(&id, &name))
        }
        ("GET", ["accounts", _, "cfd_tunnel", id]) => match s.tunnels.get(*id) {
            Some((name, ..)) => ok(tunnel_json(id, name)),
            None => err(404, 1003, "Tunnel not found"),
        },
        ("DELETE", ["accounts", _, "cfd_tunnel", id]) => match s.tunnels.remove(*id) {
            Some(_) => ok(json!({ "id": id })),
            None => err(404, 1003, "Tunnel not found"),
        },
        ("GET", ["accounts", _, "cfd_tunnel", id, "connections"]) => {
            if s.tunnels.contains_key(*id) {
                ok(connectors_json())
            } else {
                err(404, 1003, "Tunnel not found")
            }
        }
        ("DELETE", ["accounts", _, "cfd_tunnel", id, "connections"]) => {
            if s.tunnels.contains_key(*id) {
                ok(Value::Null)
            } else {
                err(404, 1003, "Tunnel not found")
            }
        }
        ("GET", ["accounts", _, "cfd_tunnel", id, "token"]) => {
            if s.tunnels.contains_key(*id) {
                ok(json!(format!("e2e-run-token-{id}")))
            } else {
                err(404, 1003, "Tunnel not found")
            }
        }
        ("GET", ["accounts", _, "cfd_tunnel", id, "configurations"]) => match s.tunnels.get(*id) {
            Some((_, version, config)) => {
                ok(json!({ "tunnel_id": id, "version": version, "config": config }))
            }
            None => err(404, 1003, "Tunnel not found"),
        },
        ("PUT", ["accounts", _, "cfd_tunnel", id, "configurations"]) => {
            match s.tunnels.get_mut(*id) {
                Some((_, version, config)) => {
                    *version += 1;
                    *config = req.body["config"].clone();
                    ok(json!({ "tunnel_id": id, "version": *version, "config": config }))
                }
                None => err(404, 1003, "Tunnel not found"),
            }
        }
        // No Load Balancing add-on: no load balancers.
        ("GET", ["zones", _, "load_balancers"]) => ok(json!([])),
        ("GET", ["zones", zone, "dns_records"]) => {
            let records = s.records.get(*zone).cloned().unwrap_or_default();
            let matching = records
                .into_iter()
                .filter(|r| {
                    req.query.get("name").is_none_or(|n| r["name"] == *n)
                        && req.query.get("type").is_none_or(|t| r["type"] == *t)
                        && req.query.get("content").is_none_or(|c| r["content"] == *c)
                        && req.query.get("comment.contains").is_none_or(|needle| {
                            r["comment"]
                                .as_str()
                                .is_some_and(|comment| comment.contains(needle.as_str()))
                        })
                })
                .collect();
            ok(Value::Array(matching))
        }
        ("POST", ["zones", zone, "dns_records"]) => {
            let name = req.body["name"].clone();
            let taken = s
                .records
                .get(*zone)
                .is_some_and(|list| list.iter().any(|r| r["name"] == name));
            if taken {
                return err(
                    400,
                    81053,
                    "An A, AAAA, or CNAME record with that host already exists.",
                );
            }
            let mut record = req.body.clone();
            record["id"] = json!(s.id("r"));
            s.records
                .entry((*zone).to_owned())
                .or_default()
                .push(record.clone());
            ok(record)
        }
        ("PATCH", ["zones", zone, "dns_records", id]) => {
            let found = s
                .records
                .get_mut(*zone)
                .and_then(|list| list.iter_mut().find(|r| r["id"] == *id));
            match found {
                // As Cloudflare since 2026-06-30: no type changes in place.
                Some(record)
                    if req
                        .body
                        .get("type")
                        .is_some_and(|kind| *kind != record["type"]) =>
                {
                    err(400, 9000, "DNS record type cannot be changed.")
                }
                Some(record) => {
                    if let (Some(target), Some(patch)) =
                        (record.as_object_mut(), req.body.as_object())
                    {
                        for (key, value) in patch {
                            target.insert(key.clone(), value.clone());
                        }
                    }
                    ok(record.clone())
                }
                // Also what capability probes (PATCH on a nil id) expect when authorized.
                None => err(404, 81044, "Record does not exist."),
            }
        }
        // Deletes, then posts, all or nothing.
        ("POST", ["zones", zone, "dns_records", "batch"]) => {
            let mut list = s.records.get(*zone).cloned().unwrap_or_default();
            let mut deleted = Vec::new();
            for delete in req.body["deletes"].as_array().into_iter().flatten() {
                let Some(at) = list.iter().position(|r| r["id"] == delete["id"]) else {
                    return err(404, 81044, "Record does not exist.");
                };
                deleted.push(list.remove(at));
            }
            let mut posted = Vec::new();
            for post in req.body["posts"].as_array().into_iter().flatten() {
                if list.iter().any(|r| r["name"] == post["name"]) {
                    return err(
                        400,
                        81053,
                        "An A, AAAA, or CNAME record with that host already exists.",
                    );
                }
                let mut record = post.clone();
                record["id"] = json!(s.id("r"));
                list.push(record.clone());
                posted.push(record);
            }
            s.records.insert((*zone).to_owned(), list);
            ok(json!({ "deletes": deleted, "posts": posted }))
        }
        ("DELETE", ["zones", zone, "dns_records", id]) => {
            let list = s.records.entry((*zone).to_owned()).or_default();
            let before = list.len();
            list.retain(|r| r["id"] != *id);
            if list.len() < before {
                ok(json!({ "id": id }))
            } else {
                err(404, 81044, "Record does not exist.")
            }
        }
        ("GET", ["accounts", _, "access", "organizations"]) => ok(json!({
            "name": "E2E", "auth_domain": "e2e.cloudflareaccess.com"
        })),
        ("GET", ["accounts", _, "access", "identity_providers"]) => {
            ok(Value::Array(s.login_methods.clone()))
        }
        ("POST", ["accounts", _, "access", "identity_providers"]) => {
            let mut method = req.body.clone();
            method["id"] = json!(s.id("idp"));
            s.login_methods.push(method.clone());
            ok(method)
        }
        ("DELETE", ["accounts", _, "access", "identity_providers", id]) => {
            let before = s.login_methods.len();
            s.login_methods.retain(|m| m["id"] != *id);
            if s.login_methods.len() < before {
                ok(json!({ "id": id }))
            } else {
                err(404, 12130, "Identity provider not found")
            }
        }
        ("GET", ["accounts", _, "access", "apps"]) => {
            let matching = s
                .access_apps
                .iter()
                .filter(|a| req.query.get("domain").is_none_or(|d| a["domain"] == *d))
                .map(|a| s.expanded(a))
                .collect();
            ok(Value::Array(matching))
        }
        ("GET", ["accounts", _, "access", "apps", id]) => {
            match s.access_apps.iter().find(|a| a["id"] == *id) {
                Some(app) => ok(s.expanded(app)),
                None => err(404, 12130, "Application not found"),
            }
        }
        ("POST", ["accounts", _, "access", "apps"]) => {
            if let Some(response) = unknown_policy(&s, &req.body) {
                return response;
            }
            let mut app = req.body.clone();
            app["id"] = json!(s.id("app"));
            s.access_apps.push(app.clone());
            ok(s.expanded(&app))
        }
        ("PUT", ["accounts", _, "access", "apps", id]) => {
            if let Some(response) = unknown_policy(&s, &req.body) {
                return response;
            }
            let Some(at) = s.access_apps.iter().position(|a| a["id"] == *id) else {
                return err(404, 12130, "Application not found");
            };
            let mut app = req.body.clone();
            app["id"] = json!(id);
            s.access_apps[at] = app.clone();
            ok(s.expanded(&app))
        }
        ("DELETE", ["accounts", _, "access", "apps", id]) => {
            let before = s.access_apps.len();
            s.access_apps.retain(|a| a["id"] != *id);
            if s.access_apps.len() < before {
                ok(json!({ "id": id }))
            } else {
                err(404, 12130, "Application not found")
            }
        }
        ("POST", ["accounts", _, "access", "policies"]) => {
            let mut policy = req.body.clone();
            policy["id"] = json!(s.id("pol"));
            policy["reusable"] = json!(true);
            s.policies.push(policy.clone());
            ok(policy)
        }
        ("DELETE", ["accounts", _, "access", "policies", id]) => {
            let target = json!(id);
            if s.uses(&target) > 0 {
                return err(400, 12130, "Policy is used by an application");
            }
            let before = s.policies.len();
            s.policies.retain(|p| p["id"] != target);
            if s.policies.len() < before {
                ok(json!({ "id": id }))
            } else {
                err(404, 12130, "Policy not found")
            }
        }
        ("GET", ["accounts", _, "teamnet", "virtual_networks"]) => ok(json!([
            { "id": "vnet-default", "name": "default", "is_default_network": true }
        ])),
        ("GET", ["accounts", _, "teamnet", "routes"]) => ok(Value::Array(s.network_routes.clone())),
        ("POST", ["accounts", _, "teamnet", "routes"]) => {
            let network = req.body["network"].clone();
            let vnet = req
                .body
                .get("virtual_network_id")
                .cloned()
                .unwrap_or(json!("vnet-default"));
            if s.network_routes
                .iter()
                .any(|r| r["network"] == network && r["virtual_network_id"] == vnet)
            {
                return err(409, 1014, "route already exists");
            }
            let tunnel = req.body["tunnel_id"].clone();
            let tunnel_name = tunnel
                .as_str()
                .and_then(|id| s.tunnels.get(id))
                .map(|(name, ..)| name.clone());
            let route = json!({
                "id": s.id("net"), "network": network, "tunnel_id": tunnel,
                "tunnel_name": tunnel_name, "virtual_network_id": vnet,
                "comment": req.body["comment"].clone(),
            });
            s.network_routes.push(route.clone());
            ok(route)
        }
        ("DELETE", ["accounts", _, "teamnet", "routes", id]) => {
            let before = s.network_routes.len();
            s.network_routes.retain(|r| r["id"] != *id);
            if s.network_routes.len() < before {
                ok(json!({ "id": id }))
            } else {
                err(404, 1015, "Route not found")
            }
        }
        ("GET", ["accounts", _, "devices", "settings"]) => ok(json!({
            "gateway_proxy_enabled": true, "gateway_udp_proxy_enabled": false
        })),
        ("GET", ["accounts", _, "devices", "policy"]) => ok(json!({
            "exclude": [{ "address": "10.0.0.0/8" }, { "address": "192.168.0.0/16" }],
            "include": null
        })),
        // Capability probes on other resources: authorized, no such object.
        ("PATCH", _) => err(404, 1003, "Not found"),
        _ => err(404, 7003, "No route for that URI"),
    }
}

/// An application referring to a policy that doesn't exist is refused, as Cloudflare does.
fn unknown_policy(s: &State, app: &Value) -> Option<(u16, Value)> {
    let unknown = app["policies"].as_array().into_iter().flatten().any(|p| {
        p.get("decision").is_none() && !s.policies.iter().any(|known| known["id"] == p["id"])
    });
    unknown.then(|| err(400, 12130, "Access policy not found"))
}

/// A tunnel as Cloudflare returns it from 2026-10-05: without `connections`, which come
/// from `…/cfd_tunnel/{id}/connections`.
fn tunnel_json(id: &str, name: &str) -> Value {
    json!({
        "id": id, "name": name, "status": "healthy", "created_at": "2026-09-23T00:00:00Z",
        "deleted_at": null, "remote_config": true
    })
}

fn connectors_json() -> Value {
    json!([{
        "id": "c", "version": "2026.9.1", "arch": "linux_amd64", "run_at": "2026-09-23T00:00:00Z",
        "conns": [{ "colo_name": "e2e01", "origin_ip": "127.0.0.1",
                    "opened_at": "2026-09-23T00:00:00Z", "is_pending_reconnect": false }]
    }])
}

fn decode(value: &str) -> String {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
                if let Ok(b) = u8::from_str_radix(hex, 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

async fn read_request(socket: &mut TcpStream) -> Option<Request> {
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end = loop {
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let mut lines = head.lines();
    let mut first = lines.next()?.split_whitespace();
    let method = first.next()?.to_owned();
    let target = first.next()?.to_owned();
    let mut length = 0usize;
    let (mut host, mut auth) = (String::new(), String::new());
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.parse().unwrap_or(0),
                "host" => value.clone_into(&mut host),
                "authorization" => value.clone_into(&mut auth),
                _ => {}
            }
        }
    }
    while data.len() < header_end + length {
        let n = socket.read(&mut buf).await.ok()?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n]);
    }
    let body = serde_json::from_slice(&data[header_end..]).unwrap_or(Value::Null);
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let query = query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect();
    Some(Request {
        method,
        path: decode(path),
        query,
        host,
        auth,
        body,
    })
}

/// Whether an Access application covers the request.
fn protected(state: &Mutex<State>, req: &Request) -> bool {
    let host = req.host.split(':').next().unwrap_or_default();
    let target = format!("{host}{}", req.path);
    state
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .access_apps
        .iter()
        .filter_map(|a| a["domain"].as_str())
        .any(|domain| {
            target == domain
                || target
                    .strip_prefix(domain)
                    .is_some_and(|rest| rest.starts_with(['/', '?']))
        })
}

async fn serve(mut socket: TcpStream, state: Arc<Mutex<State>>, own_host: String) {
    let Some(req) = read_request(&mut socket).await else {
        return;
    };
    let mut headers = String::new();
    let (status, body, content_type) = if req.host == own_host || req.host.starts_with("127.0.0.1")
    {
        let (status, body) = handle(&state, &req);
        (status, body.to_string(), "application/json")
    } else if protected(&state, &req) {
        // The edge, in front of a login.
        let host = req.host.split(':').next().unwrap_or_default();
        headers =
            format!("Location: https://e2e.cloudflareaccess.com/cdn-cgi/access/login/{host}\r\n");
        (302, String::new(), "text/html")
    } else {
        // The edge, serving a route: pretend the origin answered.
        (200, format!("hello from {}\n", req.host), "text/plain")
    };
    let response = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    let Ok(listener) = TcpListener::bind(("127.0.0.1", port)).await else {
        return ExitCode::from(1);
    };
    let Ok(addr) = listener.local_addr() else {
        return ExitCode::from(1);
    };
    #[allow(clippy::print_stdout)]
    {
        println!("listening on {addr}");
    }
    let own_host = addr.to_string();
    let state = Arc::new(Mutex::new(State::default()));
    loop {
        if let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(serve(socket, Arc::clone(&state), own_host.clone()));
        }
    }
}
