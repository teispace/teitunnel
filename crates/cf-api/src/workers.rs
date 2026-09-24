//! Workers with static assets: what Snapshots are hosted on (docs/research/cloudflare-snapshots.md).
//!
//! The flow: an upload session with a manifest of file hashes → upload the files
//! Cloudflare doesn't have yet, bucket by bucket, with the session's JWT → a script (or a
//! version of it) that references the completion token → a deployment. Custom Domains
//! and the account's workers.dev subdomain give it an address. Shapes checked 2026-09-24.

use std::collections::BTreeMap;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Client, Error, Result, encode_query,
    multipart::{Part, encode as encode_form},
    resources::encode,
};

/// The media type of Worker modules.
pub const MODULE_TYPE: &str = "application/javascript+module";

/// One file in an assets manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetEntry {
    /// 32 hex characters identifying the content.
    pub hash: String,
    /// Size in bytes.
    pub size: u64,
}

/// An assets upload session: the JWT to upload with, and the hashes Cloudflare still
/// needs, grouped into the requests to send them in. No buckets: everything is already
/// there and `jwt` is the completion token.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UploadSession {
    /// Upload (or completion) token, valid for an hour.
    pub jwt: String,
    /// Hashes to upload, one request per bucket.
    #[serde(default)]
    pub buckets: Vec<Vec<String>>,
}

/// A file to upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetFile {
    /// Its manifest hash.
    pub hash: String,
    /// Media type served for it.
    pub content_type: String,
    /// The bytes.
    pub content: Vec<u8>,
}

/// A module of a Worker script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerModule {
    /// File name, e.g. `worker.js` (the metadata's `main_module`).
    pub name: String,
    /// JavaScript source.
    pub content: String,
}

/// A version of a Worker.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct WorkerVersion {
    /// Version id.
    pub id: String,
    /// Sequence number.
    #[serde(default)]
    pub number: Option<u64>,
}

/// A version's share of a deployment.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DeploymentVersion {
    /// Version id.
    pub version_id: String,
    /// Percentage of traffic.
    #[serde(default)]
    pub percentage: f64,
}

/// A deployment: which versions serve traffic.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WorkerDeployment {
    /// Deployment id.
    pub id: String,
    /// When it was made.
    #[serde(default)]
    pub created_on: Option<String>,
    /// Versions and their shares.
    #[serde(default)]
    pub versions: Vec<DeploymentVersion>,
}

impl WorkerDeployment {
    /// The version serving most of the traffic.
    pub fn main_version(&self) -> Option<&str> {
        self.versions
            .iter()
            .max_by(|a, b| a.percentage.total_cmp(&b.percentage))
            .map(|v| v.version_id.as_str())
    }
}

/// A Custom Domain: a hostname served by a Worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerDomain {
    /// Domain id.
    pub id: String,
    /// The hostname.
    pub hostname: String,
    /// The Worker serving it.
    pub service: String,
    /// Its zone.
    #[serde(default)]
    pub zone_id: String,
    /// The zone's name.
    #[serde(default)]
    pub zone_name: String,
}

fn script_path(account: &str, script: &str) -> String {
    format!(
        "/accounts/{}/workers/scripts/{}",
        encode(account),
        encode(script)
    )
}

/// The script's form: the metadata and its modules.
fn script_form(metadata: &Value, modules: &[WorkerModule]) -> crate::multipart::Body {
    let metadata = metadata.to_string();
    let mut parts = vec![Part {
        name: "metadata",
        filename: None,
        content_type: "application/json",
        content: metadata.as_bytes(),
    }];
    parts.extend(modules.iter().map(|m| Part {
        name: &m.name,
        filename: Some(&m.name),
        content_type: MODULE_TYPE,
        content: m.content.as_bytes(),
    }));
    encode_form(&parts)
}

fn missing(err: &Error) -> bool {
    err.status() == Some(404)
        || err
            .codes()
            .iter()
            .any(|c| matches!(c, 10007 | 10090 | 10092))
}

