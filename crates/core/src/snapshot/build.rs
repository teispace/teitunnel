//! Build then publish: recognise a web project (its framework, package manager, build
//! script and output folder) and run its build through a typed command, never a shell:
//! `<manager> run <script>` as discrete arguments, in the project folder.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};

use super::SnapshotError;

/// How long a build may take.
pub const BUILD_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// Output lines kept to explain a failed build.
const TAIL: usize = 40;

/// Web frameworks Teitunnel knows the static output of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Framework {
    /// Vite (React, Vue, Svelte, Solid…): `dist`.
    Vite,
    /// Next.js with `output: 'export'`: `out`.
    Next,
    /// Astro: `dist`.
    Astro,
    /// SvelteKit with the static adapter: `build`.
    SvelteKit,
    /// Nuxt `generate`: `.output/public`.
    Nuxt,
    /// Create React App: `build`.
    CreateReactApp,
    /// Gatsby: `public`.
    Gatsby,
    /// Docusaurus: `build`.
    Docusaurus,
    /// VitePress: `docs/.vitepress/dist` (or `.vitepress/dist`).
    VitePress,
    /// Angular: `dist/<project>/browser`.
    Angular,
    /// A project with a build script Teitunnel doesn't recognise: `dist`.
    Other,
    /// Plain HTML: the folder itself, no build.
    Static,
}

/// Which package manager runs the build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum PackageManager {
    /// npm.
    Npm,
    /// pnpm.
    Pnpm,
    /// Yarn.
    Yarn,
    /// Bun.
    Bun,
}

impl PackageManager {
    /// The program name.
    pub fn program(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }
}

/// A recognised project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// The project folder.
    pub dir: String,
    /// Its name (`package.json`, else the folder's).
    pub name: String,
    /// The framework.
    pub framework: Framework,
    /// Package manager (none for plain HTML).
    pub manager: Option<PackageManager>,
    /// The script to run, e.g. `build` or `generate` (none for plain HTML).
    pub script: Option<String>,
    /// The folder the build writes the site to (the project folder for plain HTML).
    pub output: String,
    /// Something to fix first, e.g. Next.js without `output: 'export'`.
    pub warning: Option<ProjectWarning>,
}

/// A problem with the project's setup the user can fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ProjectWarning {
    /// Next.js builds a server app unless `output: 'export'` is set.
    NextNeedsExport,
    /// SvelteKit needs `@sveltejs/adapter-static` to produce files.
    SvelteKitNeedsStaticAdapter,
}

#[derive(Debug, Default, Deserialize)]
struct PackageJson {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    scripts: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    dependencies: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "devDependencies")]
    dev_dependencies: std::collections::BTreeMap<String, serde_json::Value>,
    #[serde(default, rename = "packageManager")]
    package_manager: Option<String>,
}

impl PackageJson {
    fn has(&self, dependency: &str) -> bool {
        self.dependencies.contains_key(dependency) || self.dev_dependencies.contains_key(dependency)
    }
}

fn manager(dir: &Path, package: &PackageJson) -> PackageManager {
    if let Some(declared) = &package.package_manager {
        for manager in [
            PackageManager::Pnpm,
            PackageManager::Yarn,
            PackageManager::Bun,
            PackageManager::Npm,
        ] {
            if declared.starts_with(&format!("{}@", manager.program())) {
                return manager;
            }
        }
    }
    let lock = |name: &str| dir.join(name).is_file();
    if lock("pnpm-lock.yaml") {
        PackageManager::Pnpm
    } else if lock("yarn.lock") {
        PackageManager::Yarn
    } else if lock("bun.lockb") || lock("bun.lock") {
        PackageManager::Bun
    } else {
        PackageManager::Npm
    }
}

fn read_any(dir: &Path, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| std::fs::read_to_string(dir.join(name)).ok())
}

fn folder_name(dir: &Path) -> String {
    dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("site")
        .to_owned()
}

