//! Stable and branch names for shares on your domain (M12-06): `{project}`, `{branch}`
//! and `{user}` in a hostname (`{branch}.dev.example.com`) are filled in from the folder
//! a share is started from, with the same placeholders and rules as project files
//! (D-107): values become DNS labels, `{branch}` is read from `.git/HEAD` without starting
//! git. The last name used in a folder is remembered (settings key `shareNames`), and
//! the app suggests names from the detected project.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use serde::Serialize;

use crate::{
    discovery,
    domain::Hostname,
    project::template::{self, TemplateError, Vars},
    settings,
    store::{Store, StoreError},
    text::{Text, UserText, english_display, msg},
};

const KEY: &str = "shareNames";
/// Folders remembered at most (the oldest are forgotten).
const MAX_REMEMBERED: usize = 200;

/// Why a name couldn't be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// `{something}` that isn't a placeholder, or a `{` without its `}`.
    Placeholder(String),
    /// The placeholder has no value here (`{branch}` outside a git repository).
    NoValue(String),
    /// The result isn't a valid hostname.
    Invalid(String),
}

impl UserText for NameError {
    fn text(&self) -> Text {
        use msg::error::share_name as m;
        match self {
            Self::Placeholder(name) => m::placeholder(name),
            Self::NoValue(name) => m::no_value(name),
            Self::Invalid(hostname) => m::invalid(hostname),
        }
    }
}

english_display!(NameError);

/// Whether `hostname` has placeholders to fill in.
pub fn is_template(hostname: &str) -> bool {
    hostname.contains('{')
}

/// The project a folder holds: its manifest's name (`package.json`, `Cargo.toml`, …) or
/// the folder's name.
pub fn project_name(dir: &Path) -> Option<String> {
    discovery::project_name(dir).or_else(|| {
        dir.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })
}

/// The placeholder values for `dir`.
pub fn vars_for(dir: &Path) -> Vars {
    Vars::for_dir(dir, &project_name(dir).unwrap_or_default())
}

/// Fills in `hostname`'s placeholders with `vars` and checks the result is a hostname
/// (every label a DNS label).
///
/// # Errors
/// See [`NameError`].
pub fn expand_with(hostname: &str, vars: &Vars) -> Result<String, NameError> {
    let expanded = template::expand(hostname.trim(), vars).map_err(|e| match e {
        TemplateError::Unknown(name) => NameError::Placeholder(format!("{{{name}}}")),
        TemplateError::Unclosed => NameError::Placeholder("{".into()),
        TemplateError::NoValue(name) => NameError::NoValue(format!("{{{name}}}")),
    })?;
    let parsed = Hostname::parse(&expanded).map_err(|_| NameError::Invalid(expanded.clone()))?;
    if parsed.is_wildcard() || !parsed.as_str().contains('.') {
        return Err(NameError::Invalid(expanded));
    }
    Ok(parsed.as_str().to_owned())
}

/// Fills in `hostname`'s placeholders for a share started in `dir`.
///
/// # Errors
/// See [`NameError`].
pub fn expand(hostname: &str, dir: &Path) -> Result<String, NameError> {
    expand_with(hostname, &vars_for(dir))
}

fn folder_key(dir: &Path) -> String {
    std::fs::canonicalize(dir)
        .unwrap_or_else(|_| PathBuf::from(dir))
        .to_string_lossy()
        .into_owned()
}

/// The hostname (template) last shared from `dir`, if any.
///
/// # Errors
/// The database can't be read.
pub async fn remembered(store: &Store, dir: &Path) -> Result<Option<String>, StoreError> {
    let key = folder_key(dir);
    store
        .call(move |conn| {
            let names: BTreeMap<String, (String, u64)> =
                settings::read(conn, KEY)?.unwrap_or_default();
            Ok(names.get(&key).map(|(name, _)| name.clone()))
        })
        .await
}

/// Remembers `hostname` (as typed, placeholders and all) for shares from `dir`.
///
/// # Errors
/// The database can't be written.
pub async fn remember(store: &Store, dir: &Path, hostname: &str) -> Result<(), StoreError> {
    let (key, name) = (folder_key(dir), hostname.trim().to_ascii_lowercase());
    let now = crate::domain_shares::now_ms();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let mut names: BTreeMap<String, (String, u64)> =
                settings::read(&tx, KEY)?.unwrap_or_default();
            names.insert(key, (name, now));
            while names.len() > MAX_REMEMBERED {
                let oldest = names
                    .iter()
                    .min_by_key(|(_, (_, at))| *at)
                    .map(|(key, _)| key.clone());
                match oldest {
                    Some(key) => names.remove(&key),
                    None => break,
                };
            }
            settings::write(&tx, KEY, &names)?;
            tx.commit()?;
            Ok(())
        })
        .await
}

/// A suggested name for a share.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NameSuggestion {
    /// What to type (placeholders kept, so the name follows the branch), e.g.
    /// `{branch}.dev.example.com`.
    pub template: String,
    /// What it is right now, e.g. `login-fix.dev.example.com`.
    pub hostname: String,
    /// Used for this folder last time.
    pub remembered: bool,
}

