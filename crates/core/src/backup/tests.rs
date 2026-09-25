use super::*;
use crate::{
    Secret,
    engine::Local,
    secrets::{MemoryStore, SecretStore as _},
    store::Store,
};

/// Cheap parameters, so tests stay fast (the format stores them).
const FAST: KdfParams = KdfParams {
    memory_kib: 64,
    passes: 1,
    lanes: 1,
};

const API_TOKEN: &str = "cf-api-token-0123456789-SECRET-VALUE-abcdef";
const RUN_TOKEN: &str = "eyJhIjoiYWNjIiwidCI6InR1bm5lbCIsInMiOiJydW4tdG9rZW4tc2VjcmV0In0=";
const WEBHOOK_SECRET: &str = "whsec_super_secret_webhook_value";
const PASSWORD_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c2FsdHNhbHQ$aGFzaGhhc2hoYXNo";
const CA_KEY: &str = "-----BEGIN PRIVATE KEY-----\nMIGHAgEAMBMGByqGSM49AgEGCCqGSM49AwEHBG0wawIBAQQg\n-----END PRIVATE KEY-----\n";

fn pass(text: &str) -> Secret<String> {
    Secret::new(text.to_owned())
}

/// A database like a real one: an account (its token in the keychain), a tunnel (its run
/// token in the keychain), routes' ownership, settings, a project, a dashboard password.
async fn populated() -> (Store, MemoryStore) {
    let store = Store::open_in_memory().unwrap();
    let keychain = MemoryStore::default();
    keychain.set("account:acc:token", &pass(API_TOKEN)).unwrap();
    keychain.set("tunnel:t1", &pass(RUN_TOKEN)).unwrap();
    store
        .call(|conn| {
            conn.execute_batch(&format!(
                "INSERT INTO accounts (id, name, credential, limited_zone, added_at)
                     VALUES ('acc', 'Acme', 'apiToken', NULL, 1);
                 INSERT INTO local_tunnels (tunnel_id, account_id, name, is_default, metrics_port, run_mode, created_at)
                     VALUES ('t1', 'acc', 'Mac', 1, 20301, 'alwaysOn', 1);
                 INSERT INTO dns_ownership (record_id, account_id, zone_id, hostname, route_id, created_at)
                     VALUES ('r1', 'acc', 'z', 'shop.example.com', 'abc', 1);
                 INSERT INTO access_ownership (app_id, account_id, domain, created_at)
                     VALUES ('app1', 'acc', 'shop.example.com', 1);
                 INSERT INTO settings (key, value) VALUES ('theme', '\"dark\"');
                 INSERT INTO settings (key, value) VALUES ('alertRules', '{{\"routeDown\":false}}');
                 INSERT INTO settings (key, value) VALUES ('uptimeRunner', '{{\"owner\":\"app\"}}');
                 INSERT INTO settings (key, value) VALUES ('webhookSecret', '\"{WEBHOOK_SECRET}\"');
                 INSERT INTO web_credentials (kind, name, hash, created_at)
                     VALUES ('password', 'dashboard', '{PASSWORD_HASH}', 1);"
            ))?;
            Ok(())
        })
        .await
        .unwrap();
    crate::project::registry::remember(&store, "/w/shop/teitunnel.yml", "shop")
        .await
        .unwrap();
    crate::local_domains::registry::save(
        &store,
        &crate::local_domains::LocalDomainRow {
            name: localdomains::LocalName::parse_any("shop.test").unwrap(),
            target: localdomains::DomainTarget::Port { port: 3000 },
            wildcard: true,
            https: true,
            inspect: false,
            project: None,
            created_at: 1,
        },
    )
    .await
    .unwrap();
    // The local CA's key sits in the keychain, like every secret.
    keychain
        .set(localdomains::CA_KEYCHAIN_ACCOUNT, &pass(CA_KEY))
        .unwrap();
    (store, keychain)
}

#[tokio::test]
async fn round_trips_to_another_computer() {
    let (store, _keychain) = populated().await;
    let contents = collect(&store, "Ada's Mac").await.unwrap();
    let sealed = seal(&contents, &pass("correct horse battery"), FAST).unwrap();
    assert!(sealed.starts_with(MAGIC));

    let opened = open(&sealed, &pass("correct horse battery")).unwrap();
    assert_eq!(opened, contents);

    let fresh = Store::open_in_memory().unwrap();
    let summary = summarize(&fresh, &opened).await.unwrap();
    assert_eq!(summary.machine, "Ada's Mac");
    assert_eq!(summary.accounts[0].name, "Acme");
    assert_eq!(summary.projects, ["shop"]);
    assert!(!summary.overwrites, "nothing here yet");
    restore(&fresh, opened).await.unwrap();

    let local = Local::new(fresh.clone());
    let tunnels = local.tunnels("acc").await.unwrap();
    assert_eq!(tunnels.len(), 1);
    assert_eq!(tunnels[0].name, "Mac");
    assert_eq!(tunnels[0].metrics_port, None, "a port of the old computer");
    assert!(
        !tunnels[0].always_on,
        "the service stays on the old computer"
    );
    let settings = crate::settings::load(&fresh).await.unwrap();
    assert_eq!(settings.theme, crate::settings::Theme::Dark);
    let projects = crate::project::registry::list(&fresh).await.unwrap();
    assert_eq!(projects[0].name, "shop");
    let domains = crate::local_domains::registry::list(&fresh).await.unwrap();
    assert_eq!(domains.len(), 1);
    assert_eq!(domains[0].name.as_str(), "shop.test");
    assert!(domains[0].wildcard);
    assert!(
        summary
            .sections
            .iter()
            .any(|s| s.section == "local_domains" && s.count == 1)
    );
    // Accounts are connected again by the user; nothing about them is created.
    let accounts: i64 = fresh
        .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(accounts, 0);

    // Restoring again over the same data replaces it (no duplicates).
    let again = open(&sealed, &pass("correct horse battery")).unwrap();
    let summary = summarize(&fresh, &again).await.unwrap();
    assert!(summary.overwrites);
    restore(&fresh, again).await.unwrap();
    assert_eq!(local.tunnels("acc").await.unwrap().len(), 1);
}

