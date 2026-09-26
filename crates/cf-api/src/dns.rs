//! DNS record endpoints.

use serde::{Deserialize, Serialize};

use crate::{Client, Result};

/// A DNS record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DnsRecord {
    /// Record id.
    pub id: String,
    /// Full name, e.g. `app.xyz.com`.
    pub name: String,
    /// `A`, `AAAA`, `CNAME`, `TXT`, …
    #[serde(rename = "type")]
    pub kind: String,
    /// Target.
    pub content: String,
    /// Proxied through Cloudflare.
    #[serde(default)]
    pub proxied: bool,
    /// Free-text comment (Teitunnel writes its ownership marker here).
    #[serde(default)]
    pub comment: Option<String>,
    /// TTL in seconds (1 = automatic).
    #[serde(default = "auto_ttl")]
    pub ttl: u32,
}

const fn auto_ttl() -> u32 {
    1
}

impl DnsRecord {
    /// The same record as a create/update body (to restore it after a failed change).
    pub fn to_new(&self) -> NewDnsRecord {
        NewDnsRecord {
            name: self.name.clone(),
            kind: self.kind.clone(),
            content: self.content.clone(),
            proxied: self.proxied,
            ttl: self.ttl,
            comment: self.comment.clone(),
        }
    }
}

/// A record to create or update.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewDnsRecord {
    /// Full name.
    pub name: String,
    /// Record type.
    #[serde(rename = "type")]
    pub kind: String,
    /// Target.
    pub content: String,
    /// Proxied through Cloudflare.
    pub proxied: bool,
    /// TTL (1 = automatic).
    pub ttl: u32,
    /// Comment.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// The most records one batch call may change on every plan (Free allows 200, paid plans
/// 3,500).
pub const MAX_DNS_BATCH: usize = 200;

/// What a batch call created.
#[derive(Deserialize)]
struct Batch {
    #[serde(default)]
    posts: Vec<DnsRecord>,
}

fn records_path(zone: &str) -> String {
    format!("/zones/{}/dns_records", crate::encode(zone))
}

