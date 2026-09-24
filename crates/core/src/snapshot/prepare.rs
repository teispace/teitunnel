//! Preparing files for a Snapshot: collected and hashed once, kept in memory under an id
//! while the user reviews the plan, so the preview and the apply publish the same files.

use std::{
    collections::HashMap,
    path::Path,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use serde::Serialize;

use super::{
    SnapshotError, SnapshotSource,
    build::{BuildCommand, Project},
    content::{Skipped, collect},
    crawl::{CrawlReport, Limits, crawl},
};
use crate::engine::SiteContent;

/// How long prepared files are kept without being published.
const KEEP: Duration = Duration::from_secs(60 * 60);

/// Files ready to publish.
#[derive(Debug, Clone)]
pub struct Prepared {
    /// Where they come from.
    pub source: SnapshotSource,
    /// The files.
    pub content: SiteContent,
    /// What was left out.
    pub skipped: Vec<Skipped>,
    /// A crawl's report.
    pub crawl: Option<CrawlReport>,
    at: Instant,
}

/// Prepared files, for review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PreparedView {
    /// Pass back to preview and publish.
    pub id: String,
    /// Where they come from.
    pub source: SnapshotSource,
    /// A name to suggest.
    pub suggested_name: String,
    /// Files.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub files: u64,
    /// Bytes.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub bytes: u64,
    /// Left out (secrets, tooling).
    pub skipped: Vec<Skipped>,
    /// Probably a single-page app (only one HTML page).
    pub single_page: bool,
    /// A crawl's report.
    pub crawl: Option<CrawlReport>,
}

/// Prepared files by id. One per app.
#[derive(Debug, Default)]
pub struct Preparations {
    entries: Mutex<HashMap<String, Prepared>>,
}

fn single_page(content: &SiteContent) -> bool {
    let pages = content
        .files
        .iter()
        .filter(|f| f.content_type.starts_with("text/html"))
        .count();
    pages == 1 && content.files.iter().any(|f| f.path == "/index.html")
}

/// Whether `url` is a site on this computer or its local network (a dev server, a
/// share's origin): Snapshots copy the user's own work, not other people's sites.
pub(crate) fn is_local(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    let Some(host) = parsed.host_str() else {
        return false;
    };
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback() || ip.is_unique_local(),
        Err(_) => {
            let name = host.to_ascii_lowercase();
            name == "localhost" || name.ends_with(".localhost") || name.ends_with(".local")
        }
    }
}

fn name_from(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map_or_else(|| "site".to_owned(), str::to_owned)
}

impl Preparations {
    /// Prepared files, if still kept.
    pub fn get(&self, id: &str) -> Option<Prepared> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, p| p.at.elapsed() < KEEP);
        entries.get(id).cloned()
    }

    /// Forgets prepared files (published).
    pub fn remove(&self, id: &str) {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }

    fn keep(&self, prepared: Prepared, suggested_name: String) -> PreparedView {
        let id = uuid::Uuid::new_v4().to_string();
        let view = PreparedView {
            id: id.clone(),
            source: prepared.source.clone(),
            suggested_name,
            files: prepared.content.files.len() as u64,
            bytes: prepared.content.bytes(),
            skipped: prepared.skipped.clone(),
            single_page: prepared
                .crawl
                .as_ref()
                .map_or_else(|| single_page(&prepared.content), |c| c.single_page),
            crawl: prepared.crawl.clone(),
        };
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, prepared);
        view
    }

    /// Collects a folder as it is.
    ///
    /// # Errors
    /// See [`collect`].
    pub async fn folder(&self, path: &Path) -> Result<PreparedView, SnapshotError> {
        let owned = path.to_path_buf();
        let collected = tokio::task::spawn_blocking(move || collect(&owned))
            .await
            .map_err(|e| SnapshotError::Crawl(e.to_string()))??;
        let root = collected
            .content
            .root
            .clone()
            .unwrap_or_else(|| path.to_path_buf());
        Ok(self.keep(
            Prepared {
                source: SnapshotSource::Folder {
                    path: root.display().to_string(),
                },
                content: collected.content,
                skipped: collected.skipped,
                crawl: None,
                at: Instant::now(),
            },
            name_from(&root),
        ))
    }

    /// Builds a project (if it needs building), then collects its output. Each line of
    /// the build's output goes to `line`.
    ///
    /// # Errors
    /// The build failed, or its output can't be collected.
    pub async fn build(
        &self,
        project: &Project,
        line: impl FnMut(&str) + Send,
    ) -> Result<PreparedView, SnapshotError> {
        let command = BuildCommand::for_project(project)?;
        if let Some(command) = &command {
            command.run(line).await?;
        }
        let output = Path::new(&project.output);
        if !output.is_dir() {
            return Err(SnapshotError::NoOutput(project.output.clone()));
        }
        let owned = output.to_path_buf();
        let collected = tokio::task::spawn_blocking(move || collect(&owned))
            .await
            .map_err(|e| SnapshotError::Crawl(e.to_string()))??;
        let source = match &command {
            Some(command) => SnapshotSource::Build {
                project: project.dir.clone(),
                command: command.display(),
                output: project.output.clone(),
            },
            None => SnapshotSource::Folder {
                path: project.output.clone(),
            },
        };
        Ok(self.keep(
            Prepared {
                source,
                content: collected.content,
                skipped: collected.skipped,
                crawl: None,
                at: Instant::now(),
            },
            project.name.clone(),
        ))
    }

    /// Captures a running site into `dest` (a fresh folder), then collects it.
    ///
    /// # Errors
    /// See [`crawl`] and [`collect`].
    pub async fn crawl(
        &self,
        url: &str,
        dest: &Path,
        limits: Limits,
    ) -> Result<PreparedView, SnapshotError> {
        if !is_local(url) {
            return Err(SnapshotError::NotLocal(url.to_owned()));
        }
        let report = crawl(url, dest, limits).await?;
        let owned = dest.to_path_buf();
        let collected = tokio::task::spawn_blocking(move || collect(&owned))
            .await
            .map_err(|e| SnapshotError::Crawl(e.to_string()))??;
        let name = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| {
                u.port()
                    .map(|p| format!("{}-{p}", u.host_str().unwrap_or("site")))
                    .or_else(|| u.host_str().map(str::to_owned))
            })
            .unwrap_or_else(|| "site".to_owned());
        Ok(self.keep(
            Prepared {
                source: SnapshotSource::Crawl {
                    url: url.to_owned(),
                },
                content: collected.content,
                skipped: collected.skipped,
                crawl: Some(report),
                at: Instant::now(),
            },
            name,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_only_local_sites() {
        for local in [
            "http://localhost:5173/",
            "http://127.0.0.1:3000",
            "http://[::1]:8080/",
            "http://192.168.1.20:4000/",
            "http://app.localhost:3000/",
            "http://mac.local/",
        ] {
            assert!(is_local(local), "{local}");
        }
        for remote in ["https://example.com/", "http://8.8.8.8/", "not a url"] {
            assert!(!is_local(remote), "{remote}");
        }
    }

    #[tokio::test]
    async fn refuses_to_capture_someone_elses_site() {
        let dir = tempfile::tempdir().unwrap();
        let result = Preparations::default()
            .crawl("https://example.com/", dir.path(), Limits::default())
            .await;
        assert!(matches!(result, Err(SnapshotError::NotLocal(_))));
    }
}
