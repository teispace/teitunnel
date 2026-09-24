//! Snapshots in the engine: a static copy of a site hosted on the account's own
//! Cloudflare, as a Worker with static assets (docs/research/cloudflare-snapshots.md).
//!
//! The types here are what snapshot intents and steps carry, what the observer reads
//! about a snapshot's Worker, and the pieces the executor needs: uploading only the
//! files Cloudflare doesn't have, and the Worker's metadata. Publishing is atomic: a
//! version only goes live once every file is uploaded.

use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use cf_api::{AssetEntry, AssetFile, WorkerDomain, WorkerModule};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{access::AccessRule, cloud::CloudApi};
use crate::{
    Secret,
    domain::Hostname,
    text::{Text, UserText, msg},
};

/// Most files one version may have (Workers Free plan).
pub const MAX_FILES: usize = 20_000;
/// Largest file Cloudflare accepts (25 MiB).
pub const MAX_FILE_SIZE: u64 = 25 * 1024 * 1024;
/// The Worker's module name.
pub const WORKER_MODULE: &str = "worker.js";
/// The Worker every Snapshot runs: serves the files, and checks the password first when
/// one is set. Reviewed with its tests in `apps/desktop/src/test/snapshot-worker.test.ts`.
pub const WORKER_JS: &str = include_str!("snapshot-worker.js");
/// Compatibility date the Worker is written for (navigation requests prefer assets from
/// 2025-04-01).
const COMPATIBILITY_DATE: &str = "2026-09-01";
/// Files per upload request (Cloudflare groups them into buckets; this caps a bucket
/// that's unexpectedly large).
const BUCKET_FILES: usize = 50;

/// The hash a file is stored under: the first 32 hex characters of
/// `sha256(base64(content) + extension)`, the recipe of Cloudflare's example.
pub fn content_hash(content: &[u8], path: &str) -> String {
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let mut hash = Sha256::new();
    hash.update(STANDARD.encode(content));
    hash.update(extension);
    hash.finalize()[..16]
        .iter()
        .fold(String::with_capacity(32), |mut out, b| {
            use std::fmt::Write;
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// A size for people: `980 bytes`, `12.4 KB`, `3.1 MB` (decimal units, like Finder).
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64 / 1000.0;
    let mut unit = 0;
    while value >= 1000.0 && unit + 1 < UNITS.len() {
        value /= 1000.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// One file of a Snapshot version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SiteFile {
    /// URL path, e.g. `/assets/app.js`.
    pub path: String,
    /// Content hash.
    pub hash: String,
    /// Size in bytes.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub size: u64,
    /// Media type it's served with.
    pub content_type: String,
}

/// The files of a version and where to read them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteContent {
    /// The folder the paths are relative to. `None`: the files are the live version's,
    /// already on Cloudflare (a settings-only change).
    pub root: Option<PathBuf>,
    /// Files, sorted by path.
    pub files: Vec<SiteFile>,
    /// A `_headers` file's rules, sent as configuration rather than as a file.
    pub headers: Option<String>,
    /// A `_redirects` file's rules.
    pub redirects: Option<String>,
}

impl SiteContent {
    /// Total size in bytes.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// The manifest Cloudflare gets.
    pub fn manifest(&self) -> BTreeMap<String, AssetEntry> {
        self.files
            .iter()
            .map(|f| {
                (
                    f.path.clone(),
                    AssetEntry {
                        hash: f.hash.clone(),
                        size: f.size,
                    },
                )
            })
            .collect()
    }

    /// Files (count, bytes) that aren't in `previous` with the same content.
    pub fn changes_from(&self, previous: &[SiteFile]) -> (u64, u64) {
        let old: BTreeMap<&str, &str> = previous
            .iter()
            .map(|f| (f.path.as_str(), f.hash.as_str()))
            .collect();
        self.files
            .iter()
            .filter(|f| old.get(f.path.as_str()) != Some(&f.hash.as_str()))
            .fold((0, 0), |(n, b), f| (n + 1, b + f.size))
    }
}

/// Where a Snapshot answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SiteAddress {
    /// A hostname on one of the account's domains (a Workers Custom Domain).
    Domain {
        /// The hostname.
        hostname: Hostname,
    },
    /// `<script>.<subdomain>.workers.dev`.
    WorkersDev,
}

impl SiteAddress {
    /// The hostname, for a custom one.
    pub fn hostname(&self) -> Option<&Hostname> {
        match self {
            Self::Domain { hostname } => Some(hostname),
            Self::WorkersDev => None,
        }
    }
}

/// A Snapshot as the engine knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteSpec {
    /// Local id (the store's).
    pub id: String,
    /// The name the user gave it.
    pub name: String,
    /// The Worker's name.
    pub script: String,
    /// Where it answers.
    pub address: SiteAddress,
    /// Require a Cloudflare Access login (custom hostnames only).
    pub access: Option<AccessRule>,
}

