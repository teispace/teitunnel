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
    // 3: Cloudflare accounts (metadata only; credentials live in the keychain)
    M::up(
        "CREATE TABLE accounts (
            id           TEXT PRIMARY KEY NOT NULL,
            name         TEXT NOT NULL,
            credential   TEXT NOT NULL,
            limited_zone TEXT,
            added_at     INTEGER NOT NULL
        ) STRICT;",
    ),
    // 4: routes engine: this Mac's tunnel per account, DNS ownership index, activity log
    M::up(
        "CREATE TABLE tunnels_local (
            account_id           TEXT PRIMARY KEY NOT NULL,
            tunnel_id            TEXT NOT NULL,
            name                 TEXT NOT NULL,
            last_applied_version INTEGER,
            last_applied_ingress TEXT,
            metrics_port         INTEGER,
            run_mode             TEXT NOT NULL DEFAULT 'session',
            created_at           INTEGER NOT NULL
        ) STRICT;
        CREATE TABLE dns_ownership (
            record_id  TEXT PRIMARY KEY NOT NULL,
            account_id TEXT NOT NULL,
            zone_id    TEXT NOT NULL,
            hostname   TEXT NOT NULL,
            route_id   TEXT NOT NULL,
            created_at INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX dns_ownership_account ON dns_ownership (account_id);
        CREATE TABLE activity (
            id         INTEGER PRIMARY KEY,
            account_id TEXT NOT NULL,
            at         INTEGER NOT NULL,
            summary    TEXT NOT NULL,
            outcome    TEXT NOT NULL,
            detail     TEXT NOT NULL
        ) STRICT;
        CREATE INDEX activity_account_at ON activity (account_id, at DESC);",
    ),
    // 5: per-minute traffic of this Mac's connectors, kept 7 days
    M::up(
        "CREATE TABLE metrics_rollup (
            tunnel_id       TEXT NOT NULL,
            minute          INTEGER NOT NULL,
            requests        INTEGER NOT NULL,
            errors          INTEGER NOT NULL,
            status_2xx      INTEGER NOT NULL,
            status_3xx      INTEGER NOT NULL,
            status_4xx      INTEGER NOT NULL,
            status_5xx      INTEGER NOT NULL,
            concurrent_max  INTEGER NOT NULL,
            connections_min INTEGER NOT NULL,
            rtt_sum_ms      REAL NOT NULL,
            rtt_samples     INTEGER NOT NULL,
            PRIMARY KEY (tunnel_id, minute)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX metrics_rollup_minute ON metrics_rollup (minute);",
    ),
    // 6: structured activity (kind, hostnames, step states, before/after), as JSON
    M::up("ALTER TABLE activity ADD COLUMN record TEXT;"),
    // 7: Access applications Teitunnel created (it changes only these)
    M::up(
        "CREATE TABLE access_ownership (
            app_id     TEXT PRIMARY KEY NOT NULL,
            account_id TEXT NOT NULL,
            domain     TEXT NOT NULL,
            created_at INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX access_ownership_account ON access_ownership (account_id);",
    ),
    // 8: several tunnels per machine: one row per tunnel, the machine tunnel is the default
    M::up(
        "CREATE TABLE local_tunnels (
            tunnel_id            TEXT PRIMARY KEY NOT NULL,
            account_id           TEXT NOT NULL,
            name                 TEXT NOT NULL,
            is_default           INTEGER NOT NULL,
            last_applied_version INTEGER,
            last_applied_ingress TEXT,
            metrics_port         INTEGER,
            run_mode             TEXT NOT NULL DEFAULT 'session',
            created_at           INTEGER NOT NULL
        ) STRICT;
        CREATE INDEX local_tunnels_account ON local_tunnels (account_id);
        CREATE UNIQUE INDEX local_tunnels_default ON local_tunnels (account_id) WHERE is_default = 1;
        INSERT INTO local_tunnels (tunnel_id, account_id, name, is_default, last_applied_version,
                                   last_applied_ingress, metrics_port, run_mode, created_at)
            SELECT tunnel_id, account_id, name, 1, last_applied_version, last_applied_ingress,
                   metrics_port, run_mode, created_at FROM tunnels_local;
        DROP TABLE tunnels_local;",
    ),
    // 9: temporary routes ("share on your domain"), removed when they stop or expire
    M::up(
        "CREATE TABLE domain_shares (
            account_id TEXT NOT NULL,
            hostname   TEXT NOT NULL,
            origin     TEXT NOT NULL,
            owner      TEXT NOT NULL,
            expires_at INTEGER,
            created_at INTEGER NOT NULL,
            PRIMARY KEY (account_id, hostname)
        ) STRICT;",
    ),
    // 10: sign-in for the web dashboard on servers (hashes only, never the secret)
    M::up(
        "CREATE TABLE web_credentials (
            id         INTEGER PRIMARY KEY,
            kind       TEXT NOT NULL,
            name       TEXT NOT NULL,
            hash       TEXT NOT NULL,
            created_at INTEGER NOT NULL
        ) STRICT;
        CREATE UNIQUE INDEX web_credentials_password ON web_credentials (kind) WHERE kind = 'password';",
    ),
    // 11: routes this machine load balances (for the routes view)
    M::up(
        "CREATE TABLE balanced_routes (
            account_id TEXT NOT NULL,
            hostname   TEXT NOT NULL,
            PRIMARY KEY (account_id, hostname)
        ) STRICT;",
    ),
    // 12: uptime checks through the edge (raw for 2 days, hourly for 30) and incidents
    M::up(
        "CREATE TABLE uptime_checks (
            route      TEXT NOT NULL,
            at         INTEGER NOT NULL,
            ok         INTEGER NOT NULL,
            status     INTEGER,
            latency_ms INTEGER,
            cause      TEXT,
            PRIMARY KEY (route, at)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX uptime_checks_at ON uptime_checks (at);
        CREATE TABLE uptime_hourly (
            route           TEXT NOT NULL,
            hour            INTEGER NOT NULL,
            checks          INTEGER NOT NULL,
            up              INTEGER NOT NULL,
            latency_sum_ms  INTEGER NOT NULL,
            latency_samples INTEGER NOT NULL,
            PRIMARY KEY (route, hour)
        ) STRICT, WITHOUT ROWID;
        CREATE INDEX uptime_hourly_hour ON uptime_hourly (hour);
        CREATE TABLE incidents (
            id         INTEGER PRIMARY KEY,
            route      TEXT NOT NULL,
            account_id TEXT NOT NULL,
            hostname   TEXT NOT NULL,
            path       TEXT,
            started_at INTEGER NOT NULL,
            ended_at   INTEGER,
            cause      TEXT NOT NULL
        ) STRICT;
        CREATE INDEX incidents_route ON incidents (route, started_at DESC);
        CREATE UNIQUE INDEX incidents_open ON incidents (route) WHERE ended_at IS NULL;",
    ),
    // 13: Snapshots (static copies hosted as Workers on the user's account) and their
    // recent versions' manifests (for change counts, rollback and settings-only updates)
    M::up(
        "CREATE TABLE snapshots (
            id           TEXT PRIMARY KEY,
            account_id   TEXT NOT NULL,
            name         TEXT NOT NULL,
            script       TEXT NOT NULL,
            hostname     TEXT,
            source       TEXT NOT NULL,
            spa          INTEGER NOT NULL DEFAULT 0,
            password     INTEGER NOT NULL DEFAULT 0,
            access       TEXT,
            expires_at   INTEGER,
            owner        TEXT NOT NULL,
            live_version TEXT,
            created_at   INTEGER NOT NULL,
            updated_at   INTEGER NOT NULL,
            UNIQUE (account_id, name),
            UNIQUE (account_id, script)
        ) STRICT;
        CREATE TABLE snapshot_versions (
            snapshot_id TEXT NOT NULL REFERENCES snapshots (id) ON DELETE CASCADE,
            number      INTEGER NOT NULL,
            version_id  TEXT NOT NULL,
            created_at  INTEGER NOT NULL,
            files       INTEGER NOT NULL,
            bytes       INTEGER NOT NULL,
            manifest    TEXT NOT NULL,
            headers     TEXT,
            redirects   TEXT,
            spa         INTEGER NOT NULL,
            password    INTEGER NOT NULL,
            PRIMARY KEY (snapshot_id, number)
        ) STRICT;",
    ),
    // 14: a cache of the account's reservations (the truth is in DNS comments, M12-11)
    M::up(
        "CREATE TABLE IF NOT EXISTS reservations_cache (
            account_id TEXT NOT NULL,
            hostname   TEXT NOT NULL,
            owner      TEXT,
            until      INTEGER,
            routed     INTEGER NOT NULL DEFAULT 0,
            mine       INTEGER NOT NULL DEFAULT 0,
            seen_at    INTEGER NOT NULL,
            PRIMARY KEY (account_id, hostname)
        ) STRICT;",
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
    fn moves_the_machine_tunnel_to_local_tunnels() {
        let mut conn = Connection::open_in_memory().unwrap();
        Migrations::from_slice(MIGRATIONS)
            .to_version(&mut conn, 7)
            .unwrap();
        conn.execute(
            "INSERT INTO tunnels_local (account_id, tunnel_id, name, metrics_port, run_mode, created_at)
             VALUES ('a', 't1', 'Mac', 20300, 'alwaysOn', 1)",
            [],
        )
        .unwrap();
        apply(&mut conn).unwrap();
        let row: (String, String, i64, i64, String) = conn
            .query_row(
                "SELECT tunnel_id, account_id, is_default, metrics_port, run_mode FROM local_tunnels",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(row, ("t1".into(), "a".into(), 1, 20300, "alwaysOn".into()));
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
