//! Forward-only schema migrations. Append new ones; never edit a released migration.
//! Later milestones add tables alongside their features (ARCHITECTURE §9).

use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

const MIGRATIONS: &[M<'static>] = &[
    // 1: settings (key → JSON value)
    M::up(
        "CREATE TABLE settings (
            key   TEXT PRIMARY KEY NOT NULL,
            value TEXT NOT NULL
        ) STRICT;",
    ),
    // 2: Quick Share history
    M::up(
        "CREATE TABLE quick_shares (
            id         TEXT PRIMARY KEY NOT NULL,
            origin     TEXT NOT NULL,
            url        TEXT,
            started_at INTEGER NOT NULL,
            stopped_at INTEGER
        ) STRICT;
        CREATE INDEX quick_shares_started ON quick_shares (started_at DESC);",
    ),
];

pub(super) fn apply(conn: &mut Connection) -> Result<(), rusqlite_migration::Error> {
    Migrations::from_slice(MIGRATIONS).to_latest(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_valid() {
        assert!(Migrations::from_slice(MIGRATIONS).validate().is_ok());
    }

    #[test]
    fn applying_twice_is_a_no_op() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(&mut conn).unwrap();
        apply(&mut conn).unwrap();
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, i64::try_from(MIGRATIONS.len()).unwrap());
    }
}
