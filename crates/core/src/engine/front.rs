//! Workers in front of a route, on the user's own account
//! (docs/research/cloudflare-workers-features.md):
//!
//! - the **offline page** (M12-06): a Worker route `hostname/*` whose script passes
//!   every request to the tunnel (`fetch(request)`) and, when the tunnel answers 530
//!   (error 1033: no connector, the computer is off), shows the person's own page
//!   instead of Cloudflare's error;
//! - the **webhook inbox** (M12-12): a Worker route `hostname/path*` that stores
//!   webhooks in the account's D1 database while the tunnel is down (or while earlier
//!   ones are still waiting, so order is kept) and answers `202`; Teitunnel delivers
//!   them in order when the computer is back (`crate::inbox`).
//!
//! Both are opt-in per route: every request to them runs a Worker, which counts
//! against the account's 100,000 free requests a day. Routes are created to fail open,
//! so past the limit the site keeps working without them. Teitunnel owns only the
//! scripts and routes it created (named `tt-…`, recorded in the local index); a route
//! someone else made on the same pattern is never touched.

use cf_api::WorkerModule;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::cloud::CloudApi;
use crate::{
    Secret,
    text::{Text, UserText, msg},
};

/// The offline page's Worker, reviewed with its tests in
/// `apps/desktop/src/test/front-workers.test.ts`.
pub const OFFLINE_WORKER_JS: &str = include_str!("offline-worker.js");
/// The webhook inbox's Worker (same tests).
pub const INBOX_WORKER_JS: &str = include_str!("inbox-worker.js");
/// Module name of both.
pub const MODULE: &str = "worker.js";
/// Every front Worker's name starts with this.
pub const SCRIPT_PREFIX: &str = "tt-";
const COMPATIBILITY_DATE: &str = "2026-09-01";

/// The inbox table, as both the app and the Worker create it (a test keeps them equal).
pub const INBOX_SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS teitunnel_inbox (seq INTEGER PRIMARY KEY AUTOINCREMENT, inbox TEXT NOT NULL, id TEXT NOT NULL UNIQUE, received_at INTEGER NOT NULL, method TEXT NOT NULL, path TEXT NOT NULL, headers TEXT NOT NULL, body TEXT, size INTEGER NOT NULL, delivered_at INTEGER, status INTEGER, attempts INTEGER NOT NULL DEFAULT 0, error TEXT)",
    "CREATE INDEX IF NOT EXISTS teitunnel_inbox_pending ON teitunnel_inbox (inbox, delivered_at, seq)",
];

/// Longest offline page title.
pub const MAX_TITLE: usize = 120;
/// Longest offline page message.
pub const MAX_MESSAGE: usize = 1000;
/// Most webhooks one inbox holds.
pub const MAX_INBOX_ITEMS: u32 = 1000;
/// Longest an inbox keeps a webhook, in days.
pub const MAX_RETENTION_DAYS: u32 = 30;

/// Which Worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum FrontKind {
    /// The offline page.
    Offline,
    /// The webhook inbox.
    Inbox,
}

impl FrontKind {
    /// Its name in the local index.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Offline => "offline",
            Self::Inbox => "inbox",
        }
    }

    /// From the local index.
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "offline" => Some(Self::Offline),
            "inbox" => Some(Self::Inbox),
            _ => None,
        }
    }
}

/// The page shown while this computer is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct OfflinePage {
    /// Heading, e.g. "Back soon".
    pub title: String,
    /// A line or two for visitors.
    pub message: String,
    /// Also show it when the tunnel is up but the local app doesn't answer (502/504).
    #[serde(default)]
    pub when_app_down: bool,
}

impl Default for OfflinePage {
    fn default() -> Self {
        Self {
            title: "Back soon".into(),
            message: "This site runs on a computer that's offline right now. Try again later."
                .into(),
            when_app_down: false,
        }
    }
}

/// Why front Worker settings were refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrontError {
    /// Empty or too long title.
    Title,
    /// Too long message.
    Message,
    /// The inbox path must start with `/` and have no wildcard.
    Path,
    /// Out of range numbers.
    Limits,
}