impl Client {
    /// The account's workers.dev subdomain (`name` in `name.workers.dev`), `None` if the
    /// account hasn't chosen one yet.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn workers_subdomain(&self, account: &str) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Subdomain {
            #[serde(default)]
            subdomain: Option<String>,
        }
        match self
            .get::<Subdomain>(&format!("/accounts/{}/workers/subdomain", encode(account)))
            .await
        {
            Ok(found) => Ok(found.subdomain.filter(|s| !s.is_empty())),
            Err(err) if missing(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Starts an assets upload for `script` with the full manifest (path → hash, size).
    ///
    /// # Errors
    /// API errors, e.g. too many files for the plan.
    pub async fn create_assets_upload_session(
        &self,
        account: &str,
        script: &str,
        manifest: &BTreeMap<String, AssetEntry>,
    ) -> Result<UploadSession> {
        self.post(
            &format!("{}/assets-upload-session", script_path(account, script)),
            &json!({ "manifest": manifest }),
        )
        .await
    }

    /// Uploads one bucket of files with the session's `jwt`. Returns the completion
    /// token when this was the last bucket Cloudflare needed.
    ///
    /// # Errors
    /// API or network errors (an expired JWT is a 401).
    pub async fn upload_assets(
        &self,
        account: &str,
        jwt: &str,
        files: &[AssetFile],
    ) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Uploaded {
            #[serde(default)]
            jwt: Option<String>,
        }
        let encoded: Vec<String> = files.iter().map(|f| STANDARD.encode(&f.content)).collect();
        let parts: Vec<Part<'_>> = files
            .iter()
            .zip(&encoded)
            .map(|(file, content)| Part {
                name: &file.hash,
                filename: Some(&file.hash),
                content_type: &file.content_type,
                content: content.as_bytes(),
            })
            .collect();
        let uploaded: Uploaded = self
            .send_body(
                Method::POST,
                &format!(
                    "/accounts/{}/workers/assets/upload?base64=true",
                    encode(account)
                ),
                encode_form(&parts),
                Some(jwt),
            )
            .await?;
        Ok(uploaded.jwt)
    }

    /// Creates or replaces a Worker and deploys it at once (`metadata` per the
    /// "Upload Worker Module" reference).
    ///
    /// # Errors
    /// API errors, e.g. an invalid script or an expired assets token.
    pub async fn put_worker_script(
        &self,
        account: &str,
        script: &str,
        metadata: &Value,
        modules: &[WorkerModule],
    ) -> Result<()> {
        self.send_body::<Value>(
            Method::PUT,
            &script_path(account, script),
            script_form(metadata, modules),
            None,
        )
        .await
        .map(|_| ())
    }

    /// Uploads a new version of an existing Worker without deploying it.
    ///
    /// # Errors
    /// API errors.
    pub async fn upload_worker_version(
        &self,
        account: &str,
        script: &str,
        metadata: &Value,
        modules: &[WorkerModule],
    ) -> Result<WorkerVersion> {
        self.send_body(
            Method::POST,
            &format!("{}/versions", script_path(account, script)),
            script_form(metadata, modules),
            None,
        )
        .await
    }

    /// A Worker's deployments, newest first; `None` if the Worker doesn't exist.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn worker_deployments(
        &self,
        account: &str,
        script: &str,
    ) -> Result<Option<Vec<WorkerDeployment>>> {
        #[derive(Deserialize)]
        struct Deployments {
            #[serde(default)]
            deployments: Vec<WorkerDeployment>,
        }
        match self
            .get::<Deployments>(&format!("{}/deployments", script_path(account, script)))
            .await
        {
            Ok(found) => Ok(Some(found.deployments)),
            Err(err) if missing(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }

    /// Sends all of a Worker's traffic to `version_id` (deploys it, or rolls back to it).
    ///
    /// # Errors
    /// API errors, e.g. a version older than the last 100.
    pub async fn deploy_worker_version(
        &self,
        account: &str,
        script: &str,
        version_id: &str,
        message: &str,
    ) -> Result<WorkerDeployment> {
        let message: String = message.chars().take(200).collect();
        self.post(
            &format!("{}/deployments", script_path(account, script)),
            &json!({
                "strategy": "percentage",
                "versions": [{ "version_id": version_id, "percentage": 100 }],
                "annotations": { "workers/message": message },
            }),
        )
        .await
    }

    /// Whether the Worker answers on the account's workers.dev subdomain.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn worker_on_workers_dev(&self, account: &str, script: &str) -> Result<bool> {
        #[derive(Deserialize)]
        struct State {
            #[serde(default)]
            enabled: bool,
        }
        Ok(self
            .get::<State>(&format!("{}/subdomain", script_path(account, script)))
            .await?
            .enabled)
    }

    /// Turns the Worker's workers.dev address on or off (preview URLs stay off).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn set_worker_on_workers_dev(
        &self,
        account: &str,
        script: &str,
        enabled: bool,
    ) -> Result<()> {
        self.post::<Value>(
            &format!("{}/subdomain", script_path(account, script)),
            &json!({ "enabled": enabled, "previews_enabled": false }),
        )
        .await
        .map(|_| ())
    }

    /// Deletes a Worker with its versions and assets (a missing one counts as deleted).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_worker_script(&self, account: &str, script: &str) -> Result<()> {
        self.delete(&format!("{}?force=true", script_path(account, script)))
            .await
    }

    /// Custom Domains, filtered by Worker and/or hostname.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn worker_domains(
        &self,
        account: &str,
        service: Option<&str>,
        hostname: Option<&str>,
    ) -> Result<Vec<WorkerDomain>> {
        let mut query = Vec::new();
        if let Some(service) = service {
            query.push(format!("service={}", encode_query(service)));
        }
        if let Some(hostname) = hostname {
            query.push(format!("hostname={}", encode_query(hostname)));
        }
        let mut path = format!("/accounts/{}/workers/domains", encode(account));
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query.join("&"));
        }
        self.get(&path).await
    }

    /// Serves `hostname` (in `zone_id`) with the Worker; Cloudflare creates its DNS
    /// record and certificate.
    ///
    /// # Errors
    /// API errors, e.g. an existing CNAME on the hostname.
    pub async fn attach_worker_domain(
        &self,
        account: &str,
        hostname: &str,
        zone_id: &str,
        service: &str,
    ) -> Result<WorkerDomain> {
        self.put(
            &format!("/accounts/{}/workers/domains", encode(account)),
            &json!({ "hostname": hostname, "zone_id": zone_id, "service": service }),
        )
        .await
    }

    /// Detaches a Custom Domain (its DNS record goes too; a missing one counts as gone).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn detach_worker_domain(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!(
            "/accounts/{}/workers/domains/{}",
            encode(account),
            encode(id)
        ))
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{body_json, header, method, path, query_param},
    };

    use super::*;
    use crate::ApiToken;

    #[allow(clippy::needless_pass_by_value)]
    fn ok(result: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result
        }))
    }

    fn error(status: u16, code: u32) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(json!({
            "success": false, "errors": [{"code": code, "message": "x"}], "messages": [], "result": null
        }))
    }

    async fn client() -> (MockServer, Client) {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("api-token"))
            .unwrap()
            .with_backoff(Duration::from_millis(5));
        (server, client)
    }

    fn body_text(request: &Request) -> String {
        String::from_utf8_lossy(&request.body).into_owned()
    }

    #[tokio::test]
    async fn reads_the_workers_dev_subdomain() {
        let (server, client) = client().await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/workers/subdomain"))
            .respond_with(ok(json!({"subdomain": "acme"})))
            .mount(&server)
            .await;
        assert_eq!(
            client.workers_subdomain("a1").await.unwrap().as_deref(),
            Some("acme")
        );
        let (server, client) = self::client().await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/workers/subdomain"))
            .respond_with(error(404, 10007))
            .mount(&server)
            .await;
        assert_eq!(client.workers_subdomain("a1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn opens_an_upload_session_with_the_manifest() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/workers/scripts/snap/assets-upload-session"))
            .and(header("authorization", "Bearer api-token"))
            .and(body_json(json!({
                "manifest": {"/index.html": {"hash": "0123456789abcdef0123456789abcdef", "size": 12}}
            })))
            .respond_with(ok(json!({"jwt": "upload-jwt", "buckets": [["0123456789abcdef0123456789abcdef"]]})))
            .expect(1)
            .mount(&server)
            .await;
        let manifest = BTreeMap::from([(
            "/index.html".to_owned(),
            AssetEntry {
                hash: "0123456789abcdef0123456789abcdef".into(),
                size: 12,
            },
        )]);
        let session = client
            .create_assets_upload_session("a1", "snap", &manifest)
            .await
            .unwrap();
        assert_eq!(session.jwt, "upload-jwt");
        assert_eq!(session.buckets.len(), 1);
    }

    #[tokio::test]
    async fn uploads_a_bucket_with_the_session_token_in_base64() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/workers/assets/upload"))
            .and(query_param("base64", "true"))
            .and(header("authorization", "Bearer upload-jwt"))
            .respond_with(ResponseTemplate::new(201).set_body_json(json!({
                "success": true, "errors": [], "messages": [], "result": {"jwt": "completion"}
            })))
            .expect(1)
            .mount(&server)
            .await;
        let done = client
            .upload_assets(
                "a1",
                "upload-jwt",
                &[AssetFile {
                    hash: "h1".into(),
                    content_type: "text/html".into(),
                    content: b"<h1>hi</h1>".to_vec(),
                }],
            )
            .await
            .unwrap();
        assert_eq!(done.as_deref(), Some("completion"));
        let requests = server.received_requests().await.unwrap();
        let body = body_text(&requests[0]);
        assert!(
            requests[0]
                .headers
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("multipart/form-data; boundary=")
        );
        assert!(body.contains("name=\"h1\"; filename=\"h1\"\r\nContent-Type: text/html"));
        assert!(body.contains(&STANDARD.encode("<h1>hi</h1>")));
        assert!(!body.contains("api-token"));
    }

    #[tokio::test]
    async fn a_middle_bucket_has_no_completion_token() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/workers/assets/upload"))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({
                "success": true, "errors": [], "messages": [], "result": {}
            })))
            .mount(&server)
            .await;
        assert_eq!(client.upload_assets("a1", "j", &[]).await.unwrap(), None);
    }

    #[tokio::test]
    async fn puts_a_script_with_metadata_and_module() {
        let (server, client) = client().await;
        Mock::given(method("PUT"))
            .and(path("/accounts/a1/workers/scripts/snap"))
            .respond_with(ok(json!({"id": "snap", "etag": "e"})))
            .expect(1)
            .mount(&server)
            .await;
        let metadata = json!({"main_module": "worker.js", "assets": {"jwt": "completion"}});
        client
            .put_worker_script(
                "a1",
                "snap",
                &metadata,
                &[WorkerModule {
                    name: "worker.js".into(),
                    content: "export default {}".into(),
                }],
            )
            .await
            .unwrap();
        let requests = server.received_requests().await.unwrap();
        let body = body_text(&requests[0]);
        assert!(body.contains("name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n{\"assets\":{\"jwt\":\"completion\"},\"main_module\":\"worker.js\"}"));
        assert!(body.contains(
            "name=\"worker.js\"; filename=\"worker.js\"\r\nContent-Type: application/javascript+module\r\n\r\nexport default {}"
        ));
    }

    #[tokio::test]
    async fn uploads_a_version_and_deploys_it() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/workers/scripts/snap/versions"))
            .respond_with(ok(json!({"id": "v2", "number": 2})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/workers/scripts/snap/deployments"))
            .and(body_json(json!({
                "strategy": "percentage",
                "versions": [{"version_id": "v2", "percentage": 100}],
                "annotations": {"workers/message": "Teitunnel"}
            })))
            .respond_with(ok(
                json!({"id": "d2", "versions": [{"version_id": "v2", "percentage": 100.0}]}),
            ))
            .expect(1)
            .mount(&server)
            .await;
        let version = client
            .upload_worker_version("a1", "snap", &json!({"main_module": "worker.js"}), &[])
            .await
            .unwrap();
        assert_eq!(
            version,
            WorkerVersion {
                id: "v2".into(),
                number: Some(2)
            }
        );
        let deployment = client
            .deploy_worker_version("a1", "snap", "v2", "Teitunnel")
            .await
            .unwrap();
        assert_eq!(deployment.main_version(), Some("v2"));
    }

    #[tokio::test]
    async fn lists_deployments_or_none_for_a_missing_worker() {
        let (server, client) = client().await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/workers/scripts/snap/deployments"))
            .respond_with(ok(json!({"deployments": [
                {"id": "d2", "created_on": "2026-09-24T10:00:00Z", "versions": [
                    {"version_id": "v1", "percentage": 10.0}, {"version_id": "v2", "percentage": 90.0}
                ]}
            ]})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/workers/scripts/gone/deployments"))
            .respond_with(error(404, 10007))
            .mount(&server)
            .await;
        let found = client
            .worker_deployments("a1", "snap")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found[0].main_version(), Some("v2"));
        assert_eq!(client.worker_deployments("a1", "gone").await.unwrap(), None);
    }

    #[tokio::test]
    async fn toggles_workers_dev_and_deletes_the_worker() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/workers/scripts/snap/subdomain"))
            .and(body_json(
                json!({"enabled": true, "previews_enabled": false}),
            ))
            .respond_with(ok(json!({"enabled": true, "previews_enabled": false})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/workers/scripts/snap/subdomain"))
            .respond_with(ok(json!({"enabled": true})))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/workers/scripts/snap"))
            .and(query_param("force", "true"))
            .respond_with(ok(Value::Null))
            .expect(1)
            .mount(&server)
            .await;
        client
            .set_worker_on_workers_dev("a1", "snap", true)
            .await
            .unwrap();
        assert!(client.worker_on_workers_dev("a1", "snap").await.unwrap());
        client.delete_worker_script("a1", "snap").await.unwrap();
    }

    #[tokio::test]
    async fn attaches_lists_and_detaches_custom_domains() {
        let (server, client) = client().await;
        let domain = json!({"id": "dom1", "hostname": "preview.xyz.com", "service": "snap",
                            "zone_id": "z1", "zone_name": "xyz.com"});
        Mock::given(method("PUT"))
            .and(path("/accounts/a1/workers/domains"))
            .and(body_json(
                json!({"hostname": "preview.xyz.com", "zone_id": "z1", "service": "snap"}),
            ))
            .respond_with(ok(domain.clone()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/workers/domains"))
            .and(query_param("service", "snap"))
            .respond_with(ok(json!([domain])))
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/workers/domains/dom1"))
            .respond_with(ok(Value::Null))
            .expect(1)
            .mount(&server)
            .await;
        let attached = client
            .attach_worker_domain("a1", "preview.xyz.com", "z1", "snap")
            .await
            .unwrap();
        assert_eq!(attached.id, "dom1");
        let listed = client
            .worker_domains("a1", Some("snap"), None)
            .await
            .unwrap();
        assert_eq!(listed, [attached]);
        client.detach_worker_domain("a1", "dom1").await.unwrap();
    }

    #[tokio::test]
    async fn reports_cloudflare_errors() {
        let (server, client) = client().await;
        Mock::given(method("PUT"))
            .and(path("/accounts/a1/workers/domains"))
            .respond_with(error(409, 100_117))
            .mount(&server)
            .await;
        let err = client
            .attach_worker_domain("a1", "www.xyz.com", "z1", "snap")
            .await
            .unwrap_err();
        assert_eq!(err.status(), Some(409));
        assert_eq!(err.codes(), [100_117]);
    }
}
