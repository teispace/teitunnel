//! Collecting a folder's files for a Snapshot: only regular files inside the folder,
//! never secrets or tooling (dotfiles, `.env`, `.git`, `node_modules`, keys), with
//! Cloudflare's limits checked before anything is uploaded.

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Serialize;

use super::SnapshotError;
use crate::engine::{MAX_FILE_SIZE, MAX_FILES, SiteContent, SiteFile, content_hash};

/// Why a file was left out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// A hidden file or folder (name starting with `.`).
    Hidden,
    /// Looks like a secret: `.env` files, private keys.
    Secret,
    /// Version control data.
    VersionControl,
    /// Installed dependencies (`node_modules`).
    Dependencies,
    /// A link to something outside the folder, or to a folder.
    Link,
    /// Not a regular file (socket, device…).
    Special,
    /// Its name isn't valid text, so it has no URL.
    Name,
}

/// A file left out, relative to the folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Skipped {
    /// Path relative to the folder, e.g. `.env`.
    pub path: String,
    /// Why.
    pub reason: SkipReason,
}

/// What a folder yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collected {
    /// The files to publish.
    pub content: SiteContent,
    /// What was left out (folders are listed once, not per file).
    pub skipped: Vec<Skipped>,
}

/// Names that are secrets wherever they are.
fn is_secret(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with(".env")
        || [".pem", ".key", ".p12", ".pfx", ".keystore", ".jks"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        || matches!(
            lower.as_str(),
            "id_rsa" | "id_dsa" | "id_ecdsa" | "id_ed25519" | ".npmrc" | ".netrc" | ".pgpass"
        )
}

fn skip_reason(name: &str, is_dir: bool) -> Option<SkipReason> {
    if is_secret(name) {
        return Some(SkipReason::Secret);
    }
    match name {
        ".git" | ".hg" | ".svn" => Some(SkipReason::VersionControl),
        "node_modules" => Some(SkipReason::Dependencies),
        // Needed by the web (security.txt, app links, ACME).
        ".well-known" if is_dir => None,
        _ if name.starts_with('.') => Some(SkipReason::Hidden),
        _ => None,
    }
}

/// The media type a file is served with, by extension.
pub fn content_type(path: &str) -> &'static str {
    let extension = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" | "cjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "webmanifest" => "application/manifest+json",
        "xml" => "application/xml",
        "txt" | "md" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
}

struct Walk {
    root: PathBuf,
    files: Vec<SiteFile>,
    skipped: Vec<Skipped>,
}

impl Walk {
    fn relative(&self, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(&self.root).ok()?;
        let parts: Option<Vec<&str>> = relative
            .components()
            .map(|c| c.as_os_str().to_str())
            .collect();
        Some(parts?.join("/"))
    }

    fn skip(&mut self, path: &Path, reason: SkipReason) {
        let shown = self
            .relative(path)
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        self.skipped.push(Skipped {
            path: shown,
            reason,
        });
    }

    fn visit(&mut self, dir: &Path) -> Result<(), SnapshotError> {
        let mut entries: Vec<fs::DirEntry> = fs::read_dir(dir)
            .map_err(|e| SnapshotError::io(dir, &e))?
            .collect::<Result<_, _>>()
            .map_err(|e| SnapshotError::io(dir, &e))?;
        entries.sort_by_key(fs::DirEntry::file_name);
        for entry in entries {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                self.skip(&path, SkipReason::Name);
                continue;
            };
            let meta = fs::symlink_metadata(&path).map_err(|e| SnapshotError::io(&path, &e))?;
            let file_type = meta.file_type();
            let is_dir = file_type.is_dir();
            if let Some(reason) = skip_reason(&name, is_dir) {
                self.skip(&path, reason);
                continue;
            }
            if dir == self.root && (name == "_headers" || name == "_redirects") {
                // Read as configuration by `collect`.
                continue;
            }
            if is_dir {
                self.visit(&path)?;
                continue;
            }
            let size = if file_type.is_symlink() {
                // Followed only to a regular file inside the folder.
                match fs::canonicalize(&path) {
                    Ok(target) if target.starts_with(&self.root) && target.is_file() => {
                        fs::metadata(&target)
                            .map_err(|e| SnapshotError::io(&path, &e))?
                            .len()
                    }
                    _ => {
                        self.skip(&path, SkipReason::Link);
                        continue;
                    }
                }
            } else if file_type.is_file() {
                meta.len()
            } else {
                self.skip(&path, SkipReason::Special);
                continue;
            };
            let Some(relative) = self.relative(&path) else {
                self.skip(&path, SkipReason::Name);
                continue;
            };
            let url_path = format!("/{relative}");
            if size > MAX_FILE_SIZE {
                return Err(SnapshotError::FileTooLarge(url_path));
            }
            if self.files.len() >= MAX_FILES {
                return Err(SnapshotError::TooManyFiles(MAX_FILES));
            }
            let bytes = fs::read(&path).map_err(|e| SnapshotError::io(&path, &e))?;
            self.files.push(SiteFile {
                hash: content_hash(&bytes, &url_path),
                size: bytes.len() as u64,
                content_type: content_type(&url_path).to_owned(),
                path: url_path,
            });
        }
        Ok(())
    }
}

