//! What Teitunnel remembers about edge protection (migration 15): the ownership index
//! of the edge rules it created (a backup for their description marker) and of the
//! Access service tokens it created. A token's secret is never stored: only its id,
//! client id, name and expiry.

use rusqlite::params;

use super::local::Local;
use crate::store::StoreError;

/// One of Teitunnel's edge rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRuleRow {
    /// Rule id.
    pub rule_id: String,
    /// Zone id.
    pub zone_id: String,
    /// Its phase.
    pub phase: String,
    /// The hostname (`None` for a shared rate limit).
    pub hostname: Option<String>,
    /// Which rule (`block`, `challenge`, `rateLimit`, …).
    pub kind: String,
}

/// One of Teitunnel's service tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceTokenRow {
    /// Token id.
    pub token_id: String,
    /// The hostname it was made for.
    pub hostname: String,
    /// Its name.
    pub name: String,
    /// The `CF-Access-Client-Id` value (not a secret).
    pub client_id: String,
    /// When it stops working (RFC 3339).
    pub expires_at: Option<String>,
    /// Created (ms since the epoch).
    pub created_at: u64,
}

fn now_ms() -> i64 {
    i64::try_from(crate::domain_shares::now_ms()).unwrap_or(i64::MAX)
}

impl Local {
    /// The edge rules Teitunnel created in `account`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn owned_edge_rules(&self, account: &str) -> Result<Vec<EdgeRuleRow>, StoreError> {
        let account = account.to_owned();
        self.store()
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT rule_id, zone_id, phase, hostname, kind FROM edge_rules
                     WHERE account_id = ?1 ORDER BY zone_id, phase, rule_id",
                )?;
                let rows = stmt
                    .query_map(params![account], |row| {
                        Ok(EdgeRuleRow {
                            rule_id: row.get(0)?,
                            zone_id: row.get(1)?,
                            phase: row.get(2)?,
                            hostname: row.get(3)?,
                            kind: row.get(4)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    /// Records that Teitunnel created an edge rule.
    ///
    /// # Errors
    /// Database errors.
    pub async fn own_edge_rule(&self, account: &str, row: EdgeRuleRow) -> Result<(), StoreError> {
        let account = account.to_owned();
        self.store()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO edge_rules (rule_id, account_id, zone_id, phase, hostname, kind, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (rule_id) DO UPDATE SET hostname = excluded.hostname, kind = excluded.kind",
                    params![
                        row.rule_id,
                        account,
                        row.zone_id,
                        row.phase,
                        row.hostname,
                        row.kind,
                        now_ms()
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets an edge rule (deleted).
    ///
    /// # Errors
    /// Database errors.
    pub async fn disown_edge_rule(&self, rule_id: &str) -> Result<(), StoreError> {
        let rule_id = rule_id.to_owned();
        self.store()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM edge_rules WHERE rule_id = ?1",
                    params![rule_id],
                )?;
                Ok(())
            })
            .await
    }

    /// The service tokens Teitunnel created in `account`, by hostname then name.
    ///
    /// # Errors
    /// Database errors.
    pub async fn owned_service_tokens(
        &self,
        account: &str,
    ) -> Result<Vec<ServiceTokenRow>, StoreError> {
        let account = account.to_owned();
        self.store()
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT token_id, hostname, name, client_id, expires_at, created_at
                     FROM service_tokens WHERE account_id = ?1 ORDER BY hostname, name",
                )?;
                let rows = stmt
                    .query_map(params![account], |row| {
                        Ok(ServiceTokenRow {
                            token_id: row.get(0)?,
                            hostname: row.get(1)?,
                            name: row.get(2)?,
                            client_id: row.get(3)?,
                            expires_at: row.get(4)?,
                            created_at: u64::try_from(row.get::<_, i64>(5)?).unwrap_or_default(),
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    /// Records a service token Teitunnel created (never its secret).
    ///
    /// # Errors
    /// Database errors.
    pub async fn own_service_token(
        &self,
        account: &str,
        row: ServiceTokenRow,
    ) -> Result<(), StoreError> {
        let account = account.to_owned();
        self.store()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO service_tokens (token_id, account_id, hostname, name, client_id, expires_at, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT (token_id) DO UPDATE SET expires_at = excluded.expires_at",
                    params![
                        row.token_id,
                        account,
                        row.hostname,
                        row.name,
                        row.client_id,
                        row.expires_at,
                        now_ms()
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets a service token (deleted).
    ///
    /// # Errors
    /// Database errors.
    pub async fn disown_service_token(&self, token_id: &str) -> Result<(), StoreError> {
        let token_id = token_id.to_owned();
        self.store()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM service_tokens WHERE token_id = ?1",
                    params![token_id],
                )?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[tokio::test]
    async fn remembers_rules_and_tokens_per_account() {
        let local = Local::new(Store::open_in_memory().unwrap());
        let rule = EdgeRuleRow {
            rule_id: "r1".into(),
            zone_id: "z1".into(),
            phase: cf_api::PHASE_CUSTOM.into(),
            hostname: Some("app.xyz.com".into()),
            kind: "block".into(),
        };
        local.own_edge_rule("a", rule.clone()).await.unwrap();
        assert_eq!(local.owned_edge_rules("a").await.unwrap(), [rule]);
        assert!(local.owned_edge_rules("b").await.unwrap().is_empty());
        local.disown_edge_rule("r1").await.unwrap();
        assert!(local.owned_edge_rules("a").await.unwrap().is_empty());

        let token = ServiceTokenRow {
            token_id: "t1".into(),
            hostname: "app.xyz.com".into(),
            name: "Teitunnel · app.xyz.com · CI".into(),
            client_id: "abc.access".into(),
            expires_at: None,
            created_at: 0,
        };
        local.own_service_token("a", token).await.unwrap();
        let found = local.owned_service_tokens("a").await.unwrap();
        assert_eq!(found[0].client_id, "abc.access");
        local.disown_service_token("t1").await.unwrap();
        assert!(local.owned_service_tokens("a").await.unwrap().is_empty());
    }
}