impl UserText for FrontError {
    fn text(&self) -> Text {
        use msg::front::error as m;
        match self {
            Self::Title => m::title(MAX_TITLE as u64),
            Self::Message => m::message(MAX_MESSAGE as u64),
            Self::Path => m::path(),
            Self::Limits => m::limits(u64::from(MAX_INBOX_ITEMS), u64::from(MAX_RETENTION_DAYS)),
        }
    }
}

crate::text::english_display!(FrontError);

fn plain(text: &str, max: usize, lines: bool) -> Option<String> {
    let text = text.trim();
    let ok = text.chars().count() <= max
        && !text
            .chars()
            .any(|c| c.is_control() && !(lines && c == '\n'));
    ok.then(|| text.to_owned())
}

impl OfflinePage {
    /// Trimmed and checked.
    ///
    /// # Errors
    /// [`FrontError::Title`] or [`FrontError::Message`].
    pub fn normalized(&self) -> Result<Self, FrontError> {
        let title = plain(&self.title, MAX_TITLE, false)
            .filter(|t| !t.is_empty())
            .ok_or(FrontError::Title)?;
        let message = plain(&self.message.replace("\r\n", "\n"), MAX_MESSAGE, true)
            .ok_or(FrontError::Message)?;
        Ok(Self {
            title,
            message,
            when_app_down: self.when_app_down,
        })
    }
}

/// How the webhook inbox verifies senders before keeping a webhook (optional).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum InboxVerify {
    /// GitHub's `X-Hub-Signature-256`.
    Github,
    /// Stripe's `Stripe-Signature`.
    Stripe,
    /// Standard Webhooks (`webhook-signature`: Svix, Clerk, Resend…).
    Standard,
}

impl InboxVerify {
    /// The inspector's webhook provider it checks signatures of.
    pub fn provider(self) -> lens::webhook::Provider {
        match self {
            Self::Github => lens::webhook::Provider::GitHub,
            Self::Stripe => lens::webhook::Provider::Stripe,
            Self::Standard => lens::webhook::Provider::StandardWebhooks,
        }
    }
}

/// The signing secret saved in the keychain for `verify`'s webhooks on `hostname`
/// (shared with the hostname's inspector); `None` when there's none or the keychain
/// refused.
pub(crate) async fn saved_inbox_secret(
    secrets: &crate::secrets::Secrets,
    hostname: &str,
    verify: InboxVerify,
) -> Option<Secret<String>> {
    crate::inspect::secrets::webhook_secret_text(
        secrets,
        &crate::inspect::secrets::host_scope(hostname),
        verify.provider(),
    )
    .await
    .ok()
    .flatten()
}

/// A webhook inbox's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InboxSettings {
    /// Most webhooks kept waiting (older ones stay; new ones get `503` when full).
    pub max_items: u32,
    /// Days a webhook is kept, delivered or not.
    pub retention_days: u32,
    /// Keep only webhooks with a valid signature (the secret is in the keychain and
    /// sent to Cloudflare as a Worker secret).
    #[serde(default)]
    pub verify: Option<InboxVerify>,
}

impl Default for InboxSettings {
    fn default() -> Self {
        Self {
            max_items: 500,
            retention_days: 7,
            verify: None,
        }
    }
}

impl InboxSettings {
    /// Checked.
    ///
    /// # Errors
    /// [`FrontError::Limits`].
    pub fn normalized(&self) -> Result<Self, FrontError> {
        if (1..=MAX_INBOX_ITEMS).contains(&self.max_items)
            && (1..=MAX_RETENTION_DAYS).contains(&self.retention_days)
        {
            Ok(self.clone())
        } else {
            Err(FrontError::Limits)
        }
    }
}

/// A webhook path: starts with `/`, no wildcard, query or control characters, at most
/// 200 bytes; the trailing `/` is kept.
///
/// # Errors
/// [`FrontError::Path`].
pub fn clean_inbox_path(path: &str) -> Result<String, FrontError> {
    let path = path.trim();
    if !path.starts_with('/')
        || path.len() > 200
        || path
            .chars()
            .any(|c| c.is_control() || matches!(c, '*' | '?' | '#' | ' ' | '\\'))
    {
        return Err(FrontError::Path);
    }
    Ok(path.to_owned())
}

/// What a front Worker is deployed with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FrontConfig {
    /// The offline page.
    Offline {
        /// The page.
        page: OfflinePage,
    },
    /// A webhook inbox.
    Inbox {
        /// The path it catches.
        path: String,
        /// Its settings.
        settings: InboxSettings,
    },
}