impl SiteSpec {
    /// The hostname, or the Worker's name for a workers.dev address (for messages).
    pub fn label(&self) -> String {
        self.address
            .hostname()
            .map_or_else(|| self.script.clone(), ToString::to_string)
    }
}

/// A password on a version.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Password {
    /// Open to anyone with the address.
    #[default]
    Off,
    /// Keep the password the live version has.
    Keep,
    /// Set this password hash (see `snapshot::password`); never serialized.
    Set {
        /// The hash; never serialized.
        #[serde(skip)]
        hash: Secret<String>,
    },
}

/// How a version answers.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteSettings {
    /// Unknown paths get `/index.html` (single-page apps); otherwise the nearest
    /// `404.html`.
    pub spa: bool,
    /// Password protection, checked by the Worker.
    pub password: Password,
    /// A script injected into every HTML page (the comments overlay, designed
    /// separately): the Worker adds `<script src=… defer>` when set.
    pub overlay: Option<String>,
}

impl SiteSettings {
    /// Whether the Worker runs before the files (a password or the overlay needs it;
    /// otherwise requests cost nothing).
    pub fn worker_first(&self) -> bool {
        !matches!(self.password, Password::Off) || self.overlay.is_some()
    }
}

/// The Worker's metadata for a version (Cloudflare's "Upload Worker Module" shape).
pub fn metadata(
    settings: &SiteSettings,
    content: &SiteContent,
    assets_jwt: &str,
    message: &str,
) -> Value {
    let mut config = json!({
        "html_handling": "auto-trailing-slash",
        "not_found_handling": if settings.spa { "single-page-application" } else { "404-page" },
        "run_worker_first": settings.worker_first(),
    });
    if let Some(headers) = &content.headers {
        config["_headers"] = json!(headers);
    }
    if let Some(redirects) = &content.redirects {
        config["_redirects"] = json!(redirects);
    }
    let mut bindings = vec![json!({ "type": "assets", "name": "ASSETS" })];
    let mut keep = Vec::new();
    match &settings.password {
        Password::Off => {}
        Password::Keep => keep.push(json!("secret_text")),
        Password::Set { hash } => bindings.push(json!({
            "type": "secret_text", "name": "PASSWORD_HASH", "text": hash.expose()
        })),
    }
    if let Some(src) = &settings.overlay {
        bindings.push(json!({ "type": "plain_text", "name": "OVERLAY_SRC", "text": src }));
    }
    let mut metadata = json!({
        "main_module": WORKER_MODULE,
        "compatibility_date": COMPATIBILITY_DATE,
        "assets": { "jwt": assets_jwt, "config": config },
        "bindings": bindings,
        "annotations": { "workers/message": message },
    });
    if !keep.is_empty() {
        metadata["keep_bindings"] = Value::Array(keep);
    }
    metadata
}

/// The Worker's code.
pub fn modules() -> Vec<WorkerModule> {
    vec![WorkerModule {
        name: WORKER_MODULE.to_owned(),
        content: WORKER_JS.to_owned(),
    }]
}

/// What a snapshot change needs observed about its Worker.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SiteNeed {
    /// The Worker (none: nothing to read).
    pub script: Option<String>,
    /// A hostname it's to answer on (to find another Worker already serving it).
    pub hostname: Option<String>,
}

/// A snapshot's Worker as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SiteState {
    /// The Worker's name.
    pub script: String,
    /// Whether it exists.
    pub exists: bool,
    /// The version serving it.
    pub active_version: Option<String>,
    /// Whether it answers on workers.dev.
    pub workers_dev: bool,
    /// The account's workers.dev subdomain, if it has one.
    pub subdomain: Option<String>,
    /// Custom Domains it serves.
    pub domains: Vec<WorkerDomain>,
    /// Another Worker already serving the requested hostname.
    pub hostname_taken_by: Option<String>,
}

