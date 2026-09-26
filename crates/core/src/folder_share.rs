//! Sharing a folder: Lens serves it as static files (`crates/lens`'s folder
//! upstream) behind a Quick Share or a share on your domain. Paths can't leave the
//! folder (no `..`, links resolved and checked), and what Snapshots never publish is
//! never served either: dotfiles, `.env*`, keys, `.git`, `node_modules`
//! ([`crate::snapshot::content::servable`]). Directory listings and a single-page-app
//! fallback are optional.

use std::path::{Path, PathBuf};

use lens::{FolderConfig, NameFilter};
use serde::{Deserialize, Serialize};

use crate::text::{Text, UserText, english_display, msg};

/// A folder to share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FolderShare {
    /// The folder (absolute once resolved).
    pub path: String,
    /// List a folder's files when it has no `index.html`. `None`: only when the shared
    /// folder itself has none, so its address never answers "Not found".
    #[serde(default)]
    pub listing: Option<bool>,
    /// The folder has an `index.html` (set by [`FolderShare::resolve`]).
    #[serde(default)]
    pub has_index: bool,
    /// A single-page app: unknown paths that ask for a page get `/index.html`.
    #[serde(default)]
    pub spa: bool,
}

/// Why a folder can't be shared.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FolderError {
    /// It doesn't exist or isn't a folder.
    NotFolder(String),
    /// The whole disk or the home folder: almost certainly a mistake.
    TooBroad(String),
}

impl UserText for FolderError {
    fn text(&self) -> Text {
        use msg::error::folder as m;
        match self {
            Self::NotFolder(path) => m::not_folder(path),
            Self::TooBroad(path) => m::too_broad(path),
        }
    }
}

english_display!(FolderError);

impl FolderShare {
    /// Resolves `path` (relative to the current folder, `~` for home) to an absolute,
    /// existing folder that's safe to share.
    ///
    /// # Errors
    /// See [`FolderError`].
    pub fn resolve(path: &str, listing: Option<bool>, spa: bool) -> Result<Self, FolderError> {
        let raw = path.trim();
        let expanded = match raw.strip_prefix('~') {
            Some(rest) if rest.is_empty() || rest.starts_with(['/', '\\']) => std::env::home_dir()
                .map_or_else(
                    || PathBuf::from(raw),
                    |home| home.join(rest.trim_start_matches(['/', '\\'])),
                ),
            _ => PathBuf::from(raw),
        };
        let canonical =
            std::fs::canonicalize(&expanded).map_err(|_| FolderError::NotFolder(raw.to_owned()))?;
        if !canonical.is_dir() {
            return Err(FolderError::NotFolder(raw.to_owned()));
        }
        let home = std::env::home_dir().and_then(|h| std::fs::canonicalize(h).ok());
        if canonical.parent().is_none() || Some(&canonical) == home.as_ref() {
            return Err(FolderError::TooBroad(canonical.display().to_string()));
        }
        Ok(Self {
            has_index: canonical.join("index.html").is_file(),
            path: canonical.display().to_string(),
            listing,
            spa,
        })
    }

    /// Lens's settings for serving it.
    pub fn config(&self) -> FolderConfig {
        let mut config = FolderConfig::new(&self.path);
        config.listing = self.lists();
        config.spa_fallback = self.spa;
        config.hidden = false;
        config.allow = NameFilter::new(crate::snapshot::content::servable);
        config
    }

    /// Whether visitors get a file list where there's no `index.html`.
    pub fn lists(&self) -> bool {
        self.listing.unwrap_or(!self.has_index)
    }

    /// The folder's name, for display.
    pub fn name(&self) -> String {
        Path::new(&self.path)
            .file_name()
            .map_or_else(|| self.path.clone(), |n| n.to_string_lossy().into_owned())
    }
}

/// Whether a `share` argument names a folder rather than a service: an existing folder
/// written as a path (`./dist`, `/srv/site`, `~/site`, `dist/`), or any existing folder
/// that isn't a port or `host:port`.
pub fn looks_like_folder(argument: &str) -> bool {
    let argument = argument.trim();
    if argument.is_empty() || argument.contains("://") || argument.parse::<u16>().is_ok() {
        return false;
    }
    let pathlike = argument.starts_with(['.', '/', '~', '\\'])
        || argument.ends_with(['/', '\\'])
        || (argument.len() > 2 && argument.as_bytes()[1] == b':');
    let exists =
        FolderShare::resolve(argument, None, false).is_ok() || Path::new(argument).is_dir();
    exists && (pathlike || !argument.contains(':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_folders_and_refuses_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let site = dir.path().join("site");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("index.html"), "hi").unwrap();
        let share = FolderShare::resolve(site.to_str().unwrap(), Some(true), false).unwrap();
        assert!(Path::new(&share.path).is_absolute());
        assert!(share.lists() && share.has_index && !share.spa);
        assert_eq!(share.name(), "site");
        assert!(matches!(
            FolderShare::resolve(site.join("index.html").to_str().unwrap(), None, false),
            Err(FolderError::NotFolder(_))
        ));
        assert!(matches!(
            FolderShare::resolve("/definitely/not/here", None, false),
            Err(FolderError::NotFolder(_))
        ));
        assert!(matches!(
            FolderShare::resolve("/", None, false),
            Err(FolderError::TooBroad(_))
        ));
        if std::env::home_dir().is_some() {
            assert!(matches!(
                FolderShare::resolve("~", None, false),
                Err(FolderError::TooBroad(_))
            ));
        }
    }

    #[test]
    fn serves_nothing_a_snapshot_wouldnt_publish() {
        let dir = tempfile::tempdir().unwrap();
        let config = FolderShare::resolve(dir.path().to_str().unwrap(), None, true)
            .unwrap()
            .config();
        assert!(config.spa_fallback);
        for refused in [".env", ".env.production", "id_rsa", "server.key", ".npmrc"] {
            assert!(!config.allow.allows(refused, false), "{refused}");
        }
        assert!(!config.allow.allows(".git", true));
        assert!(!config.allow.allows("node_modules", true));
        assert!(config.allow.allows(".well-known", true));
        assert!(config.allow.allows("index.html", false));
    }

    #[test]
    fn lists_a_folder_without_an_index_unless_told_not_to() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap();
        let photos = FolderShare::resolve(path, None, false).unwrap();
        assert!(!photos.has_index);
        assert!(
            photos.config().listing,
            "otherwise its address says Not found"
        );
        assert!(
            !FolderShare::resolve(path, Some(false), false)
                .unwrap()
                .config()
                .listing
        );

        std::fs::write(dir.path().join("index.html"), "hi").unwrap();
        let site = FolderShare::resolve(path, None, false).unwrap();
        assert!(site.has_index);
        assert!(
            !site.config().listing,
            "a site shows its index, not its files"
        );
        assert!(
            FolderShare::resolve(path, Some(true), false)
                .unwrap()
                .config()
                .listing
        );
    }

    #[test]
    fn tells_folders_from_services() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().to_str().unwrap().to_owned();
        assert!(looks_like_folder(&path));
        assert!(looks_like_folder(&format!("{path}/")));
        assert!(!looks_like_folder("3000"));
        assert!(!looks_like_folder("localhost:3000"));
        assert!(!looks_like_folder("http://localhost:3000"));
        assert!(!looks_like_folder("./definitely-not-here"));
    }
}
