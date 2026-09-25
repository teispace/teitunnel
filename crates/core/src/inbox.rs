//! Webhook inbox delivery (M12-12): webhooks the inbox Worker kept while this computer
//! was off (`engine::front`, `inbox-worker.js`) are read from the account's D1 database
//! and delivered in order to the route's local service once it's back.
//!
//! Delivery goes to the service the route points at (its ingress rule), so a route
//! being inspected delivers through Lens and the inspector shows each delivery next to
//! the webhook's arrival time (`X-Teitunnel-Inbox`, `X-Teitunnel-Received-At`). A
//! delivery answered below 500 is done; a 5xx or no answer stops the round (order is
//! kept) and is retried later. Delivered webhooks are deleted after the inbox's
//! retention; the Worker deletes anything older on each write too.

use std::time::Duration;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use cf_api::D1Statement;
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    domain_shares::now_ms,
    engine::{
        CloudApi, Engine,
        front::{FrontConfig, FrontRow},
    },
};

/// Webhooks delivered per round and inbox.
pub const BATCH: usize = 20;
/// How long one delivery may take.
const DELIVERY_TIMEOUT: Duration = Duration::from_secs(30);

/// A kept webhook, as the app and the inspector show it (never its body).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    /// Id (sent along as `X-Teitunnel-Inbox`).
    pub id: String,
    /// When it arrived at Cloudflare (ms).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub received_at: u64,
    /// Method.
    pub method: String,
    /// Path and query.
    pub path: String,
    /// Body size in bytes.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub size: u64,
    /// When it was delivered (ms).
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub delivered_at: Option<u64>,
    /// What the local service answered.
    pub status: Option<u16>,
    /// Delivery attempts.
    pub attempts: u32,
    /// Why the last attempt failed.
    pub error: Option<String>,
}

/// What a delivery round did for one inbox.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct DrainReport {
    /// The hostname.
    pub hostname: String,
    /// The inbox path.
    pub path: String,
    /// Delivered this round.
    pub delivered: u32,
    /// Still waiting.
    pub waiting: u32,
    /// Why delivery stopped, if it did.
    pub error: Option<String>,
}

fn num(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|f| f.max(0.0) as u64))
        .unwrap_or_default()
}

fn item(row: &Value) -> Option<InboxItem> {
    Some(InboxItem {
        id: row["id"].as_str()?.to_owned(),
        received_at: num(&row["received_at"]),
        method: row["method"].as_str()?.to_owned(),
        path: row["path"].as_str()?.to_owned(),
        size: num(&row["size"]),
        delivered_at: row["delivered_at"]
            .as_f64()
            .map(|_| num(&row["delivered_at"])),
        status: row["status"].as_u64().and_then(|s| u16::try_from(s).ok()),
        attempts: u32::try_from(num(&row["attempts"])).unwrap_or(u32::MAX),
        error: row["error"].as_str().map(str::to_owned),
    })
}

