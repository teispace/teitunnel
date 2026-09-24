//! Names for shell completion (`teitunnel __complete`): hostnames, tunnels, domains,
//! shares and accounts, read from the local database only.
//!
//! It opens the database read-only (no migrations, no writes) and never touches the
//! network or the keychain, so a Tab press stays well under 50 ms. Anything missing or
//! unreadable simply offers nothing.

use std::{collections::BTreeSet, path::Path};

use rusqlite::{Connection, OpenFlags};

/// What's worth completing, each list sorted and without duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Candidates {
    /// Hostnames of this machine's routes and shares on your domains.
    pub hostnames: Vec<String>,
    /// This machine's tunnels, by name.
    pub tunnels: Vec<String>,
    /// Domains (the parent of each known hostname).
    pub domains: Vec<String>,
    /// Accounts, by name.
    pub accounts: Vec<String>,
    /// Shares to stop: hostnames on your domains, and Quick Share ids.
    pub shares: Vec<String>,
}

/// Reads the candidates from `<data dir>/teitunnel.db` (empty when it doesn't exist).
pub fn candidates(data_dir: &Path) -> Candidates {
    let path = data_dir.join("teitunnel.db");
    if !path.exists() {
        return Candidates::default();
    }
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    match Connection::open_with_flags(&path, flags) {
        Ok(conn) => {
            // Never wait on the app's writes.
            let _ = conn.busy_timeout(std::time::Duration::from_millis(20));
            read(&conn)
        }
        Err(_) => Candidates::default(),
    }
}

fn column(conn: &Connection, sql: &str) -> Vec<String> {
    let Ok(mut statement) = conn.prepare(sql) else {
        return Vec::new();
    };
    statement
        .query_map([], |row| row.get::<_, Option<String>>(0))
        .map(|rows| rows.filter_map(Result::ok).flatten().collect())
        .unwrap_or_default()
}

/// Hostnames in the ingress rules Teitunnel last applied (JSON arrays of rules).
fn ingress_hostnames(conn: &Connection) -> Vec<String> {
    column(
        conn,
        "SELECT last_applied_ingress FROM local_tunnels WHERE last_applied_ingress IS NOT NULL",
    )
    .iter()
    .filter_map(|json| serde_json::from_str::<serde_json::Value>(json).ok())
    .filter_map(|rules| rules.as_array().cloned())
    .flatten()
    .filter_map(|rule| {
        rule.get("hostname")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    })
    .collect()
}

fn sorted(values: impl IntoIterator<Item = String>) -> Vec<String> {
    values
        .into_iter()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty() && !v.contains(char::is_whitespace))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn read(conn: &Connection) -> Candidates {
    let shares = column(conn, "SELECT hostname FROM domain_shares");
    let hostnames = sorted(
        ingress_hostnames(conn)
            .into_iter()
            .chain(column(conn, "SELECT hostname FROM dns_ownership"))
            .chain(column(conn, "SELECT hostname FROM balanced_routes"))
            .chain(shares.iter().cloned())
            .map(|h| h.to_ascii_lowercase())
            .filter(|h| !h.starts_with('*')),
    );
    let domains = sorted(hostnames.iter().filter_map(|h| {
        let (_, parent) = h.split_once('.')?;
        parent.contains('.').then(|| parent.to_owned())
    }));
    let quick = column(
        conn,
        "SELECT id FROM quick_shares WHERE stopped_at IS NULL ORDER BY started_at DESC LIMIT 20",
    );
    Candidates {
        tunnels: sorted(column(conn, "SELECT name FROM local_tunnels")),
        accounts: sorted(column(conn, "SELECT name FROM accounts")),
        shares: sorted(shares.into_iter().chain(quick)),
        hostnames,
        domains,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    #[tokio::test]
    async fn reads_names_from_a_seeded_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("teitunnel.db")).unwrap();
        store
            .call(|conn| {
                conn.execute_batch(
                    r#"
                    INSERT INTO accounts (id, name, credential, added_at)
                        VALUES ('a1', 'Personal', 'token', 1), ('a2', 'Work Team', 'token', 1);
                    INSERT INTO local_tunnels (tunnel_id, account_id, name, is_default,
                                               last_applied_ingress, created_at)
                        VALUES ('t1', 'a1', 'mac-mini', 1,
                                '[{"hostname":"app.example.com","service":"http://localhost:3000"},
                                  {"hostname":"*.wild.example.com","service":"http://localhost:4000"},
                                  {"service":"http_status:404"}]', 1),
                               ('t2', 'a1', 'staging', 0, NULL, 1);
                    INSERT INTO dns_ownership (record_id, account_id, zone_id, hostname, route_id, created_at)
                        VALUES ('r1', 'a1', 'z1', 'API.example.com', 'x', 1);
                    INSERT INTO domain_shares (account_id, hostname, origin, owner, created_at)
                        VALUES ('a1', 'demo.other.dev', '3000', 'app', 1);
                    INSERT INTO quick_shares (id, origin, started_at, stopped_at)
                        VALUES ('qs-live', 'http://localhost:3000', 2, NULL),
                               ('qs-old', 'http://localhost:3000', 1, 5);
                    "#,
                )?;
                Ok(())
            })
            .await
            .unwrap();
        drop(store);

        let started = std::time::Instant::now();
        let found = candidates(dir.path());
        assert!(started.elapsed() < std::time::Duration::from_millis(50));
        assert_eq!(
            found.hostnames,
            ["api.example.com", "app.example.com", "demo.other.dev"]
        );
        assert_eq!(found.domains, ["example.com", "other.dev"]);
        assert_eq!(found.tunnels, ["mac-mini", "staging"]);
        assert_eq!(
            found.accounts,
            ["Personal"],
            "names with spaces can't be completed"
        );
        assert_eq!(found.shares, ["demo.other.dev", "qs-live"]);
    }

    #[test]
    fn nothing_without_a_database() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(candidates(dir.path()), Candidates::default());
    }
}