#[tokio::test]
async fn never_holds_a_secret() {
    let (store, _keychain) = populated().await;
    let contents = collect(&store, "Mac").await.unwrap();
    let plaintext = serde_json::to_string(&contents).unwrap();
    for secret in [
        API_TOKEN,
        RUN_TOKEN,
        WEBHOOK_SECRET,
        PASSWORD_HASH,
        CA_KEY,
        "PRIVATE KEY",
        "webhookSecret",
        "uptimeRunner",
    ] {
        assert!(
            !plaintext.contains(secret),
            "{secret} leaked into the backup"
        );
    }
    assert!(!contents.tables.contains_key("web_credentials"));
    assert!(!contents.tables.contains_key("accounts"));
    // The sealed file doesn't show anything readable either.
    let sealed = seal(&contents, &pass("correct horse battery"), FAST).unwrap();
    let raw = String::from_utf8_lossy(&sealed);
    assert!(!raw.contains("Acme") && !raw.contains("shop.example.com"));
}

#[test]
fn a_wrong_passphrase_or_a_changed_file_is_refused() {
    let contents = Contents {
        created_at: 1,
        app_version: "0.2.0".into(),
        machine: "Mac".into(),
        schema: 1,
        accounts: Vec::new(),
        settings: BTreeMap::new(),
        tables: BTreeMap::new(),
    };
    let sealed = seal(&contents, &pass("correct horse battery"), FAST).unwrap();
    assert!(matches!(
        open(&sealed, &pass("correct horse battery!")),
        Err(BackupError::WrongPassphrase)
    ));
    // Any byte changed, in the header or the body, is detected.
    for at in [20, 35, 50, HEADER_LEN + 3, sealed.len() - 1] {
        let mut tampered = sealed.clone();
        tampered[at] ^= 0x01;
        assert!(
            matches!(
                open(&tampered, &pass("correct horse battery")),
                Err(BackupError::WrongPassphrase)
            ),
            "byte {at}"
        );
    }
    let mut truncated = sealed.clone();
    truncated.truncate(sealed.len() - 5);
    assert!(open(&truncated, &pass("correct horse battery")).is_err());
    assert!(matches!(
        open(b"not a backup at all, just text", &pass("x")),
        Err(BackupError::NotABackup)
    ));
    let mut future = sealed.clone();
    future[16] = 2;
    assert!(matches!(
        open(&future, &pass("correct horse battery")),
        Err(BackupError::UnsupportedFormat(2))
    ));
    // A crafted header can't ask for absurd amounts of memory.
    let mut greedy = sealed;
    greedy[18..22].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(
        open(&greedy, &pass("correct horse battery")),
        Err(BackupError::WrongPassphrase)
    ));
    assert!(matches!(
        seal(&contents, &pass("short"), FAST),
        Err(BackupError::WeakPassphrase)
    ));
}

#[tokio::test]
async fn refuses_a_backup_of_a_newer_database() {
    let store = Store::open_in_memory().unwrap();
    let mut contents = collect(&store, "Mac").await.unwrap();
    contents.schema += 1;
    assert!(matches!(
        restore(&store, contents).await,
        Err(BackupError::NewerSchema)
    ));
}

#[tokio::test]
async fn writes_a_private_file() {
    let (store, _keychain) = populated().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("setup.{EXTENSION}"));
    create_file(&store, "Mac", &path, pass("correct horse battery"), FAST)
        .await
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let contents = read_file(&path, pass("correct horse battery"))
        .await
        .unwrap();
    assert_eq!(contents.machine, "Mac");
    assert!(matches!(
        read_file(&path, pass("wrong passphrase!")).await,
        Err(BackupError::WrongPassphrase)
    ));
}

#[test]
fn every_section_has_a_name() {
    for section in std::iter::once("settings").chain(TABLES.iter().map(|(table, _)| *table)) {
        assert_ne!(
            section_label(section).key,
            "core.raw",
            "{section} needs a name in core.backup.section"
        );
    }
}