/// Reads what `need` asks for.
///
/// # Errors
/// API errors.
pub(crate) async fn observe<C: CloudApi>(
    api: &C,
    account: &str,
    need: &SiteNeed,
) -> Result<Option<SiteState>, cf_api::Error> {
    let Some(script) = &need.script else {
        return Ok(None);
    };
    let (deployments, subdomain) = tokio::try_join!(
        api.worker_deployments(account, script),
        api.workers_subdomain(account)
    )?;
    let exists = deployments.is_some();
    let active_version = deployments
        .as_ref()
        .and_then(|d| d.first())
        .and_then(|d| d.main_version())
        .map(str::to_owned);
    let (mut domains, workers_dev) = if exists {
        tokio::try_join!(
            api.worker_domains(account, Some(script), None),
            api.worker_on_workers_dev(account, script)
        )?
    } else {
        (Vec::new(), false)
    };
    domains.retain(|d| d.service == *script);
    domains.sort_by(|a, b| a.hostname.cmp(&b.hostname));
    let hostname_taken_by = match &need.hostname {
        Some(hostname) => api
            .worker_domains(account, None, Some(hostname))
            .await?
            .into_iter()
            .find(|d| d.hostname.eq_ignore_ascii_case(hostname) && d.service != *script)
            .map(|d| d.service),
        None => None,
    };
    Ok(Some(SiteState {
        script: script.clone(),
        exists,
        active_version,
        workers_dev,
        subdomain,
        domains,
        hostname_taken_by,
    }))
}

/// Upload progress: files and bytes sent so far, of the total to send.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Transfer {
    /// Files sent.
    pub files: u64,
    /// Of this many.
    pub total_files: u64,
    /// Bytes sent.
    pub bytes: u64,
    /// Of this many.
    pub total_bytes: u64,
}

/// The file at `path` under `root`, refusing anything that would resolve outside it
/// (a symlink or a file replaced since it was collected).
fn read_inside(root: &Path, path: &str) -> Result<Vec<u8>, Text> {
    let relative = Path::new(path.trim_start_matches('/'));
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(msg::snapshot::error::outside(path));
    }
    let root = root
        .canonicalize()
        .map_err(|e| msg::snapshot::error::read(path, e))?;
    let full = root
        .join(relative)
        .canonicalize()
        .map_err(|e| msg::snapshot::error::read(path, e))?;
    if !full.starts_with(&root) {
        return Err(msg::snapshot::error::outside(path));
    }
    std::fs::read(&full).map_err(|e| msg::snapshot::error::read(path, e))
}

