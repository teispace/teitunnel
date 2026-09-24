//! The local domains in the database (`local_domains`, migration 17) and their setting.

use localdomains::{DomainTarget, LocalName};
use rusqlite::{OptionalExtension, params};

use super::model::{LocalDomainRow, LocalDomainSettings};
use crate::store::{Store, StoreError};

/// The settings key.
const SETTINGS_KEY: &str = "localDomains";

fn row_of(
    name: &str,
    target: &str,
    wildcard: bool,
    https: bool,
    inspect: bool,
    project: Option<String>,
    created_at: i64,
) -> Option<LocalDomainRow> {
    let parsed_name = LocalName::parse_any(name);
    let parsed_target = serde_json::from_str::<DomainTarget>(target);
    match (parsed_name, parsed_target) {
        (Ok(name), Ok(target)) => Some(LocalDomainRow {
            name,
            target,
            wildcard,
            https,
            inspect,
            project,
            created_at,
        }),
        _ => {
            tracing::warn!(name, "skipping a local domain that can't be read");
            None
        }
    }
}

/// Every local domain, by name. Rows that can't be read (from a newer version) are
/// skipped.
///
/// # Errors
/// The database failed.
pub async fn list(store: &Store) -> Result<Vec<LocalDomainRow>, StoreError> {
    store
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT name, target, wildcard, https, inspect, project, created_at
                 FROM local_domains ORDER BY name",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(row_of(
                    &row.get::<_, String>(0)?,
                    &row.get::<_, String>(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                ))
            })?;
            Ok(rows
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .flatten()
                .collect())
        })
        .await
}

/// One local domain.
///
/// # Errors
/// The database failed.
pub async fn get(store: &Store, name: &LocalName) -> Result<Option<LocalDomainRow>, StoreError> {
    let name = name.as_str().to_owned();
    store
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT name, target, wildcard, https, inspect, project, created_at
                     FROM local_domains WHERE name = ?1",
                    params![name],
                    |row| {
                        Ok(row_of(
                            &row.get::<_, String>(0)?,
                            &row.get::<_, String>(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                        ))
                    },
                )
                .optional()?
                .flatten())
        })
        .await
}

/// Adds or replaces a local domain.
///
/// # Errors
/// The database failed.
pub async fn save(store: &Store, row: &LocalDomainRow) -> Result<(), StoreError> {
    let target = serde_json::to_string(&row.target).unwrap_or_default();
    let (name, wildcard, https, inspect, project, created_at) = (
        row.name.as_str().to_owned(),
        row.wildcard,
        row.https,
        row.inspect,
        row.project.clone(),
        row.created_at,
    );
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO local_domains (name, target, wildcard, https, inspect, project, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT (name) DO UPDATE SET target = ?2, wildcard = ?3, https = ?4,
                   inspect = ?5, project = ?6",
                params![name, target, wildcard, https, inspect, project, created_at],
            )?;
            Ok(())
        })
        .await
}

/// Removes a local domain; returns whether it existed.
///
/// # Errors
/// The database failed.
pub async fn remove(store: &Store, name: &LocalName) -> Result<bool, StoreError> {
    let name = name.as_str().to_owned();
    store
        .call(move |conn| {
            Ok(conn.execute("DELETE FROM local_domains WHERE name = ?1", params![name])? > 0)
        })
        .await
}

/// The local domains setting (defaults when unset or unreadable).
///
/// # Errors
/// The database failed.
pub async fn settings(store: &Store) -> Result<LocalDomainSettings, StoreError> {
    store
        .call(|conn| {
            let value: Option<String> = conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![SETTINGS_KEY],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(value
                .and_then(|v| serde_json::from_str(&v).ok())
                .unwrap_or_default())
        })
        .await
}

/// Saves the local domains setting.
///
/// # Errors
/// The database failed.
pub async fn save_settings(store: &Store, settings: LocalDomainSettings) -> Result<(), StoreError> {
    let value = serde_json::to_string(&settings).unwrap_or_default();
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = ?2",
                params![SETTINGS_KEY, value],
            )?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, port: u16) -> LocalDomainRow {
        LocalDomainRow {
            name: LocalName::parse_any(name).unwrap(),
            target: DomainTarget::Port { port },
            wildcard: false,
            https: true,
            inspect: false,
            project: None,
            created_at: 1_790_000_000,
        }
    }

    #[tokio::test]
    async fn saves_lists_updates_and_removes() {
        let store = Store::open_in_memory().unwrap();
        assert!(list(&store).await.unwrap().is_empty());
        save(&store, &row("b.test", 3000)).await.unwrap();
        let mut a = row("a.localhost", 4000);
        a.wildcard = true;
        a.project = Some("/src/app/teitunnel.yml".into());
        save(&store, &a).await.unwrap();
        let all = list(&store).await.unwrap();
        assert_eq!(all, vec![a.clone(), row("b.test", 3000)]);

        let mut changed = row("b.test", 3001);
        changed.inspect = true;
        changed.created_at = 1;
        save(&store, &changed).await.unwrap();
        let got = get(&store, &changed.name).await.unwrap().unwrap();
        assert_eq!(got.target, DomainTarget::Port { port: 3001 });
        assert!(got.inspect);
        assert_eq!(got.created_at, 1_790_000_000, "created_at is kept");

        assert!(remove(&store, &a.name).await.unwrap());
        assert!(!remove(&store, &a.name).await.unwrap());
        assert_eq!(list(&store).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn unreadable_rows_are_skipped_and_settings_default() {
        let store = Store::open_in_memory().unwrap();
        store
            .call(|conn| {
                conn.execute(
                    "INSERT INTO local_domains (name, target, created_at) VALUES ('x.test', '{\"kind\":\"future\"}', 1)",
                    [],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(list(&store).await.unwrap().is_empty());
        assert_eq!(
            settings(&store).await.unwrap(),
            LocalDomainSettings::default()
        );
        save_settings(&store, LocalDomainSettings { lan: true })
            .await
            .unwrap();
        assert!(settings(&store).await.unwrap().lan);
    }
}