impl Client {
    /// Records in a zone with exactly this name (all types).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn dns_records_named(&self, zone: &str, name: &str) -> Result<Vec<DnsRecord>> {
        self.get_all(&format!(
            "{}?name={}",
            records_path(zone),
            crate::encode_query(name)
        ))
        .await
    }

    /// Records of one type in a zone, e.g. every `CNAME` (orphan scans).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn dns_records_of_type(&self, zone: &str, kind: &str) -> Result<Vec<DnsRecord>> {
        self.get_all(&format!(
            "{}?type={}",
            records_path(zone),
            crate::encode_query(kind)
        ))
        .await
    }

    /// Records in a zone whose comment contains `needle` (ownership scans).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn dns_records_with_comment(
        &self,
        zone: &str,
        needle: &str,
    ) -> Result<Vec<DnsRecord>> {
        self.get_all(&format!(
            "{}?comment.contains={}",
            records_path(zone),
            crate::encode_query(needle)
        ))
        .await
    }

    /// CNAME records in a zone pointing at `content` (e.g. `<tunnel>.cfargotunnel.com`).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn dns_records_with_content(
        &self,
        zone: &str,
        content: &str,
    ) -> Result<Vec<DnsRecord>> {
        self.get_all(&format!(
            "{}?type=CNAME&content={}",
            records_path(zone),
            crate::encode_query(content)
        ))
        .await
    }

    /// Creates a record. Not retried on server errors (no duplicates).
    ///
    /// # Errors
    /// API errors such as 81053 (a record with that name already exists).
    pub async fn create_dns_record(&self, zone: &str, record: &NewDnsRecord) -> Result<DnsRecord> {
        self.post(&records_path(zone), &serde_json::to_value(record)?)
            .await
    }

    /// Updates a record.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn update_dns_record(
        &self,
        zone: &str,
        id: &str,
        record: &NewDnsRecord,
    ) -> Result<DnsRecord> {
        self.patch(
            &format!("{}/{}", records_path(zone), crate::encode(id)),
            &serde_json::to_value(record)?,
        )
        .await
    }

    /// Creates several records in one batch: Cloudflare applies all of them or none, and
    /// returns them in the order given. At most [`MAX_DNS_BATCH`] records per call.
    /// <https://developers.cloudflare.com/dns/manage-dns-records/how-to/batch-record-changes/>
    ///
    /// # Errors
    /// API or network errors (the first record Cloudflare refused); nothing changed then.
    pub async fn create_dns_records(
        &self,
        zone: &str,
        records: &[NewDnsRecord],
    ) -> Result<Vec<DnsRecord>> {
        let posts = self
            .batch(zone, &serde_json::json!({ "posts": records }))
            .await?
            .posts;
        if posts.len() != records.len() {
            return Err(crate::Error::Decode(serde::de::Error::custom(format!(
                "the batch created {} of {} records",
                posts.len(),
                records.len()
            ))));
        }
        Ok(posts)
    }

    /// Replaces a record with another in one batch (the delete runs first, and neither
    /// applies unless both do). Cloudflare doesn't change a record's type in place
    /// (since 2026-06-30), so an A or AAAA record becomes a CNAME this way.
    ///
    /// # Errors
    /// API or network errors; nothing changed then.
    pub async fn replace_dns_record(
        &self,
        zone: &str,
        id: &str,
        record: &NewDnsRecord,
    ) -> Result<DnsRecord> {
        self.batch(
            zone,
            &serde_json::json!({ "deletes": [{ "id": id }], "posts": [record] }),
        )
        .await?
        .posts
        .into_iter()
        .next()
        .ok_or_else(|| {
            crate::Error::Decode(serde::de::Error::custom("the batch created no record"))
        })
    }

    /// One call to the batch endpoint: deletes, patches, puts, then posts, in one
    /// transaction.
    async fn batch(&self, zone: &str, body: &serde_json::Value) -> Result<Batch> {
        self.post(&format!("{}/batch", records_path(zone)), body)
            .await
    }

    /// Deletes a record (already-gone counts as success).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_dns_record(&self, zone: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", records_path(zone), crate::encode(id)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use crate::{ApiToken, Client, IngressRule, NewDnsRecord, TunnelConfig};

    fn ok(result: &serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true, "errors": [], "messages": [], "result": result,
            "result_info": {"page": 1, "per_page": 50, "total_pages": 1}
        }))
    }

    #[tokio::test]
    async fn endpoints_send_the_expected_requests() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();

        Mock::given(method("POST"))
            .and(path("/accounts/a1/cfd_tunnel"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(
                    body,
                    serde_json::json!({"name": "My Mac", "config_src": "cloudflare"})
                );
                ok(&serde_json::json!({"id": "t1", "name": "My Mac"}))
            })
            .mount(&server)
            .await;
        assert_eq!(client.create_tunnel("a1", "My Mac").await.unwrap().id, "t1");

        Mock::given(method("GET"))
            .and(path("/zones/z1/dns_records"))
            .and(query_param("name", "app.xyz.com"))
            .respond_with(ok(&serde_json::json!([{"id": "r1", "name": "app.xyz.com", "type": "CNAME", "content": "t1.cfargotunnel.com", "proxied": true, "comment": "teitunnel:route=x"}])))
            .mount(&server)
            .await;
        let records = client.dns_records_named("z1", "app.xyz.com").await.unwrap();
        assert_eq!(records[0].comment.as_deref(), Some("teitunnel:route=x"));

        Mock::given(method("POST"))
            .and(path("/zones/z1/dns_records"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["type"], "CNAME");
                assert_eq!(body["proxied"], true);
                ok(&serde_json::json!({"id": "r2", "name": body["name"], "type": "CNAME", "content": body["content"], "proxied": true}))
            })
            .mount(&server)
            .await;
        let created = client
            .create_dns_record(
                "z1",
                &NewDnsRecord {
                    name: "b.xyz.com".into(),
                    kind: "CNAME".into(),
                    content: "t1.cfargotunnel.com".into(),
                    proxied: true,
                    ttl: 1,
                    comment: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(created.id, "r2");

        Mock::given(method("PUT"))
            .and(path("/accounts/a1/cfd_tunnel/t1/configurations"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["config"]["ingress"][0]["service"], "http_status:404");
                ok(&serde_json::json!({"version": 8, "config": body["config"]}))
            })
            .mount(&server)
            .await;
        let config = TunnelConfig {
            ingress: vec![IngressRule {
                hostname: None,
                path: None,
                service: "http_status:404".into(),
                origin_request: Default::default(),
                extra: Default::default(),
            }],
            origin_request: Default::default(),
            extra: Default::default(),
        };
        assert_eq!(
            client
                .put_tunnel_config("a1", "t1", &config)
                .await
                .unwrap()
                .version,
            8
        );
    }

    #[tokio::test]
    async fn replaces_a_record_in_one_batch() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("POST"))
            .and(path("/zones/z1/dns_records/batch"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body["deletes"], serde_json::json!([{"id": "old"}]));
                assert_eq!(body["posts"][0]["type"], "CNAME");
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "success": true, "errors": [], "messages": [],
                    "result": {"deletes": [{"id": "old", "name": "app.xyz.com", "type": "A", "content": "192.0.2.1"}],
                               "posts": [{"id": "new", "name": "app.xyz.com", "type": "CNAME",
                                          "content": "t1.cfargotunnel.com", "proxied": true}]}
                }))
            })
            .expect(1)
            .mount(&server)
            .await;
        let record = NewDnsRecord {
            name: "app.xyz.com".into(),
            kind: "CNAME".into(),
            content: "t1.cfargotunnel.com".into(),
            proxied: true,
            ttl: 1,
            comment: None,
        };
        let created = client
            .replace_dns_record("z1", "old", &record)
            .await
            .unwrap();
        assert_eq!(
            (created.id.as_str(), created.kind.as_str()),
            ("new", "CNAME")
        );
    }

    fn cname(name: &str) -> NewDnsRecord {
        NewDnsRecord {
            name: name.into(),
            kind: "CNAME".into(),
            content: "t1.cfargotunnel.com".into(),
            proxied: true,
            ttl: 1,
            comment: Some("teitunnel:route=r".into()),
        }
    }

    #[tokio::test]
    async fn creates_records_in_one_batch() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("POST"))
            .and(path("/zones/z1/dns_records/batch"))
            .respond_with(|req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                assert!(body.get("deletes").is_none());
                let posts = body["posts"].as_array().unwrap();
                let created: Vec<_> = posts
                    .iter()
                    .enumerate()
                    .map(|(i, p)| {
                        let mut record = p.clone();
                        record["id"] = serde_json::json!(format!("r{i}"));
                        record
                    })
                    .collect();
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "success": true, "errors": [], "messages": [],
                    "result": {"posts": created}
                }))
            })
            .expect(1)
            .mount(&server)
            .await;
        let created = client
            .create_dns_records("z1", &[cname("a.xyz.com"), cname("b.xyz.com")])
            .await
            .unwrap();
        let names: Vec<_> = created
            .iter()
            .map(|r| (r.id.as_str(), r.name.as_str()))
            .collect();
        assert_eq!(names, [("r0", "a.xyz.com"), ("r1", "b.xyz.com")]);
    }

    #[tokio::test]
    async fn a_refused_batch_is_an_error() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("POST"))
            .and(path("/zones/z1/dns_records/batch"))
            .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                "success": false, "messages": [], "result": null,
                "errors": [{"code": 81053, "message": "An A, AAAA, or CNAME record with that host already exists."}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        let error = client
            .create_dns_records("z1", &[cname("a.xyz.com"), cname("b.xyz.com")])
            .await
            .unwrap_err();
        assert!(
            matches!(&error, crate::Error::Api { errors, .. } if errors[0].code == 81053),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn a_batch_that_creates_fewer_records_is_an_error() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("POST"))
            .and(path("/zones/z1/dns_records/batch"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "errors": [], "messages": [],
                "result": {"posts": [{"id": "r0", "name": "a.xyz.com", "type": "CNAME",
                                      "content": "t1.cfargotunnel.com", "proxied": true}]}
            })))
            .mount(&server)
            .await;
        let error = client
            .create_dns_records("z1", &[cname("a.xyz.com"), cname("b.xyz.com")])
            .await
            .unwrap_err();
        assert!(matches!(error, crate::Error::Decode(_)), "{error:?}");
    }
}
