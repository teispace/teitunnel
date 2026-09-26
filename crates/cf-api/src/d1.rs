//! D1 databases: where Snapshot comments and webhook inboxes are kept on the user's
//! account (<https://developers.cloudflare.com/api/resources/d1/>).
//!
//! The Workers read and write through their `d1` binding; the app reads and answers
//! through the query endpoint with the account's own token (D1 Read/Write), so no
//! Worker exposes an owner API.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Client, Result, encode_query, resources::encode};

/// A D1 database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct D1Database {
    /// Its id (a UUID), used by bindings and the query endpoint.
    pub uuid: String,
    /// Its name.
    pub name: String,
    /// When it was made (ISO 8601).
    #[serde(default)]
    pub created_at: Option<String>,
}

/// What a statement did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct D1Meta {
    /// Rows changed.
    #[serde(default)]
    pub changes: u64,
    /// The last inserted row id.
    #[serde(default)]
    pub last_row_id: i64,
    /// Rows read (counted against the daily free limit).
    #[serde(default)]
    pub rows_read: u64,
    /// Rows written.
    #[serde(default)]
    pub rows_written: u64,
}

/// One statement's result.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct D1Result {
    /// Rows, as JSON objects keyed by column.
    #[serde(default)]
    pub results: Vec<Value>,
    /// Whether it succeeded.
    #[serde(default)]
    pub success: bool,
    /// Counts.
    #[serde(default)]
    pub meta: D1Meta,
}

/// A statement with its bound parameters (`?1`, `?2`… or `?`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct D1Statement {
    /// SQL.
    pub sql: String,
    /// Parameters (strings, numbers, null).
    pub params: Vec<Value>,
}

impl D1Statement {
    /// A statement.
    pub fn new(sql: impl Into<String>, params: Vec<Value>) -> Self {
        Self {
            sql: sql.into(),
            params,
        }
    }
}

fn databases_path(account: &str) -> String {
    format!("/accounts/{}/d1/database", encode(account))
}

impl Client {
    /// Databases named exactly `name` (D1 Read).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn d1_databases(&self, account: &str, name: &str) -> Result<Vec<D1Database>> {
        let found: Vec<D1Database> = self
            .get_all(&format!(
                "{}?name={}",
                databases_path(account),
                encode_query(name)
            ))
            .await?;
        Ok(found.into_iter().filter(|d| d.name == name).collect())
    }

    /// Creates a database (D1 Write).
    ///
    /// # Errors
    /// API errors, e.g. the account's 10 free databases are used up.
    pub async fn create_d1_database(&self, account: &str, name: &str) -> Result<D1Database> {
        self.post(&databases_path(account), &json!({ "name": name }))
            .await
    }

    /// Deletes a database and everything in it (a missing one counts as deleted).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_d1_database(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", databases_path(account), encode(id)))
            .await
    }

    /// Runs statements in order (one request; D1 runs a batch as a transaction). Not
    /// retried, so a write never happens twice.
    ///
    /// # Errors
    /// API errors (a SQL error comes back as an API error), network failures.
    pub async fn d1_query(
        &self,
        account: &str,
        database: &str,
        statements: &[D1Statement],
    ) -> Result<Vec<D1Result>> {
        let path = format!("{}/{}/query", databases_path(account), encode(database));
        let body = match statements {
            [one] => json!({ "sql": one.sql, "params": one.params }),
            many => json!({ "batch": many }),
        };
        self.post(&path, &body).await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{body_json, header, method, path, query_param},
    };

    use super::*;
    use crate::ApiToken;

    #[allow(clippy::needless_pass_by_value)]
    fn ok(result: Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result,
            "result_info": {"page": 1, "per_page": 50, "count": 1, "total_count": 1, "total_pages": 1}
        }))
    }

    async fn client() -> (MockServer, Client) {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("api-token"))
            .unwrap()
            .with_backoff(Duration::from_millis(5));
        (server, client)
    }

    #[tokio::test]
    async fn finds_a_database_by_its_exact_name() {
        let (server, client) = client().await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/d1/database"))
            .and(query_param("name", "teitunnel-data"))
            .respond_with(ok(json!([
                {"uuid": "u1", "name": "teitunnel-data", "created_at": "2026-09-25T00:00:00Z"},
                {"uuid": "u2", "name": "teitunnel-data-old"}
            ])))
            .mount(&server)
            .await;
        let found = client.d1_databases("a1", "teitunnel-data").await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, "u1");
    }

    #[tokio::test]
    async fn creates_and_deletes_a_database() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/d1/database"))
            .and(header("authorization", "Bearer api-token"))
            .and(body_json(json!({"name": "teitunnel-data"})))
            .respond_with(ok(json!({"uuid": "u1", "name": "teitunnel-data"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/d1/database/u1"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        let db = client
            .create_d1_database("a1", "teitunnel-data")
            .await
            .unwrap();
        assert_eq!(db.uuid, "u1");
        client.delete_d1_database("a1", "u1").await.unwrap();
    }

    #[tokio::test]
    async fn queries_one_statement_or_a_batch() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/d1/database/u1/query"))
            .and(body_json(
                json!({"sql": "SELECT * FROM t WHERE a = ?1", "params": ["x"]}),
            ))
            .respond_with(ok(json!([{
                "results": [{"a": "x", "n": 1}], "success": true,
                "meta": {"changes": 0, "last_row_id": 0, "rows_read": 1, "rows_written": 0}
            }])))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/d1/database/u1/query"))
            .and(body_json(json!({"batch": [
                {"sql": "INSERT INTO t VALUES (?1)", "params": [1]},
                {"sql": "DELETE FROM t", "params": []}
            ]})))
            .respond_with(ok(json!([
                {"results": [], "success": true, "meta": {"changes": 1, "last_row_id": 7}},
                {"results": [], "success": true, "meta": {"changes": 1}}
            ])))
            .expect(1)
            .mount(&server)
            .await;
        let one = client
            .d1_query(
                "a1",
                "u1",
                &[D1Statement::new(
                    "SELECT * FROM t WHERE a = ?1",
                    vec![json!("x")],
                )],
            )
            .await
            .unwrap();
        assert_eq!(one[0].results[0]["n"], 1);
        assert_eq!(one[0].meta.rows_read, 1);
        let batch = client
            .d1_query(
                "a1",
                "u1",
                &[
                    D1Statement::new("INSERT INTO t VALUES (?1)", vec![json!(1)]),
                    D1Statement::new("DELETE FROM t", vec![]),
                ],
            )
            .await
            .unwrap();
        assert_eq!(batch[0].meta.last_row_id, 7);
        assert_eq!(batch.len(), 2);
    }

    #[tokio::test]
    async fn a_sql_error_is_an_api_error() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/d1/database/u1/query"))
            .respond_with(ResponseTemplate::new(400).set_body_json(json!({
                "success": false, "errors": [{"code": 7500, "message": "no such table: comments"}],
                "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        let err = client
            .d1_query(
                "a1",
                "u1",
                &[D1Statement::new("SELECT 1 FROM comments", vec![])],
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("no such table") || err.codes().contains(&7500));
    }
}