impl FrontConfig {
    /// Which Worker.
    pub fn kind(&self) -> FrontKind {
        match self {
            Self::Offline { .. } => FrontKind::Offline,
            Self::Inbox { .. } => FrontKind::Inbox,
        }
    }

    /// The inbox path (`""` for the offline page), as the local index keys it.
    pub fn path(&self) -> &str {
        match self {
            Self::Offline { .. } => "",
            Self::Inbox { path, .. } => path,
        }
    }
}

/// Which D1 database a Worker binds to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum DatabaseRef {
    /// One that exists.
    Existing(String),
    /// The one created earlier in the same plan.
    Created,
}

/// The account's D1 database for Teitunnel, as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DatabaseState {
    /// Its id, when there is one.
    pub id: Option<String>,
}

/// A Worker name for a hostname (and inbox path): `tt-offline-<10 hex>`.
pub fn script_for(kind: FrontKind, hostname: &str, path: &str) -> String {
    let mut hash = Sha256::new();
    hash.update(hostname.to_ascii_lowercase().as_bytes());
    hash.update([0]);
    hash.update(path.as_bytes());
    let digest: String = hash.finalize()[..5]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("{SCRIPT_PREFIX}{}-{digest}", kind.as_str())
}

/// The route pattern: `hostname/*` for the offline page, `hostname/path*` for an inbox.
pub fn pattern_for(hostname: &str, config: &FrontConfig) -> String {
    let hostname = hostname.to_ascii_lowercase();
    match config {
        FrontConfig::Offline { .. } => format!("{hostname}/*"),
        FrontConfig::Inbox { path, .. } => format!("{hostname}{path}*"),
    }
}

/// The Worker's metadata. `database` is the D1 id for an inbox; `secret` the signing
/// secret for a verifying inbox (`None` with verification on keeps the one it has).
pub fn metadata(
    script: &str,
    config: &FrontConfig,
    database: Option<&str>,
    secret: Option<&Secret<String>>,
) -> Value {
    let (bindings, message, keep) = match config {
        FrontConfig::Offline { page } => (
            vec![json!({
                "type": "plain_text", "name": "PAGE",
                "text": json!({
                    "title": page.title, "message": page.message, "whenAppDown": page.when_app_down,
                }).to_string(),
            })],
            "Teitunnel offline page",
            false,
        ),
        FrontConfig::Inbox { path, settings } => {
            let mut bindings = vec![json!({
                "type": "plain_text", "name": "INBOX",
                "text": json!({
                    "id": script, "path": path, "maxItems": settings.max_items,
                    "retentionDays": settings.retention_days, "verify": settings.verify,
                }).to_string(),
            })];
            if let Some(id) = database {
                bindings.push(json!({ "type": "d1", "name": "DB", "id": id }));
            }
            let mut keep = false;
            if settings.verify.is_some() {
                match secret {
                    Some(secret) => bindings.push(json!({
                        "type": "secret_text", "name": "SIGNING_SECRET", "text": secret.expose(),
                    })),
                    None => keep = true,
                }
            }
            (bindings, "Teitunnel webhook inbox", keep)
        }
    };
    let mut metadata = json!({
        "main_module": MODULE,
        "compatibility_date": COMPATIBILITY_DATE,
        "bindings": bindings,
        "annotations": { "workers/message": message },
    });
    if keep {
        metadata["keep_bindings"] = json!(["secret_text"]);
    }
    metadata
}

/// The Worker's code.
pub fn modules(kind: FrontKind) -> Vec<WorkerModule> {
    vec![WorkerModule {
        name: MODULE.to_owned(),
        content: match kind {
            FrontKind::Offline => OFFLINE_WORKER_JS,
            FrontKind::Inbox => INBOX_WORKER_JS,
        }
        .to_owned(),
    }]
}

/// One of Teitunnel's front Workers on the hostname, as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedFront {
    /// Its settings, as Teitunnel last deployed them.
    pub config: FrontConfig,
    /// The Worker.
    pub script: String,
    /// Whether the Worker exists.
    pub exists: bool,
    /// Its route, when there is one.
    pub route: Option<cf_api::WorkerRoute>,
}