/// The inbox's recent webhooks, newest first (waiting and delivered).
///
/// # Errors
/// API errors (a missing table reads as empty).
pub async fn items<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    script: &str,
    limit: u32,
) -> Result<Vec<InboxItem>, cf_api::Error> {
    let result = api
        .d1_query(
            account,
            database,
            &[D1Statement::new(
                "SELECT id, received_at, method, path, size, delivered_at, status, attempts, error FROM teitunnel_inbox WHERE inbox = ?1 ORDER BY seq DESC LIMIT ?2",
                vec![json!(script), json!(limit.min(500))],
            )],
        )
        .await;
    match result {
        Ok(results) => Ok(results
            .iter()
            .flat_map(|r| r.results.iter())
            .filter_map(item)
            .collect()),
        Err(err) if err.detail().contains("no such table") => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

/// Where webhooks for `hostname` and `path` go on this computer: the route's service
/// (`http://localhost:3000`, or Lens's address while it's inspected).
pub fn service_for(config: &cf_api::TunnelConfig, hostname: &str, path: &str) -> Option<String> {
    config
        .ingress
        .iter()
        .filter(|rule| {
            rule.hostname
                .as_deref()
                .is_some_and(|h| h.eq_ignore_ascii_case(hostname))
        })
        .find(|rule| {
            rule.path
                .as_deref()
                .is_none_or(|p| regex::Regex::new(p).is_ok_and(|re| re.is_match(path)))
        })
        .map(|rule| rule.service.clone())
        .filter(|s| s.starts_with("http://") || s.starts_with("https://"))
}

/// The HTTP client deliveries use: no proxy, no redirects, a timeout.
///
/// # Errors
/// The TLS backend couldn't start.
pub fn client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(DELIVERY_TIMEOUT)
        .build()
}

struct Pending {
    seq: i64,
    id: String,
    received_at: u64,
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn pending(row: &Value) -> Option<Pending> {
    Some(Pending {
        seq: row["seq"].as_i64()?,
        id: row["id"].as_str()?.to_owned(),
        received_at: num(&row["received_at"]),
        method: row["method"].as_str()?.to_owned(),
        path: row["path"].as_str()?.to_owned(),
        headers: serde_json::from_str(row["headers"].as_str().unwrap_or("[]")).ok()?,
        body: row["body"]
            .as_str()
            .map_or(Ok(Vec::new()), |b| STANDARD.decode(b))
            .ok()?,
    })
}

async fn deliver(
    http: &reqwest::Client,
    service: &str,
    hostname: &str,
    webhook: &Pending,
) -> Result<u16, String> {
    let url = format!("{}{}", service.trim_end_matches('/'), webhook.path);
    let method =
        reqwest::Method::from_bytes(webhook.method.as_bytes()).map_err(|e| e.to_string())?;
    let mut request = http.request(method, &url);
    for (name, value) in &webhook.headers {
        if name.eq_ignore_ascii_case("host") {
            continue;
        }
        request = request.header(name, value);
    }
    request = request
        .header("host", hostname)
        .header("x-teitunnel-inbox", &webhook.id)
        .header("x-teitunnel-received-at", webhook.received_at.to_string())
        .body(webhook.body.clone());
    let response = request.send().await.map_err(|e| e.to_string())?;
    Ok(response.status().as_u16())
}

/// Delivers what's waiting in one inbox, in order, to `service`.
///
/// # Errors
/// API errors reading or marking webhooks (delivery failures are in the report).
pub async fn drain<C: CloudApi>(
    api: &C,
    http: &reqwest::Client,
    account: &str,
    database: &str,
    front: &FrontRow,
    service: &str,
) -> Result<DrainReport, cf_api::Error> {
    let FrontConfig::Inbox { path, settings } = &front.config else {
        return Ok(DrainReport::default());
    };
    let mut report = DrainReport {
        hostname: front.hostname.clone(),
        path: path.clone(),
        ..DrainReport::default()
    };
    let results = match api
        .d1_query(
            account,
            database,
            &[D1Statement::new(
                "SELECT seq, id, received_at, method, path, headers, body FROM teitunnel_inbox WHERE inbox = ?1 AND delivered_at IS NULL ORDER BY seq LIMIT ?2",
                vec![json!(front.script), json!(BATCH)],
            )],
        )
        .await
    {
        Ok(results) => results,
        Err(err) if err.detail().contains("no such table") => return Ok(report),
        Err(err) => return Err(err),
    };
    let waiting: Vec<Pending> = results
        .iter()
        .flat_map(|r| r.results.iter())
        .filter_map(pending)
        .collect();
    let total = waiting.len();
    for webhook in waiting {
        match deliver(http, service, &front.hostname, &webhook).await {
            Ok(status) if status < 500 => {
                api.d1_query(
                    account,
                    database,
                    &[D1Statement::new(
                        "UPDATE teitunnel_inbox SET delivered_at = ?2, status = ?3, attempts = attempts + 1, error = NULL WHERE seq = ?1",
                        vec![json!(webhook.seq), json!(now_ms()), json!(status)],
                    )],
                )
                .await?;
                report.delivered += 1;
            }
            outcome => {
                let error = match outcome {
                    Ok(status) => format!("{service} answered {status}"),
                    Err(err) => err,
                };
                api.d1_query(
                    account,
                    database,
                    &[D1Statement::new(
                        "UPDATE teitunnel_inbox SET attempts = attempts + 1, error = ?2 WHERE seq = ?1",
                        vec![json!(webhook.seq), json!(error.chars().take(300).collect::<String>())],
                    )],
                )
                .await?;
                report.error = Some(error);
                break;
            }
        }
    }
    report.waiting = u32::try_from(total).unwrap_or(u32::MAX) - report.delivered;
    // Delivered webhooks go after the retention.
    let cutoff = now_ms().saturating_sub(u64::from(settings.retention_days) * 86_400_000);
    api.d1_query(
        account,
        database,
        &[D1Statement::new(
            "DELETE FROM teitunnel_inbox WHERE inbox = ?1 AND delivered_at IS NOT NULL AND delivered_at < ?2",
            vec![json!(front.script), json!(cutoff)],
        )],
    )
    .await?;
    Ok(report)
}

/// One delivery round for every inbox on `account` whose route this computer serves.
/// Returns what was delivered (for notifications and the UI).
///
/// # Errors
/// Database errors; API errors for one inbox are in its report.
pub async fn drain_account<C: CloudApi>(
    engine: &Engine,
    api: &C,
    http: &reqwest::Client,
    account: &str,
) -> Result<Vec<DrainReport>, crate::store::StoreError> {
    let local = engine.local();
    let inboxes: Vec<FrontRow> = local
        .fronts(Some(account), None)
        .await?
        .into_iter()
        .map(|(_, row)| row)
        .filter(|row| matches!(row.config, FrontConfig::Inbox { .. }) && row.route_id.is_some())
        .collect();
    if inboxes.is_empty() {
        return Ok(Vec::new());
    }
    let Some(database) = local.cloud_database(account).await? else {
        return Ok(Vec::new());
    };
    let mut configs = Vec::new();
    for tunnel in local.tunnels(account).await? {
        if let Ok(versioned) = api.tunnel_config(account, &tunnel.tunnel_id).await
            && let Some(config) = versioned.config
        {
            configs.push(config);
        }
    }
    let mut reports = Vec::new();
    for inbox in &inboxes {
        let path = inbox.config.path();
        let Some(service) = configs
            .iter()
            .find_map(|config| service_for(config, &inbox.hostname, path))
        else {
            // Another computer serves this route: it delivers.
            continue;
        };
        match drain(api, http, account, &database, inbox, &service).await {
            Ok(report) => reports.push(report),
            Err(err) => reports.push(DrainReport {
                hostname: inbox.hostname.clone(),
                path: path.to_owned(),
                error: Some(err.detail()),
                ..DrainReport::default()
            }),
        }
    }
    Ok(reports)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::engine::{
        fake::{CloudState, FakeCloud},
        front::InboxSettings,
    };

    fn rule(hostname: &str, path: Option<&str>, service: &str) -> cf_api::IngressRule {
        cf_api::IngressRule {
            hostname: Some(hostname.into()),
            path: path.map(str::to_owned),
            service: service.into(),
            origin_request: serde_json::Map::new(),
            extra: serde_json::Map::new(),
        }
    }

    #[test]
    fn finds_the_service_a_webhook_goes_to() {
        let config = cf_api::TunnelConfig {
            ingress: vec![
                rule("app.xyz.com", Some("^/api"), "http://localhost:4000"),
                rule("app.xyz.com", None, "http://127.0.0.1:59123"),
                rule("ssh.xyz.com", None, "ssh://localhost:22"),
            ],
            origin_request: serde_json::Map::new(),
            extra: serde_json::Map::new(),
        };
        assert_eq!(
            service_for(&config, "APP.xyz.com", "/api/hook").as_deref(),
            Some("http://localhost:4000")
        );
        assert_eq!(
            service_for(&config, "app.xyz.com", "/hooks/").as_deref(),
            Some("http://127.0.0.1:59123")
        );
        assert_eq!(service_for(&config, "ssh.xyz.com", "/"), None);
        assert_eq!(service_for(&config, "other.xyz.com", "/"), None);
    }

    type Seen = Arc<Mutex<Vec<(String, String, String, Vec<u8>)>>>;

    /// A local service that records what it gets and answers `statuses` in turn.
    async fn service(statuses: Vec<u16>) -> (String, Seen) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let record = Arc::clone(&seen);
        tokio::spawn(async move {
            for status in statuses {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let mut buf = vec![0u8; 65536];
                let mut data = Vec::new();
                loop {
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    data.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&data).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let length = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                            })
                            .unwrap_or(0);
                        if data.len() >= end + 4 + length || n == 0 {
                            let head = text[..end].to_owned();
                            let first = head.lines().next().unwrap_or_default().to_owned();
                            let inbox = head
                                .lines()
                                .find_map(|l| l.strip_prefix("x-teitunnel-inbox: "))
                                .unwrap_or_default()
                                .to_owned();
                            record.lock().unwrap().push((
                                first,
                                inbox,
                                head.to_ascii_lowercase(),
                                data[end + 4..].to_vec(),
                            ));
                            break;
                        }
                    }
                    if n == 0 {
                        break;
                    }
                }
                let reply = format!(
                    "HTTP/1.1 {status} X\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        (url, seen)
    }

    async fn inbox_with(cloud: &FakeCloud, rows: &[(&str, &str)]) -> String {
        let db = cloud
            .create_d1_database("acc", crate::comments::remote::DATABASE_NAME)
            .await
            .unwrap()
            .uuid;
        crate::engine::front::create_tables(cloud, "acc", &db)
            .await
            .unwrap();
        for (i, (id, body)) in rows.iter().enumerate() {
            cloud
                .d1_query(
                    "acc",
                    &db,
                    &[D1Statement::new(
                        "INSERT INTO teitunnel_inbox (inbox, id, received_at, method, path, headers, body, size) VALUES ('tt-inbox-1', ?1, ?2, 'POST', '/hooks/github?x=1', ?3, ?4, ?5)",
                        vec![
                            json!(id),
                            json!(1000 + i),
                            json!(r#"[["content-type","application/json"],["x-hub-signature-256","sha256=abc"]]"#),
                            json!(STANDARD.encode(body)),
                            json!(body.len()),
                        ],
                    )],
                )
                .await
                .unwrap();
        }
        db
    }

    fn row() -> FrontRow {
        FrontRow {
            hostname: "app.xyz.com".into(),
            config: FrontConfig::Inbox {
                path: "/hooks/".into(),
                settings: InboxSettings::default(),
            },
            script: "tt-inbox-1".into(),
            zone_id: "z".into(),
            route_id: Some("r".into()),
        }
    }

    #[tokio::test]
    async fn delivers_in_order_with_the_original_headers() {
        let cloud = FakeCloud::new(CloudState::default());
        let db = inbox_with(&cloud, &[("w1", "{\"n\":1}"), ("w2", "{\"n\":2}")]).await;
        let (url, seen) = service(vec![200, 204]).await;
        let report = drain(&cloud, &client().unwrap(), "acc", &db, &row(), &url)
            .await
            .unwrap();
        assert_eq!((report.delivered, report.waiting), (2, 0));
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen[0].0, "POST /hooks/github?x=1 HTTP/1.1");
        assert_eq!((seen[0].1.as_str(), seen[1].1.as_str()), ("w1", "w2"));
        assert!(seen[0].2.contains("x-hub-signature-256: sha256=abc"));
        assert!(seen[0].2.contains("host: app.xyz.com"));
        assert_eq!(seen[1].3, b"{\"n\":2}");
        let listed = items(&cloud, "acc", &db, "tt-inbox-1", 10).await.unwrap();
        assert!(listed.iter().all(|i| i.delivered_at.is_some()));
        assert_eq!(listed[0].status, Some(204));
    }

    #[tokio::test]
    async fn a_failure_stops_the_round_to_keep_the_order() {
        let cloud = FakeCloud::new(CloudState::default());
        let db = inbox_with(&cloud, &[("w1", "a"), ("w2", "b")]).await;
        let (url, seen) = service(vec![503]).await;
        let report = drain(&cloud, &client().unwrap(), "acc", &db, &row(), &url)
            .await
            .unwrap();
        assert_eq!((report.delivered, report.waiting), (0, 2));
        assert!(report.error.unwrap().contains("503"));
        assert_eq!(seen.lock().unwrap().len(), 1);
        let listed = items(&cloud, "acc", &db, "tt-inbox-1", 10).await.unwrap();
        let first = listed.iter().find(|i| i.id == "w1").unwrap();
        assert_eq!((first.attempts, first.delivered_at), (1, None));
        // Nothing listening: also kept for later.
        let report = drain(
            &cloud,
            &client().unwrap(),
            "acc",
            &db,
            &row(),
            "http://127.0.0.1:9",
        )
        .await
        .unwrap();
        assert_eq!(report.delivered, 0);
        assert!(report.error.is_some());
    }

    #[tokio::test]
    async fn an_inbox_nobody_wrote_to_is_empty() {
        let cloud = FakeCloud::new(CloudState::default());
        let db = cloud
            .create_d1_database("acc", "teitunnel-data")
            .await
            .unwrap()
            .uuid;
        assert!(
            items(&cloud, "acc", &db, "tt-inbox-1", 10)
                .await
                .unwrap()
                .is_empty()
        );
        let report = drain(
            &cloud,
            &client().unwrap(),
            "acc",
            &db,
            &row(),
            "http://127.0.0.1:9",
        )
        .await
        .unwrap();
        assert_eq!(report.delivered, 0);
    }
}