/// Uploads the files of `content` that Cloudflare doesn't have yet and returns the
/// completion token a version is created with. Each file is re-read and re-hashed: one
/// that changed since the preview fails the upload rather than publishing something
/// else than what was reviewed.
///
/// # Errors
/// API errors, unreadable or changed files.
pub(crate) async fn upload<C: CloudApi>(
    api: &C,
    account: &str,
    script: &str,
    content: &SiteContent,
    mut progress: impl FnMut(Transfer),
) -> Result<String, Text> {
    let session = api
        .create_assets_upload_session(account, script, &content.manifest())
        .await
        .map_err(|e| e.text())?;
    if session.buckets.is_empty() {
        return Ok(session.jwt);
    }
    let Some(root) = &content.root else {
        return Err(msg::snapshot::error::files_missing());
    };
    let by_hash: BTreeMap<&str, &SiteFile> =
        content.files.iter().map(|f| (f.hash.as_str(), f)).collect();
    let wanted: Vec<&SiteFile> = session
        .buckets
        .iter()
        .flatten()
        .filter_map(|h| by_hash.get(h.as_str()).copied())
        .collect();
    let mut transfer = Transfer {
        total_files: wanted.len() as u64,
        total_bytes: wanted.iter().map(|f| f.size).sum(),
        ..Transfer::default()
    };
    progress(transfer);
    let mut completion = None;
    for bucket in &session.buckets {
        for chunk in bucket.chunks(BUCKET_FILES) {
            let mut files = Vec::with_capacity(chunk.len());
            for hash in chunk {
                let file = by_hash
                    .get(hash.as_str())
                    .ok_or_else(|| msg::snapshot::error::unknown_hash(hash))?;
                let root = root.clone();
                let path = file.path.clone();
                let bytes = tokio::task::spawn_blocking(move || read_inside(&root, &path))
                    .await
                    .map_err(|e| msg::raw(e.to_string()))??;
                if bytes.len() as u64 > MAX_FILE_SIZE {
                    return Err(msg::snapshot::error::too_large(&file.path));
                }
                if content_hash(&bytes, &file.path) != file.hash {
                    return Err(msg::snapshot::error::changed(&file.path));
                }
                files.push(AssetFile {
                    hash: file.hash.clone(),
                    content_type: file.content_type.clone(),
                    content: bytes,
                });
            }
            let sent: u64 = files.iter().map(|f| f.content.len() as u64).sum();
            let count = files.len() as u64;
            if let Some(jwt) = api
                .upload_assets(account, &session.jwt, &files)
                .await
                .map_err(|e| e.text())?
            {
                completion = Some(jwt);
            }
            transfer.files += count;
            transfer.bytes += sent;
            progress(transfer);
        }
    }
    completion.ok_or_else(msg::snapshot::error::not_confirmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(path: &str, hash: &str, size: u64) -> SiteFile {
        SiteFile {
            path: path.into(),
            hash: hash.into(),
            size,
            content_type: "text/html".into(),
        }
    }

    #[test]
    fn hashes_like_cloudflares_example() {
        let hash = content_hash(b"<h1>hi</h1>", "/index.html");
        assert_eq!(hash.len(), 32);
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
        // The extension is part of the key; the path otherwise isn't.
        assert_eq!(hash, content_hash(b"<h1>hi</h1>", "/other/page.html"));
        assert_ne!(hash, content_hash(b"<h1>hi</h1>", "/index.htm"));
        assert_ne!(hash, content_hash(b"<h1>hi!</h1>", "/index.html"));
    }

    #[test]
    fn formats_sizes() {
        assert_eq!(format_bytes(0), "0 bytes");
        assert_eq!(format_bytes(999), "999 bytes");
        assert_eq!(format_bytes(12_400), "12.4 KB");
        assert_eq!(format_bytes(3_100_000), "3.1 MB");
        assert_eq!(format_bytes(2_000_000_000_000_000), "2000.0 TB");
    }

    #[test]
    fn counts_what_changed() {
        let content = SiteContent {
            root: None,
            files: vec![
                file("/a", "1", 10),
                file("/b", "2", 20),
                file("/c", "3", 30),
            ],
            headers: None,
            redirects: None,
        };
        let previous = [
            file("/a", "1", 10),
            file("/b", "old", 5),
            file("/gone", "9", 1),
        ];
        assert_eq!(content.changes_from(&previous), (2, 50));
        assert_eq!(content.bytes(), 60);
        assert_eq!(content.manifest()["/b"].hash, "2");
    }

    #[test]
    fn metadata_runs_the_worker_only_when_needed() {
        let content = SiteContent {
            root: None,
            files: Vec::new(),
            headers: Some("/*\n  X-Robots-Tag: noindex".into()),
            redirects: None,
        };
        let open = metadata(&SiteSettings::default(), &content, "jwt", "Teitunnel");
        assert_eq!(open["assets"]["config"]["run_worker_first"], false);
        assert_eq!(open["assets"]["config"]["not_found_handling"], "404-page");
        assert_eq!(
            open["assets"]["config"]["_headers"],
            "/*\n  X-Robots-Tag: noindex"
        );
        assert_eq!(open["assets"]["jwt"], "jwt");
        assert_eq!(open["bindings"].as_array().unwrap().len(), 1);
        assert!(open.get("keep_bindings").is_none());

        let locked = SiteSettings {
            spa: true,
            password: Password::Set {
                hash: Secret::new("pbkdf2-sha256$1$s$h".into()),
            },
            overlay: None,
        };
        let meta = metadata(&locked, &content, "jwt", "m");
        assert_eq!(meta["assets"]["config"]["run_worker_first"], true);
        assert_eq!(
            meta["assets"]["config"]["not_found_handling"],
            "single-page-application"
        );
        assert_eq!(meta["bindings"][1]["type"], "secret_text");
        assert_eq!(meta["bindings"][1]["text"], "pbkdf2-sha256$1$s$h");
        // The hash never shows in debug output or serialized settings.
        assert!(!format!("{locked:?}").contains("pbkdf2"));
        assert!(!serde_json::to_string(&locked).unwrap().contains("pbkdf2"));

        let kept = SiteSettings {
            password: Password::Keep,
            ..SiteSettings::default()
        };
        let meta = metadata(&kept, &content, "jwt", "m");
        assert_eq!(meta["keep_bindings"], json!(["secret_text"]));
        assert_eq!(meta["assets"]["config"]["run_worker_first"], true);
    }

    #[test]
    fn reads_only_inside_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "a").unwrap();
        assert_eq!(read_inside(dir.path(), "/a.txt").unwrap(), b"a");
        assert!(read_inside(dir.path(), "/../a.txt").is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret"), "s").unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("link"))
                .unwrap();
            assert!(read_inside(dir.path(), "/link").is_err());
        }
    }
}