/// Recognises the project in `dir` (blocking: reads a few files).
///
/// # Errors
/// [`SnapshotError::NoProject`] when there's neither a `package.json` with a build
/// script nor an `index.html`.
pub fn detect(dir: &Path) -> Result<Project, SnapshotError> {
    let display = dir.display().to_string();
    let package: Option<PackageJson> = std::fs::read_to_string(dir.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let Some(package) = package else {
        if dir.join("index.html").is_file() {
            return Ok(Project {
                dir: display.clone(),
                name: folder_name(dir),
                framework: Framework::Static,
                manager: None,
                script: None,
                output: display,
                warning: None,
            });
        }
        return Err(SnapshotError::NoProject(display));
    };
    let mut warning = None;
    let (framework, output, preferred) = if package.has("next") {
        let config = read_any(
            dir,
            &[
                "next.config.js",
                "next.config.mjs",
                "next.config.ts",
                "next.config.cjs",
            ],
        )
        .unwrap_or_default();
        let exports = config.contains("output") && config.contains("export");
        if !exports {
            warning = Some(ProjectWarning::NextNeedsExport);
        }
        (Framework::Next, "out".to_owned(), "build")
    } else if package.has("nuxt") {
        (Framework::Nuxt, ".output/public".to_owned(), "generate")
    } else if package.has("@sveltejs/kit") {
        if !package.has("@sveltejs/adapter-static") {
            warning = Some(ProjectWarning::SvelteKitNeedsStaticAdapter);
        }
        (Framework::SvelteKit, "build".to_owned(), "build")
    } else if package.has("astro") {
        (Framework::Astro, "dist".to_owned(), "build")
    } else if package.has("gatsby") {
        (Framework::Gatsby, "public".to_owned(), "build")
    } else if package.has("@docusaurus/core") {
        (Framework::Docusaurus, "build".to_owned(), "build")
    } else if package.has("vitepress") {
        let output = if dir.join("docs/.vitepress").is_dir() {
            "docs/.vitepress/dist"
        } else {
            ".vitepress/dist"
        };
        (Framework::VitePress, output.to_owned(), "docs:build")
    } else if package.has("@angular/core") {
        let name = package.name.clone().unwrap_or_else(|| folder_name(dir));
        (Framework::Angular, format!("dist/{name}/browser"), "build")
    } else if package.has("react-scripts") {
        (Framework::CreateReactApp, "build".to_owned(), "build")
    } else if package.has("vite") {
        (Framework::Vite, "dist".to_owned(), "build")
    } else {
        (Framework::Other, "dist".to_owned(), "build")
    };
    // The preferred script, else `build`; a project with neither can't be built.
    let script = [preferred, "build"]
        .into_iter()
        .find(|s| package.scripts.contains_key(*s))
        .map(str::to_owned);
    let Some(script) = script else {
        if dir.join("index.html").is_file() && framework == Framework::Other {
            return Ok(Project {
                dir: display.clone(),
                name: package.name.unwrap_or_else(|| folder_name(dir)),
                framework: Framework::Static,
                manager: None,
                script: None,
                output: display,
                warning: None,
            });
        }
        return Err(SnapshotError::NoProject(display));
    };
    Ok(Project {
        name: package.name.clone().unwrap_or_else(|| folder_name(dir)),
        manager: Some(manager(dir, &package)),
        script: Some(script),
        output: dir.join(output).display().to_string(),
        dir: display,
        framework,
        warning,
    })
}

/// A build as a process launch: `<manager> run <script>` in the project folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildCommand {
    program: String,
    args: Vec<String>,
    dir: PathBuf,
}

/// Script names are plain words (`build`, `docs:build`): nothing a package manager could
/// read as an option.
fn valid_script(script: &str) -> bool {
    !script.is_empty()
        && !script.starts_with('-')
        && script
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_:.".contains(&b))
}

impl BuildCommand {
    /// The build of `project`, or `None` for plain HTML.
    ///
    /// # Errors
    /// A script name that isn't a plain word.
    pub fn for_project(project: &Project) -> Result<Option<Self>, SnapshotError> {
        let (Some(manager), Some(script)) = (project.manager, &project.script) else {
            return Ok(None);
        };
        if !valid_script(script) {
            return Err(SnapshotError::NoProject(project.dir.clone()));
        }
        // On Windows the managers are `.cmd` shims, which `CreateProcess` doesn't find
        // by their bare name.
        let program = if cfg!(windows) && manager != PackageManager::Bun {
            format!("{}.cmd", manager.program())
        } else {
            manager.program().to_owned()
        };
        Ok(Some(Self {
            program,
            args: vec!["run".to_owned(), script.clone()],
            dir: PathBuf::from(&project.dir),
        }))
    }