/// Names to offer for a share on `domain` (a zone or a subdomain of one, e.g.
/// `dev.example.com`) of a service in `dir` (or known only by its `project` name): the
/// one used last in the folder, then `{project}`, `{branch}-{project}` and
/// `{user}-{project}`. Names that can't be made here are left out.
pub fn suggestions(domain: &str, vars: &Vars, remembered: Option<&str>) -> Vec<NameSuggestion> {
    let domain = domain.trim().trim_matches('.').to_ascii_lowercase();
    let mut out: Vec<NameSuggestion> = Vec::new();
    let mut add = |template: String, remembered: bool| {
        if let Ok(hostname) = expand_with(&template, vars)
            && !out.iter().any(|s| s.hostname == hostname)
        {
            out.push(NameSuggestion {
                template,
                hostname,
                remembered,
            });
        }
    };
    if let Some(name) = remembered {
        add(name.to_owned(), true);
    }
    if vars.project.is_some() {
        add(format!("{{project}}.{domain}"), false);
        if vars.branch.is_some() {
            add(format!("{{branch}}-{{project}}.{domain}"), false);
        }
        if vars.user.is_some() {
            add(format!("{{user}}-{{project}}.{domain}"), false);
        }
    } else if vars.user.is_some() {
        add(format!("{{user}}.{domain}"), false);
    }
    out
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn vars() -> Vars {
        Vars {
            branch: Some("login-fix".into()),
            user: Some("krishna".into()),
            project: Some("shop".into()),
        }
    }

    #[test]
    fn fills_in_placeholders_and_checks_the_result() {
        assert_eq!(
            expand_with("{branch}.dev.example.com", &vars()).unwrap(),
            "login-fix.dev.example.com"
        );
        assert_eq!(
            expand_with("{Project}-{USER}.Example.com", &vars()).unwrap(),
            "shop-krishna.example.com"
        );
        assert_eq!(
            expand_with("demo.example.com", &vars()).unwrap(),
            "demo.example.com"
        );
        assert_eq!(
            expand_with("{team}.example.com", &vars()),
            Err(NameError::Placeholder("{team}".into()))
        );
        assert_eq!(
            expand_with("{branch.example.com", &vars()),
            Err(NameError::Placeholder("{".into()))
        );
        let no_git = Vars {
            branch: None,
            ..vars()
        };
        assert_eq!(
            expand_with("{branch}.example.com", &no_git),
            Err(NameError::NoValue("{branch}".into()))
        );
        assert!(matches!(
            expand_with("*.example.com", &vars()),
            Err(NameError::Invalid(_))
        ));
        assert!(matches!(
            expand_with("{branch}", &vars()),
            Err(NameError::Invalid(_))
        ));
    }

    #[test]
    fn reads_the_branch_and_project_of_a_folder() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("Shop Front");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(
            repo.join(".git/HEAD"),
            "ref: refs/heads/feature/Login_Fix\n",
        )
        .unwrap();
        std::fs::write(repo.join("package.json"), r#"{"name": "@acme/shop"}"#).unwrap();
        let expanded = expand("{branch}.{project}.example.com", &repo).unwrap();
        assert_eq!(expanded, "feature-login-fix.acme-shop.example.com");
        let plain = dir.path().join("Blog");
        std::fs::create_dir_all(&plain).unwrap();
        assert_eq!(
            expand("{project}.example.com", &plain).unwrap(),
            "blog.example.com"
        );
    }

    #[test]
    fn suggests_from_the_project() {
        let names: Vec<String> = suggestions("dev.example.com", &vars(), None)
            .into_iter()
            .map(|s| s.hostname)
            .collect();
        assert_eq!(
            names,
            [
                "shop.dev.example.com",
                "login-fix-shop.dev.example.com",
                "krishna-shop.dev.example.com"
            ]
        );
        let remembered = suggestions("example.com", &vars(), Some("{branch}.example.com"));
        assert!(remembered[0].remembered);
        assert_eq!(remembered[0].hostname, "login-fix.example.com");
        let nothing = Vars::default();
        assert!(suggestions("example.com", &nothing, None).is_empty());
    }

    #[tokio::test]
    async fn remembers_per_folder() {
        let store = Store::open_in_memory().unwrap();
        let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        assert_eq!(remembered(&store, a.path()).await.unwrap(), None);
        remember(&store, a.path(), "{Branch}.example.com")
            .await
            .unwrap();
        remember(&store, b.path(), "b.example.com").await.unwrap();
        assert_eq!(
            remembered(&store, a.path()).await.unwrap().as_deref(),
            Some("{branch}.example.com")
        );
        assert_eq!(
            remembered(&store, b.path()).await.unwrap().as_deref(),
            Some("b.example.com")
        );
    }

    proptest! {
        /// Any branch, user or project name becomes a hostname: placeholders only ever
        /// produce DNS labels.
        #[test]
        fn any_values_make_valid_hostnames(
            branch in "\\PC{1,80}",
            user in "\\PC{1,40}",
            project in "\\PC{1,60}",
        ) {
            let vars = Vars {
                branch: Some(template::label(&branch)).filter(|b| !b.is_empty()),
                user: Some(template::label(&user)).filter(|u| !u.is_empty()),
                project: Some(template::label(&project)).filter(|p| !p.is_empty()),
            };
            for suggestion in suggestions("dev.example.com", &vars, None) {
                prop_assert!(Hostname::parse(&suggestion.hostname).is_ok());
                prop_assert!(suggestion.hostname.split('.').all(|l| l.len() <= 63 && !l.is_empty()));
            }
        }
    }
}