fn read_config(root: &Path, name: &str) -> Result<Option<String>, SnapshotError> {
    let path = root.join(name);
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.is_file() => fs::read_to_string(&path)
            .map(Some)
            .map_err(|e| SnapshotError::io(&path, &e)),
        _ => Ok(None),
    }
}

/// Collects the files under `folder` (blocking: call it off the async runtime).
///
/// # Errors
/// Not a folder, no files, a file over 25 MiB, more than 20,000 files, or an I/O error.
pub fn collect(folder: &Path) -> Result<Collected, SnapshotError> {
    let root = fs::canonicalize(folder).map_err(|e| SnapshotError::io(folder, &e))?;
    if !root.is_dir() {
        return Err(SnapshotError::NotAFolder(folder.display().to_string()));
    }
    let mut walk = Walk {
        root: root.clone(),
        files: Vec::new(),
        skipped: Vec::new(),
    };
    walk.visit(&root)?;
    if walk.files.is_empty() {
        return Err(SnapshotError::Empty(folder.display().to_string()));
    }
    walk.files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(Collected {
        content: SiteContent {
            headers: read_config(&root, "_headers")?,
            redirects: read_config(&root, "_redirects")?,
            root: Some(root),
            files: walk.files,
        },
        skipped: walk.skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, path: &str, content: &str) {
        let full = root.join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, content).unwrap();
    }

    #[test]
    fn collects_files_and_leaves_out_secrets_and_tooling() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "index.html", "<h1>hi</h1>");
        write(root, "assets/app.js", "console.log(1)");
        write(root, ".well-known/security.txt", "Contact: x");
        write(root, ".env", "SECRET=1");
        write(root, ".env.local", "SECRET=1");
        write(root, ".git/config", "[core]");
        write(root, "node_modules/x/index.js", "");
        write(root, ".DS_Store", "");
        write(root, "certs/server.key", "-----BEGIN");
        write(root, "_headers", "/*\n  X-Robots-Tag: noindex");
        let collected = collect(root).unwrap();
        let paths: Vec<&str> = collected
            .content
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(
            paths,
            ["/.well-known/security.txt", "/assets/app.js", "/index.html"]
        );
        assert_eq!(
            collected.content.headers.as_deref(),
            Some("/*\n  X-Robots-Tag: noindex")
        );
        let skipped: Vec<(&str, SkipReason)> = collected
            .skipped
            .iter()
            .map(|s| (s.path.as_str(), s.reason))
            .collect();
        assert_eq!(
            skipped,
            [
                (".DS_Store", SkipReason::Hidden),
                (".env", SkipReason::Secret),
                (".env.local", SkipReason::Secret),
                (".git", SkipReason::VersionControl),
                ("certs/server.key", SkipReason::Secret),
                ("node_modules", SkipReason::Dependencies),
            ]
        );
        let index = &collected.content.files[2];
        assert_eq!(index.content_type, "text/html; charset=utf-8");
        assert_eq!(index.hash, content_hash(b"<h1>hi</h1>", "/index.html"));
        assert_eq!(index.size, 11);
    }

    #[cfg(unix)]
    #[test]
    fn never_follows_links_out_of_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path(), "passwords.txt", "hunter2");
        write(dir.path(), "index.html", "ok");
        write(dir.path(), "real.txt", "inside");
        std::os::unix::fs::symlink(
            outside.path().join("passwords.txt"),
            dir.path().join("leak.txt"),
        )
        .unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("leakdir")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("real.txt"), dir.path().join("alias.txt"))
            .unwrap();
        let collected = collect(dir.path()).unwrap();
        let paths: Vec<&str> = collected
            .content
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect();
        assert_eq!(paths, ["/alias.txt", "/index.html", "/real.txt"]);
        let links: Vec<&str> = collected
            .skipped
            .iter()
            .filter(|s| s.reason == SkipReason::Link)
            .map(|s| s.path.as_str())
            .collect();
        assert_eq!(links, ["leak.txt", "leakdir"]);
    }

    #[test]
    fn enforces_cloudflares_limits_before_uploading() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "index.html", "ok");
        let big = fs::File::create(dir.path().join("video.mp4")).unwrap();
        big.set_len(MAX_FILE_SIZE + 1).unwrap();
        assert!(matches!(
            collect(dir.path()),
            Err(SnapshotError::FileTooLarge(path)) if path == "/video.mp4"
        ));
        let empty = tempfile::tempdir().unwrap();
        write(empty.path(), ".env", "x");
        assert!(matches!(
            collect(empty.path()),
            Err(SnapshotError::Empty(_))
        ));
        assert!(matches!(
            collect(&empty.path().join(".env")),
            Err(SnapshotError::NotAFolder(_))
        ));
    }

    #[test]
    fn serves_common_types() {
        assert_eq!(content_type("/a/b.CSS"), "text/css; charset=utf-8");
        assert_eq!(content_type("/font.woff2"), "font/woff2");
        assert_eq!(content_type("/noext"), "application/octet-stream");
    }
}
