//! Hostname templates: `{branch}`, `{user}` and `{project}` in a project file's hostnames,
//! so each person and branch of a shared repository gets its own address
//! (`{branch}-{project}.example.com` → `login-fix-shop.example.com`).

use std::path::Path;

/// The placeholders a hostname may use.
pub const PLACEHOLDERS: &[&str] = &["branch", "user", "project"];

/// Values for the placeholders, already made into DNS labels.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Vars {
    /// The git branch (or the short commit when detached), if the folder is in a repository.
    pub branch: Option<String>,
    /// The user's login name.
    pub user: Option<String>,
    /// The project's name.
    pub project: Option<String>,
}

impl Vars {
    /// The values for a project in `dir` named `project`.
    pub fn for_dir(dir: &Path, project: &str) -> Self {
        Self {
            branch: git_branch(dir).map(|b| label(&b)).filter(|b| !b.is_empty()),
            user: std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .ok()
                .map(|u| label(&u))
                .filter(|u| !u.is_empty()),
            project: Some(label(project)).filter(|p| !p.is_empty()),
        }
    }

    fn get(&self, name: &str) -> Option<&str> {
        match name {
            "branch" => self.branch.as_deref(),
            "user" => self.user.as_deref(),
            "project" => self.project.as_deref(),
            _ => None,
        }
    }
}

/// Why a template couldn't be expanded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    /// `{something}` that isn't a placeholder.
    Unknown(String),
    /// A `{` without its `}`.
    Unclosed,
    /// The placeholder has no value here (e.g. `{branch}` outside a git repository).
    NoValue(String),
}

/// The placeholders `template` uses, in order.
///
/// # Errors
/// An unknown or unclosed placeholder.
pub fn placeholders(template: &str) -> Result<Vec<String>, TemplateError> {
    let mut found = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        let after = &rest[start + 1..];
        let end = after.find('}').ok_or(TemplateError::Unclosed)?;
        let name = after[..end].trim().to_ascii_lowercase();
        if !PLACEHOLDERS.contains(&name.as_str()) {
            return Err(TemplateError::Unknown(after[..end].to_owned()));
        }
        found.push(name);
        rest = &after[end + 1..];
    }
    Ok(found)
}

/// Fills in `template`'s placeholders.
///
/// # Errors
/// See [`TemplateError`].
pub fn expand(template: &str, vars: &Vars) -> Result<String, TemplateError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find('{') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        let end = after.find('}').ok_or(TemplateError::Unclosed)?;
        let name = after[..end].trim().to_ascii_lowercase();
        if !PLACEHOLDERS.contains(&name.as_str()) {
            return Err(TemplateError::Unknown(after[..end].to_owned()));
        }
        out.push_str(vars.get(&name).ok_or(TemplateError::NoValue(name))?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out.to_ascii_lowercase())
}

/// Makes `text` a DNS label: lower-case letters, digits and single hyphens, at most 40
/// characters (so a label built from several still fits 63).
pub fn label(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out: String = out.chars().take(40).collect();
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// The branch checked out in the repository `dir` is in: read from `.git/HEAD` (no git
/// process is started). Worktrees (`.git` files pointing elsewhere) are followed. A
/// detached head gives the commit's first 7 characters.
pub fn git_branch(dir: &Path) -> Option<String> {
    let mut current = Some(dir);
    while let Some(folder) = current {
        let dot_git = folder.join(".git");
        let git_dir = if dot_git.is_dir() {
            Some(dot_git)
        } else if dot_git.is_file() {
            let text = std::fs::read_to_string(&dot_git).ok()?;
            let target = text.trim().strip_prefix("gitdir:")?.trim().to_owned();
            let target = Path::new(&target);
            Some(if target.is_absolute() {
                target.to_path_buf()
            } else {
                folder.join(target)
            })
        } else {
            None
        };
        if let Some(git_dir) = git_dir {
            let head = std::fs::read_to_string(git_dir.join("HEAD")).ok()?;
            let head = head.trim();
            return match head.strip_prefix("ref:") {
                Some(reference) => reference
                    .trim()
                    .strip_prefix("refs/heads/")
                    .map(str::to_owned),
                None => (head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit()))
                    .then(|| head[..7].to_owned()),
            };
        }
        current = folder.parent();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> Vars {
        Vars {
            branch: Some("feat-login".into()),
            user: Some("ada".into()),
            project: Some("shop".into()),
        }
    }

    #[test]
    fn expands_placeholders() {
        assert_eq!(
            expand("{branch}-{project}.example.com", &vars()).unwrap(),
            "feat-login-shop.example.com"
        );
        assert_eq!(
            expand("{ User }.dev.Example.com", &vars()).unwrap(),
            "ada.dev.example.com"
        );
        assert_eq!(
            expand("plain.example.com", &vars()).unwrap(),
            "plain.example.com"
        );
    }

    #[test]
    fn refuses_unknown_unclosed_and_missing() {
        assert_eq!(
            expand("{team}.example.com", &vars()),
            Err(TemplateError::Unknown("team".into()))
        );
        assert_eq!(
            expand("{branch.example.com", &vars()),
            Err(TemplateError::Unclosed)
        );
        let none = Vars::default();
        assert_eq!(
            expand("{branch}.example.com", &none),
            Err(TemplateError::NoValue("branch".into()))
        );
        assert_eq!(
            placeholders("{branch}-{user}.x.io").unwrap(),
            ["branch", "user"]
        );
    }

    #[test]
    fn makes_labels() {
        assert_eq!(label("feature/Login_Fix"), "feature-login-fix");
        assert_eq!(label("--weird..name--"), "weird-name");
        assert_eq!(label("ünïcode"), "n-code");
        assert_eq!(label(&"a".repeat(80)).len(), 40);
        assert_eq!(label("///"), "");
    }

    #[test]
    fn reads_the_branch_without_git() {
        let dir = tempfile::tempdir().unwrap();
        let git = dir.path().join(".git");
        std::fs::create_dir_all(&git).unwrap();
        std::fs::write(git.join("HEAD"), "ref: refs/heads/feature/pay\n").unwrap();
        let nested = dir.path().join("apps/web");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(git_branch(&nested).as_deref(), Some("feature/pay"));
        std::fs::write(
            git.join("HEAD"),
            "0123456789abcdef0123456789abcdef01234567\n",
        )
        .unwrap();
        assert_eq!(git_branch(dir.path()).as_deref(), Some("0123456"));

        // A worktree: `.git` is a file naming the real git directory.
        let worktree = tempfile::tempdir().unwrap();
        let real = dir.path().join(".git/worktrees/wt");
        std::fs::create_dir_all(&real).unwrap();
        std::fs::write(real.join("HEAD"), "ref: refs/heads/wt-branch\n").unwrap();
        std::fs::write(
            worktree.path().join(".git"),
            format!("gitdir: {}\n", real.display()),
        )
        .unwrap();
        assert_eq!(git_branch(worktree.path()).as_deref(), Some("wt-branch"));
    }
}