/// Worker routes on a hostname, as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FrontState {
    /// The hostname.
    pub hostname: String,
    /// Its zone.
    pub zone_id: String,
    /// Teitunnel's front Workers there.
    pub fronts: Vec<ObservedFront>,
    /// Routes on the hostname Teitunnel didn't make (never touched).
    pub foreign: Vec<cf_api::WorkerRoute>,
}

impl FrontState {
    /// Teitunnel's Worker of `kind` (at `path` for an inbox).
    pub fn find(&self, kind: FrontKind, path: &str) -> Option<&ObservedFront> {
        self.fronts
            .iter()
            .find(|f| f.config.kind() == kind && f.config.path() == path)
    }

    /// A foreign route with exactly this pattern.
    pub fn foreign_on(&self, pattern: &str) -> Option<&cf_api::WorkerRoute> {
        self.foreign
            .iter()
            .find(|r| r.pattern.eq_ignore_ascii_case(pattern))
    }
}

/// What a front change needs observed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FrontNeed {
    /// The hostname.
    pub hostname: Option<String>,
    /// Read even when the local index has nothing for the hostname (a change to it);
    /// otherwise only when it does (removing a route cleans up after it).
    pub required: bool,
    /// The account's D1 database is needed.
    pub database: bool,
    /// Also every routed hostname the local index has Workers for (removing a
    /// tunnel cleans up after all its routes).
    pub routed: bool,
}

/// A local index row, as observation needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontRow {
    /// Hostname.
    pub hostname: String,
    /// Settings deployed.
    pub config: FrontConfig,
    /// The Worker.
    pub script: String,
    /// Zone id.
    pub zone_id: String,
    /// Route id.
    pub route_id: Option<String>,
}

/// Reads Worker routes on `hostname` (in `zone_id`) and whether Teitunnel's scripts
/// exist. `rows` are the local index's entries for the hostname.
///
/// # Errors
/// API errors.
pub(crate) async fn observe<C: CloudApi>(
    api: &C,
    account: &str,
    hostname: &str,
    zone_id: &str,
    rows: &[FrontRow],
) -> Result<FrontState, cf_api::Error> {
    let host = hostname.to_ascii_lowercase();
    let routes: Vec<cf_api::WorkerRoute> = api
        .worker_routes(zone_id)
        .await?
        .into_iter()
        .filter(|r| {
            r.pattern
                .trim_start_matches("https://")
                .trim_start_matches("http://")
                .split('/')
                .next()
                .is_some_and(|h| h.eq_ignore_ascii_case(&host))
        })
        .collect();
    let mut fronts = Vec::new();
    for row in rows {
        let exists = api
            .worker_deployments(account, &row.script)
            .await?
            .is_some();
        let route = routes
            .iter()
            .find(|r| {
                row.route_id.as_deref() == Some(r.id.as_str())
                    || (r.script.as_deref() == Some(row.script.as_str())
                        && r.pattern
                            .eq_ignore_ascii_case(&pattern_for(&host, &row.config)))
            })
            .cloned();
        fronts.push(ObservedFront {
            config: row.config.clone(),
            script: row.script.clone(),
            exists,
            route,
        });
    }
    let ours: Vec<&str> = fronts
        .iter()
        .filter_map(|f| f.route.as_ref().map(|r| r.id.as_str()))
        .collect();
    let mut foreign: Vec<cf_api::WorkerRoute> = routes
        .into_iter()
        .filter(|r| !ours.contains(&r.id.as_str()))
        .collect();
    foreign.sort_by(|a, b| a.pattern.cmp(&b.pattern));
    fronts.sort_by(|a, b| {
        (a.config.kind(), a.config.path()).cmp(&(b.config.kind(), b.config.path()))
    });
    Ok(FrontState {
        hostname: host,
        zone_id: zone_id.to_owned(),
        fronts,
        foreign,
    })
}

/// Reads the account's Teitunnel database: the one in the local index if it still
/// exists, else one named [`crate::comments::remote::DATABASE_NAME`].
///
/// # Errors
/// API errors.
pub(crate) async fn observe_database<C: CloudApi>(
    api: &C,
    account: &str,
    indexed: Option<&str>,
) -> Result<DatabaseState, cf_api::Error> {
    let found = api
        .d1_databases(account, crate::comments::remote::DATABASE_NAME)
        .await?;
    let id = indexed
        .and_then(|id| found.iter().find(|d| d.uuid == id))
        .or_else(|| found.first())
        .map(|d| d.uuid.clone());
    Ok(DatabaseState { id })
}

