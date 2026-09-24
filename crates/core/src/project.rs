//! Project files: `teitunnel.yml` in a repository declares the project's routes, shares,
//! Snapshots and local domains, so a team shares one setup and `teitunnel up` (or
//! opening the folder in the app) brings it up (M12-12).
//!
//! The file never holds a secret (it's checked into git): passwords are references to an
//! environment variable or a keychain entry, and anything that looks like a credential
//! is refused. Applying it goes through the same plan → apply engine as every other
//! change, previewed first; a second apply of an applied file changes nothing.

pub mod init;
mod plan;
pub mod registry;
mod schema;
mod secret_scan;
mod state;
pub mod template;
mod yaml;

use std::path::{Path, PathBuf};

pub use plan::{
    ProjectPlan, RouteAction, ShareAction, SnapshotAction, apply_route, plan, resolve_secret,
    snapshot_change,
};
pub use schema::{
    Diagnostic, HostHeaderDecl, LocalDomainDecl, Parsed, ProjectFile, RouteDecl, SecretRef,
    Severity, ShareDecl, SnapshotDecl, SnapshotSourceDecl, VERSION, parse, parse_duration,
    valid_local_name,
};
pub use state::{ItemKind, ItemState, ProjectItem, Resolved, ResolvedShare, resolve, status};
pub use yaml::Pos;

use crate::text::{Text, UserText, english_display, msg::project as m};

/// The file's name (`teitunnel.yaml` is read too).
pub const FILE_NAME: &str = "teitunnel.yml";
const OTHER_NAME: &str = "teitunnel.yaml";
/// Larger files are refused (a project file is a few hundred lines at most).
const MAX_SIZE: u64 = 256 * 1024;
/// The published JSON Schema, for editors.
pub const SCHEMA_URL: &str = "https://teitunnel.teispace.com/schema/teitunnel.v1.json";

/// Why a project file couldn't be used.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// No `teitunnel.yml` in the folder (or above it, up to the repository's root).
    NotFound(String),
    /// Reading the file failed.
    Io {
        /// The path.
        path: String,
        /// What went wrong.
        detail: String,
    },
    /// Over 256 KiB.
    TooLarge(String),
    /// The file has errors (see its diagnostics).
    Invalid(usize),
    /// A placeholder has no value here, or a hostname is invalid once filled in.
    Unresolved(Text),
    /// Cloudflare or this machine changed since the plan was shown.
    Changed,
    /// The engine refused or failed.
    Engine(#[from] crate::engine::EngineError),
    /// The local database.
    Store(#[from] crate::store::StoreError),
    /// A Snapshot couldn't be prepared or planned.
    Snapshot(#[from] crate::snapshot::SnapshotError),
    /// A secret reference couldn't be read.
    Secret(Text),
}

impl UserText for ProjectError {
    fn text(&self) -> Text {
        match self {
            Self::NotFound(dir) => m::not_found(dir),
            Self::Io { path, detail } => m::io(path, detail),
            Self::TooLarge(path) => m::too_large(path),
            Self::Invalid(count) => m::invalid(*count as u64),
            Self::Unresolved(text) | Self::Secret(text) => text.clone(),
            Self::Changed => m::changed(),
            Self::Engine(err) => err.text(),
            Self::Store(err) => err.text(),
            Self::Snapshot(err) => err.text(),
        }
    }
}

english_display!(ProjectError);

/// A project file, read and validated.
#[derive(Debug, Clone)]
pub struct Loaded {
    /// The file.
    pub path: PathBuf,
    /// Its folder (relative paths in it are relative to this).
    pub dir: PathBuf,
    /// The project's name: the file's `project`, else detected from the folder.
    pub name: String,
    /// The model and its diagnostics.
    pub parsed: Parsed,
    /// Placeholder values on this machine.
    pub vars: template::Vars,
}

impl Loaded {
    /// The model, when the file has no errors.
    ///
    /// # Errors
    /// [`ProjectError::Invalid`] with the number of errors.
    pub fn file(&self) -> Result<&ProjectFile, ProjectError> {
        self.parsed.file.as_ref().ok_or_else(|| {
            ProjectError::Invalid(
                self.parsed
                    .diagnostics
                    .iter()
                    .filter(|d| d.severity == Severity::Error)
                    .count(),
            )
        })
    }
}

/// The project file for `dir`: in it, or in a folder above it up to the repository's
/// root (the folder with `.git`).
pub fn find(dir: &Path) -> Option<PathBuf> {
    let mut current = Some(dir);
    while let Some(folder) = current {
        for name in [FILE_NAME, OTHER_NAME] {
            let candidate = folder.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if folder.join(".git").exists() {
            return None;
        }
        current = folder.parent();
    }
    None
}

/// Where the file is when `path` is a project file or a folder with one.
///
/// # Errors
/// [`ProjectError::NotFound`].
pub fn locate(path: &Path) -> Result<PathBuf, ProjectError> {
    if path.is_file() {
        return Ok(path.to_path_buf());
    }
    find(path).ok_or_else(|| ProjectError::NotFound(path.display().to_string()))
}

/// Reads and validates the project file at `path` (a file, or a folder with one).
///
/// # Errors
/// The file is missing, unreadable or too large. Problems in its content are in
/// [`Loaded::parsed`], not errors.
pub fn load(path: &Path) -> Result<Loaded, ProjectError> {
    let path = locate(path)?;
    let io = |e: std::io::Error| ProjectError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    };
    let size = std::fs::metadata(&path).map_err(io)?.len();
    if size > MAX_SIZE {
        return Err(ProjectError::TooLarge(path.display().to_string()));
    }
    let text = std::fs::read_to_string(&path).map_err(io)?;
    let dir = path
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let parsed = parse(&text);
    let name = parsed
        .file
        .as_ref()
        .and_then(|f| f.project.clone())
        .or_else(|| crate::discovery::project_name(&dir).map(|n| template::label(&n)))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "project".to_owned());
    let vars = template::Vars::for_dir(&dir, &name);
    Ok(Loaded {
        path,
        dir,
        name,
        parsed,
        vars,
    })
}

#[cfg(test)]
mod tests;