    /// The program.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// The arguments.
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// How it's shown to people, e.g. `pnpm run build`.
    pub fn display(&self) -> String {
        std::iter::once(self.program.trim_end_matches(".cmd"))
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn to_command(&self) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.program);
        command
            .args(&self.args)
            .current_dir(&self.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            // Build tools print plain progress instead of redrawing a terminal.
            .env("CI", "1")
            .env("FORCE_COLOR", "0");
        command
    }

    /// Runs the build, passing each output line to `line`. Fails with the last lines of
    /// output when it exits unsuccessfully or takes longer than [`BUILD_TIMEOUT`].
    ///
    /// # Errors
    /// The build couldn't start, failed, or timed out.
    pub async fn run(&self, mut line: impl FnMut(&str) + Send) -> Result<(), SnapshotError> {
        let mut child = self
            .to_command()
            .spawn()
            .map_err(|e| SnapshotError::Build {
                command: self.display(),
                output: e.to_string(),
            })?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        for stream in [
            stdout.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
            stderr.map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Send + Unpin>),
        ]
        .into_iter()
        .flatten()
        {
            let tx = tx.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stream).lines();
                while let Ok(Some(text)) = lines.next_line().await {
                    if tx.send(text).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut tail: VecDeque<String> = VecDeque::with_capacity(TAIL);
        let collect = async {
            while let Some(text) = rx.recv().await {
                line(&text);
                if tail.len() == TAIL {
                    tail.pop_front();
                }
                tail.push_back(text);
            }
            child.wait().await
        };
        let status = match tokio::time::timeout(BUILD_TIMEOUT, collect).await {
            Ok(status) => status.map_err(|e| SnapshotError::Build {
                command: self.display(),
                output: e.to_string(),
            })?,
            Err(_) => {
                return Err(SnapshotError::BuildTimeout(self.display()));
            }
        };
        if status.success() {
            Ok(())
        } else {
            Err(SnapshotError::Build {
                command: self.display(),
                output: tail.into_iter().collect::<Vec<_>>().join("\n"),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (path, content) in files {
            let full = dir.path().join(path);
            std::fs::create_dir_all(full.parent().unwrap()).unwrap();
            std::fs::write(full, content).unwrap();
        }
        dir
    }

    fn detected(files: &[(&str, &str)]) -> (Project, tempfile::TempDir) {
        let dir = project(files);
        (detect(dir.path()).unwrap(), dir)
    }

    fn output_of(project: &Project) -> String {
        Path::new(&project.output)
            .strip_prefix(&project.dir)
            .unwrap()
            .display()
            .to_string()
    }

    #[test]
    fn recognises_frameworks_and_their_output() {
        type Case<'a> = (
            &'a str,
            &'a [(&'a str, &'a str)],
            Framework,
            &'a str,
            &'a str,
        );
        let cases: &[Case<'_>] = &[
            (
                "vite",
                &[(
                    "package.json",
                    r#"{"scripts":{"build":"vite build"},"devDependencies":{"vite":"7"}}"#,
                )],
                Framework::Vite,
                "dist",
                "build",
            ),
            (
                "next",
                &[
                    (
                        "package.json",
                        r#"{"scripts":{"build":"next build"},"dependencies":{"next":"16"}}"#,
                    ),
                    ("next.config.mjs", "export default { output: 'export' }"),
                ],
                Framework::Next,
                "out",
                "build",
            ),
            (
                "astro",
                &[(
                    "package.json",
                    r#"{"scripts":{"build":"astro build"},"dependencies":{"astro":"5"}}"#,
                )],
                Framework::Astro,
                "dist",
                "build",
            ),
            (
                "sveltekit",
                &[(
                    "package.json",
                    r#"{"scripts":{"build":"vite build"},"devDependencies":{"@sveltejs/kit":"2","@sveltejs/adapter-static":"3","vite":"7"}}"#,
                )],
                Framework::SvelteKit,
                "build",
                "build",
            ),
            (
                "nuxt",
                &[(
                    "package.json",
                    r#"{"scripts":{"build":"nuxt build","generate":"nuxt generate"},"dependencies":{"nuxt":"4"}}"#,
                )],
                Framework::Nuxt,
                ".output/public",
                "generate",
            ),
            (
                "cra",
                &[(
                    "package.json",
                    r#"{"scripts":{"build":"react-scripts build"},"dependencies":{"react-scripts":"5"}}"#,
                )],
                Framework::CreateReactApp,
                "build",
                "build",
            ),
        ];
        for (label, files, framework, output, script) in cases {
            let (found, _dir) = detected(files);
            assert_eq!(found.framework, *framework, "{label}");
            assert_eq!(output_of(&found), *output, "{label}");
            assert_eq!(found.script.as_deref(), Some(*script), "{label}");
            assert_eq!(found.warning, None, "{label}");
        }
    }

    #[test]
    fn picks_the_package_manager() {
        let json = r#"{"scripts":{"build":"vite build"},"devDependencies":{"vite":"7"}}"#;
        for (lock, manager) in [
            ("pnpm-lock.yaml", PackageManager::Pnpm),
            ("yarn.lock", PackageManager::Yarn),
            ("bun.lock", PackageManager::Bun),
            ("package-lock.json", PackageManager::Npm),
        ] {
            let (found, _dir) = detected(&[("package.json", json), (lock, "")]);
            assert_eq!(found.manager, Some(manager), "{lock}");
        }
        let declared = r#"{"packageManager":"yarn@4.5.0","scripts":{"build":"x"}}"#;
        let (found, _dir) = detected(&[("package.json", declared), ("package-lock.json", "")]);
        assert_eq!(found.manager, Some(PackageManager::Yarn));
    }

    #[test]
    fn warns_about_setups_that_build_no_files() {
        let (next, _a) = detected(&[(
            "package.json",
            r#"{"scripts":{"build":"next build"},"dependencies":{"next":"16"}}"#,
        )]);
        assert_eq!(next.warning, Some(ProjectWarning::NextNeedsExport));
        let (kit, _b) = detected(&[(
            "package.json",
            r#"{"scripts":{"build":"vite build"},"devDependencies":{"@sveltejs/kit":"2"}}"#,
        )]);
        assert_eq!(
            kit.warning,
            Some(ProjectWarning::SvelteKitNeedsStaticAdapter)
        );
    }

    #[test]
    fn plain_html_needs_no_build_and_other_folders_are_refused() {
        let (site, dir) = detected(&[("index.html", "<h1>hi</h1>")]);
        assert_eq!(site.framework, Framework::Static);
        assert_eq!(site.output, dir.path().display().to_string());
        assert_eq!(BuildCommand::for_project(&site).unwrap(), None);
        let nothing = project(&[("notes.txt", "x")]);
        assert!(matches!(
            detect(nothing.path()),
            Err(SnapshotError::NoProject(_))
        ));
        let no_script = project(&[("package.json", r#"{"scripts":{"dev":"vite"}}"#)]);
        assert!(matches!(
            detect(no_script.path()),
            Err(SnapshotError::NoProject(_))
        ));
    }

    #[test]
    fn builds_with_discrete_arguments() {
        let (found, _dir) = detected(&[
            (
                "package.json",
                r#"{"scripts":{"build":"vite build"},"devDependencies":{"vite":"7"}}"#,
            ),
            ("pnpm-lock.yaml", ""),
        ]);
        let command = BuildCommand::for_project(&found).unwrap().unwrap();
        assert_eq!(command.args(), ["run", "build"]);
        assert_eq!(command.display(), "pnpm run build");
        let mut hostile = found.clone();
        hostile.script = Some("build; rm -rf ~".into());
        assert!(BuildCommand::for_project(&hostile).is_err());
        hostile.script = Some("--prefix=/".into());
        assert!(BuildCommand::for_project(&hostile).is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reports_the_output_of_a_failed_build() {
        let dir = tempfile::tempdir().unwrap();
        let command = BuildCommand {
            program: "sh".into(),
            args: vec!["-c".into(), "echo compiling; echo boom >&2; exit 3".into()],
            dir: dir.path().to_path_buf(),
        };
        let mut seen = Vec::new();
        let err = command.run(|l| seen.push(l.to_owned())).await.unwrap_err();
        seen.sort();
        assert_eq!(seen, ["boom", "compiling"]);
        assert!(matches!(err, SnapshotError::Build { output, .. } if output.contains("boom")));
        let ok = BuildCommand {
            program: "true".into(),
            args: Vec::new(),
            dir: dir.path().to_path_buf(),
        };
        ok.run(|_| {}).await.unwrap();
    }
}