/// Creates the tables comments and inboxes use (safe to run again).
///
/// # Errors
/// API errors.
pub(crate) async fn create_tables<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
) -> Result<(), cf_api::Error> {
    let statements: Vec<cf_api::D1Statement> = crate::comments::remote::COMMENTS_SCHEMA
        .iter()
        .chain(INBOX_SCHEMA)
        .map(|sql| cf_api::D1Statement::new(*sql, Vec::new()))
        .collect();
    api.d1_query(account, database, &statements).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_patterns_are_stable() {
        let a = script_for(FrontKind::Offline, "App.Example.com", "");
        assert_eq!(a, script_for(FrontKind::Offline, "app.example.com", ""));
        assert!(a.starts_with("tt-offline-") && a.len() == "tt-offline-".len() + 10);
        assert_ne!(
            script_for(FrontKind::Inbox, "app.example.com", "/hooks/"),
            script_for(FrontKind::Inbox, "app.example.com", "/stripe/")
        );
        let page = FrontConfig::Offline {
            page: OfflinePage::default(),
        };
        assert_eq!(pattern_for("App.example.com", &page), "app.example.com/*");
        let inbox = FrontConfig::Inbox {
            path: "/webhooks/".into(),
            settings: InboxSettings::default(),
        };
        assert_eq!(
            pattern_for("app.example.com", &inbox),
            "app.example.com/webhooks/*"
        );
    }

    #[test]
    fn settings_are_checked() {
        let page = OfflinePage {
            title: "  Back soon ".into(),
            message: "Line one\r\nLine two".into(),
            when_app_down: true,
        };
        let page = page.normalized().unwrap();
        assert_eq!(page.title, "Back soon");
        assert_eq!(page.message, "Line one\nLine two");
        assert_eq!(
            OfflinePage {
                title: String::new(),
                ..OfflinePage::default()
            }
            .normalized(),
            Err(FrontError::Title)
        );
        assert_eq!(
            OfflinePage {
                message: "x".repeat(MAX_MESSAGE + 1),
                ..OfflinePage::default()
            }
            .normalized(),
            Err(FrontError::Message)
        );
        assert!(InboxSettings::default().normalized().is_ok());
        assert!(
            InboxSettings {
                max_items: 0,
                ..InboxSettings::default()
            }
            .normalized()
            .is_err()
        );
        assert_eq!(clean_inbox_path(" /hooks/ ").unwrap(), "/hooks/");
        for bad in ["hooks", "/a*", "/a?b", "/a b"] {
            assert!(clean_inbox_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn metadata_binds_what_each_worker_needs() {
        let offline = metadata(
            "tt-offline-1",
            &FrontConfig::Offline {
                page: OfflinePage::default(),
            },
            None,
            None,
        );
        assert_eq!(offline["bindings"][0]["name"], "PAGE");
        assert!(offline.get("assets").is_none());
        assert!(offline.get("keep_bindings").is_none());
        let page: Value =
            serde_json::from_str(offline["bindings"][0]["text"].as_str().unwrap()).unwrap();
        assert_eq!(page["title"], "Back soon");

        let config = FrontConfig::Inbox {
            path: "/hooks/".into(),
            settings: InboxSettings {
                verify: Some(InboxVerify::Github),
                ..InboxSettings::default()
            },
        };
        let secret = Secret::new("whsec".to_owned());
        let inbox = metadata("tt-inbox-1", &config, Some("db1"), Some(&secret));
        assert_eq!(inbox["bindings"][1]["type"], "d1");
        assert_eq!(inbox["bindings"][1]["id"], "db1");
        assert_eq!(inbox["bindings"][2]["type"], "secret_text");
        let kept = metadata("tt-inbox-1", &config, Some("db1"), None);
        assert_eq!(kept["keep_bindings"], json!(["secret_text"]));
        assert_eq!(kept["bindings"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn the_inbox_worker_creates_the_same_table() {
        for sql in INBOX_SCHEMA {
            assert!(
                INBOX_WORKER_JS.contains(sql),
                "the inbox Worker lacks: {sql}"
            );
        }
    }
}
