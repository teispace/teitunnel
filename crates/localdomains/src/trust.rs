//! Installing the local CA into trust stores, removing it, and reporting per-store status.
//!
//! | OS | Store | How | Rights |
//! |---|---|---|---|
//! | macOS | login keychain + user trust settings (SSL) | `security add-trusted-cert` | user; macOS asks for the password |
//! | Windows | `CurrentUser\Root` | `certutil -user -addstore Root` | user; Windows asks to confirm |
//! | Linux | system bundle | copy to the anchors dir + refresh | **admin**: returned as [`PrivilegedAction`]s |
//! | Linux | NSS (Chrome/Chromium, Firefox profiles) | NSS `certutil -A` | user |
//! | macOS/Windows | Firefox | follows the OS store (default since Firefox 120); `user.js` pref only with consent | user |
//!
//! Nothing here elevates. User-level steps run through the [`Runner`]; privileged steps come
//! back in the [`TrustReport`] for the app to show (copy button) or run via `pkexec` with the
//! user's consent.

pub mod firefox;
pub mod linux;
pub mod macos;
pub mod nss;
pub mod windows;

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{
    ca::LocalCa,
    platform::Platform,
    privileged::PrivilegedAction,
    process::{Invocation, Runner, find_program},
};

use self::{
    firefox::EnterpriseRoots,
    linux::{LinuxFlavor, SystemStore},
    nss::{NssApp, NssDb},
};

/// The facts about the CA certificate the installers need.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaCert {
    /// Subject CN, also the NSS nickname.
    pub common_name: String,
    /// SHA-1, uppercase hex.
    pub sha1: String,
    /// PEM text.
    pub pem: String,
    /// Where the PEM file is (written with [`LocalCa::write_cert_pem`]).
    pub path: PathBuf,
}

impl CaCert {
    /// From a loaded CA whose certificate was written to `path`.
    #[must_use]
    pub fn from_ca(ca: &LocalCa, path: PathBuf) -> Self {
        Self {
            common_name: ca.common_name().to_owned(),
            sha1: ca.sha1_fingerprint(),
            pem: ca.cert_pem().to_owned(),
            path,
        }
    }
}

/// The machine facts the installers use. [`TrustEnv::detect`] fills them in; tests build
/// them by hand.
#[derive(Debug, Clone)]
pub struct TrustEnv {
    /// The OS.
    pub platform: Platform,
    /// The user's home directory.
    pub home: PathBuf,
    /// `%APPDATA%` (Windows).
    pub app_data: Option<PathBuf>,
    /// `%SystemRoot%` (Windows).
    pub system_root: Option<PathBuf>,
    /// Filesystem root for store checks and the fixed tool directories (`/`).
    pub fs_root: PathBuf,
    /// `PATH`, for finding tools.
    pub path_var: Option<OsString>,
    /// A directory Teitunnel owns, for temporary files (e.g. the macOS trust export).
    pub work_dir: PathBuf,
}

impl TrustEnv {
    /// The current machine; `None` without a home directory.
    #[must_use]
    pub fn detect(work_dir: PathBuf) -> Option<Self> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)?;
        Some(Self {
            platform: Platform::current(),
            home,
            app_data: std::env::var_os("APPDATA").map(PathBuf::from),
            system_root: std::env::var_os("SystemRoot").map(PathBuf::from),
            fs_root: PathBuf::from("/"),
            path_var: std::env::var_os("PATH"),
            work_dir,
        })
    }

    /// `name` on `PATH`, else in one of the fixed `extra` directories (under `fs_root`).
    fn find(&self, name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
        let extra: Vec<PathBuf> = extra
            .iter()
            .map(|dir| self.fs_root.join(dir.strip_prefix("/").unwrap_or(dir)))
            .collect();
        let extra: Vec<&Path> = extra.iter().map(PathBuf::as_path).collect();
        find_program(name, self.path_var.as_deref(), &extra)
    }

    fn linux_store(&self) -> Option<SystemStore> {
        let sbin = [
            PathBuf::from("/usr/sbin"),
            PathBuf::from("/sbin"),
            PathBuf::from("/usr/bin"),
        ];
        linux::detect(&self.fs_root, &|name| self.find(name, &sbin))
    }

    fn nss_certutil(&self) -> Option<PathBuf> {
        if self.platform == Platform::Windows {
            // `certutil.exe` on Windows is Microsoft's tool, not NSS's.
            return None;
        }
        self.find("certutil", &nss::certutil_extra_dirs(self.platform))
    }

    fn firefox_profiles(&self) -> Vec<PathBuf> {
        let roots = nss::firefox_profile_roots(self.platform, &self.home, self.app_data.as_deref());
        nss::profiles_with(&roots, "prefs.js")
    }
}

/// A trust store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TrustStore {
    /// macOS login keychain with user-domain SSL trust.
    MacosLoginKeychain,
    /// Windows `CurrentUser\Root`.
    WindowsCurrentUserRoot,
    /// The Linux system bundle.
    LinuxSystem {
        /// The detected flavor, if any.
        flavor: Option<LinuxFlavor>,
    },
    /// An NSS database.
    Nss {
        /// Chrome/Chromium or Firefox.
        app: NssApp,
        /// The database directory.
        path: PathBuf,
    },
    /// A Firefox profile's "trust OS roots" setting (macOS, Windows).
    FirefoxEnterpriseRoots {
        /// The profile directory.
        profile: PathBuf,
    },
}

/// A store's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum StoreState {
    /// The CA is there and trusted.
    Trusted,
    /// The certificate is there but not trusted (macOS: in the keychain without trust
    /// settings).
    PresentNotTrusted,
    /// Not installed.
    Absent,
    /// Firefox: trusts what the OS trusts (the default since Firefox 120).
    FollowsSystem,
    /// Firefox: the user switched off trusting OS roots.
    Disabled,
    /// No supported trust store was found (Linux without a known tool).
    Unsupported,
    /// The tool needed isn't installed.
    ToolMissing {
        /// The program.
        tool: String,
        /// The package that provides it, when known.
        package: Option<String>,
    },
    /// A check or change failed.
    Error {
        /// What went wrong (technical, for logs and details).
        message: String,
    },
}

/// One store's status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreStatus {
    /// The store.
    pub store: TrustStore,
    /// Its state.
    pub state: StoreState,
}

/// What to include beyond the OS store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallOptions {
    /// Also add to NSS databases (always wanted on Linux; on macOS only if NSS `certutil`
    /// is installed).
    pub nss: bool,
    /// Write `security.enterprise_roots.enabled` into each Firefox profile's `user.js`
    /// (macOS/Windows). Only with the user's explicit consent.
    pub firefox_enterprise_roots: bool,
}

/// The outcome of an install or uninstall.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TrustReport {
    /// Every store's state afterwards.
    pub stores: Vec<StoreStatus>,
    /// Steps that still need administrator rights (Linux system store).
    pub privileged: Vec<PrivilegedAction>,
}

/// Installs, removes and checks the CA in this machine's trust stores.
#[derive(Debug)]
pub struct TrustManager<R> {
    env: TrustEnv,
    cert: CaCert,
    runner: R,
}

impl<R: Runner> TrustManager<R> {
    /// A manager for `cert` on `env`, running tools through `runner`.
    pub fn new(env: TrustEnv, cert: CaCert, runner: R) -> Self {
        Self { env, cert, runner }
    }

    async fn run(&self, inv: &Invocation) -> Result<crate::process::ProcessOutput, String> {
        self.runner
            .run(inv)
            .await
            .map_err(|e| format!("{}: {e}", inv.program().display()))
    }

    async fn run_ok(&self, inv: &Invocation) -> Result<(), String> {
        let out = self.run(inv).await?;
        if out.success {
            Ok(())
        } else {
            let detail = if out.stderr.trim().is_empty() {
                out.stdout
            } else {
                out.stderr
            };
            Err(format!(
                "{} exited with {:?}: {}",
                inv.program().display(),
                out.code,
                detail.trim()
            ))
        }
    }

    /// Every relevant store's state.
    pub async fn status(&self) -> Vec<StoreStatus> {
        let mut out = Vec::new();
        match self.env.platform {
            Platform::Macos => out.push(StoreStatus {
                store: TrustStore::MacosLoginKeychain,
                state: self.macos_state().await,
            }),
            Platform::Windows => out.push(StoreStatus {
                store: TrustStore::WindowsCurrentUserRoot,
                state: self.windows_state().await,
            }),
            Platform::Linux => out.push(self.linux_system_status().await),
        }
        out.extend(self.nss_status().await);
        out.extend(self.firefox_status());
        out
    }

    /// Installs the CA: runs every user-level step and returns the privileged ones.
    pub async fn install(&self, options: InstallOptions) -> TrustReport {
        let mut privileged = Vec::new();
        match self.env.platform {
            Platform::Macos => {
                if self.macos_state().await != StoreState::Trusted {
                    let kc = macos::login_keychain(&self.env.home);
                    log_failure(
                        self.run_ok(&macos::add_trusted_cert(&kc, &self.cert.path))
                            .await,
                    );
                }
            }
            Platform::Windows => {
                if self.windows_state().await != StoreState::Trusted {
                    let certutil = windows::certutil_path(self.env.system_root.as_deref());
                    log_failure(
                        self.run_ok(&windows::add_store(&certutil, &self.cert.path))
                            .await,
                    );
                }
            }
            Platform::Linux => {
                if let Some(store) = self.env.linux_store()
                    && self.linux_system_status().await.state != StoreState::Trusted
                {
                    privileged = store.install_actions(&self.cert.path);
                }
            }
        }
        let wants_nss = options.nss || self.env.platform == Platform::Linux;
        if wants_nss && let Some(certutil) = self.env.nss_certutil() {
            let dbs = nss::discover(
                self.env.platform,
                &self.env.home,
                self.env.app_data.as_deref(),
            );
            for db in dbs {
                if !self.nss_has(&certutil, &db).await {
                    let add =
                        nss::add(&certutil, &db.path, &self.cert.common_name, &self.cert.path);
                    log_failure(self.run_ok(&add).await);
                }
            }
        }
        if options.firefox_enterprise_roots && self.env.platform != Platform::Linux {
            for profile in self.env.firefox_profiles() {
                log_failure(firefox::enable(&profile).map_err(|e| e.to_string()));
            }
        }
        TrustReport {
            stores: self.status().await,
            privileged,
        }
    }

    /// Removes the CA from every store it was added to; returns privileged steps left.
    pub async fn uninstall(&self) -> TrustReport {
        let mut privileged = Vec::new();
        match self.env.platform {
            Platform::Macos => {
                if self.macos_state().await != StoreState::Absent {
                    let kc = macos::login_keychain(&self.env.home);
                    // Trust settings first; then the item. Either may already be gone.
                    let _ = self
                        .run_ok(&macos::remove_trusted_cert(&self.cert.path))
                        .await;
                    log_failure(
                        self.run_ok(&macos::delete_certificate(&self.cert.sha1, &kc))
                            .await,
                    );
                }
            }
            Platform::Windows => {
                if self.windows_state().await != StoreState::Absent {
                    let certutil = windows::certutil_path(self.env.system_root.as_deref());
                    log_failure(
                        self.run_ok(&windows::del_store(&certutil, &self.cert.sha1))
                            .await,
                    );
                }
            }
            Platform::Linux => {
                if let Some(store) = self.env.linux_store()
                    && self.linux_system_status().await.state != StoreState::Absent
                {
                    privileged = store.uninstall_actions(&self.cert.path);
                }
            }
        }
        if let Some(certutil) = self.env.nss_certutil() {
            for db in nss::discover(
                self.env.platform,
                &self.env.home,
                self.env.app_data.as_deref(),
            ) {
                if self.nss_has(&certutil, &db).await {
                    log_failure(
                        self.run_ok(&nss::delete(&certutil, &db.path, &self.cert.common_name))
                            .await,
                    );
                }
            }
        }
        if self.env.platform != Platform::Linux {
            for profile in self.env.firefox_profiles() {
                log_failure(firefox::disable(&profile).map_err(|e| e.to_string()));
            }
        }
        TrustReport {
            stores: self.status().await,
            privileged,
        }
    }

    async fn macos_state(&self) -> StoreState {
        let kc = macos::login_keychain(&self.env.home);
        let listing = match self
            .run(&macos::find_certificate(&self.cert.common_name, &kc))
            .await
        {
            Ok(out) => out,
            Err(message) => return StoreState::Error { message },
        };
        if !listing.success || !macos::listing_has_sha1(&listing.stdout, &self.cert.sha1) {
            return StoreState::Absent;
        }
        let export = self.env.work_dir.join("trust-settings-export.plist");
        if let Err(err) = std::fs::create_dir_all(&self.env.work_dir) {
            return StoreState::Error {
                message: err.to_string(),
            };
        }
        let exported = self.run(&macos::export_trust_settings(&export)).await;
        let bytes = std::fs::read(&export).unwrap_or_default();
        let _ = std::fs::remove_file(&export);
        match exported {
            // With no user trust settings at all, the export fails: nothing is trusted.
            Ok(out) if out.success && macos::trust_settings_have_sha1(&bytes, &self.cert.sha1) => {
                StoreState::Trusted
            }
            Ok(_) => StoreState::PresentNotTrusted,
            Err(message) => StoreState::Error { message },
        }
    }

    async fn windows_state(&self) -> StoreState {
        let certutil = windows::certutil_path(self.env.system_root.as_deref());
        match self
            .run(&windows::find_in_store(&certutil, &self.cert.sha1))
            .await
        {
            Ok(out) if out.success => StoreState::Trusted,
            Ok(_) => StoreState::Absent,
            Err(message) => StoreState::Error { message },
        }
    }

    async fn linux_system_status(&self) -> StoreStatus {
        let Some(store) = self.env.linux_store() else {
            return StoreStatus {
                store: TrustStore::LinuxSystem { flavor: None },
                state: StoreState::Unsupported,
            };
        };
        let state = match store.anchor_matches(&self.env.fs_root, &self.cert.pem) {
            Some(true) => StoreState::Trusted,
            Some(false) => StoreState::Absent,
            None => match self.run(&store.list_anchors()).await {
                Ok(out) if linux::anchors_list_has(&out.stdout, &self.cert.common_name) => {
                    StoreState::Trusted
                }
                Ok(_) => StoreState::Absent,
                Err(message) => StoreState::Error { message },
            },
        };
        StoreStatus {
            store: TrustStore::LinuxSystem {
                flavor: Some(store.flavor),
            },
            state,
        }
    }

    async fn nss_has(&self, certutil: &Path, db: &NssDb) -> bool {
        self.run(&nss::list(certutil, &db.path, &self.cert.common_name))
            .await
            .is_ok_and(|out| out.success)
    }

    async fn nss_status(&self) -> Vec<StoreStatus> {
        let dbs = nss::discover(
            self.env.platform,
            &self.env.home,
            self.env.app_data.as_deref(),
        );
        let certutil = self.env.nss_certutil();
        let mut out = Vec::new();
        for db in dbs {
            let state = match &certutil {
                Some(certutil) => {
                    if self.nss_has(certutil, &db).await {
                        StoreState::Trusted
                    } else {
                        StoreState::Absent
                    }
                }
                // Firefox on macOS/Windows follows the OS store; NSS is optional there.
                None if self.env.platform != Platform::Linux => continue,
                None => StoreState::ToolMissing {
                    tool: "certutil".into(),
                    package: self
                        .env
                        .linux_store()
                        .map(|s| s.flavor.certutil_package().to_owned()),
                },
            };
            out.push(StoreStatus {
                store: TrustStore::Nss {
                    app: db.app,
                    path: db.path,
                },
                state,
            });
        }
        out
    }

    fn firefox_status(&self) -> Vec<StoreStatus> {
        if self.env.platform == Platform::Linux {
            return Vec::new();
        }
        self.env
            .firefox_profiles()
            .into_iter()
            .map(|profile| StoreStatus {
                state: match firefox::state(&profile) {
                    EnterpriseRoots::SetByTeitunnel => StoreState::Trusted,
                    EnterpriseRoots::Default => StoreState::FollowsSystem,
                    EnterpriseRoots::Disabled => StoreState::Disabled,
                },
                store: TrustStore::FirefoxEnterpriseRoots { profile },
            })
            .collect()
    }
}

fn log_failure(result: Result<(), String>) {
    if let Err(message) = result {
        tracing::warn!(%message, "trust store change failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::tests::FakeRunner;

    fn cert() -> CaCert {
        CaCert {
            common_name: "Teitunnel Local CA (a@b)".into(),
            sha1: "AB12".into(),
            pem: "-----BEGIN CERTIFICATE-----\nX\n-----END CERTIFICATE-----\n".into(),
            path: "/data/ca.pem".into(),
        }
    }

    fn env(
        platform: Platform,
        home: &Path,
        fs_root: &Path,
        path_var: Option<OsString>,
    ) -> TrustEnv {
        TrustEnv {
            platform,
            home: home.to_path_buf(),
            app_data: None,
            system_root: Some(PathBuf::from(r"C:\Windows")),
            fs_root: fs_root.to_path_buf(),
            path_var,
            work_dir: home.join("work"),
        }
    }

    #[tokio::test]
    async fn macos_install_adds_trusted_cert_when_absent() {
        let home = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        // status before install: find-certificate finds nothing.
        runner.reply(false, "");
        let manager = TrustManager::new(
            env(Platform::Macos, home.path(), Path::new("/"), None),
            cert(),
            runner,
        );
        let report = manager.install(InstallOptions::default()).await;
        let calls = manager.runner.calls();
        assert_eq!(calls[0][1], "find-certificate");
        assert_eq!(
            calls[1][1..6],
            ["add-trusted-cert", "-r", "trustRoot", "-p", "ssl"]
        );
        assert!(report.privileged.is_empty());
    }

    #[tokio::test]
    async fn macos_status_trusted_when_listed_and_in_trust_settings() {
        let home = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        runner.reply(true, "SHA-1 hash: AB12\n");
        let manager = TrustManager::new(
            env(Platform::Macos, home.path(), Path::new("/"), None),
            cert(),
            runner,
        );
        // The fake doesn't write the export file, so the CA is present but not trusted.
        let status = manager.status().await;
        assert_eq!(status[0].state, StoreState::PresentNotTrusted);
    }

    #[tokio::test]
    async fn macos_uninstall_removes_trust_then_item() {
        let home = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        runner.reply(true, "SHA-1 hash: AB12\n");
        let manager = TrustManager::new(
            env(Platform::Macos, home.path(), Path::new("/"), None),
            cert(),
            runner,
        );
        manager.uninstall().await;
        let calls = manager.runner.calls();
        let verbs: Vec<&str> = calls.iter().map(|c| c[1].as_str()).collect();
        assert_eq!(
            verbs[..4],
            [
                "find-certificate",
                "trust-settings-export",
                "remove-trusted-cert",
                "delete-certificate"
            ]
        );
        assert_eq!(calls[3][3], "AB12");
    }

    #[tokio::test]
    async fn windows_install_and_uninstall() {
        let home = tempfile::tempdir().unwrap();
        let runner = FakeRunner::default();
        runner.reply(false, "");
        let manager = TrustManager::new(
            env(Platform::Windows, home.path(), Path::new("/"), None),
            cert(),
            runner,
        );
        manager.install(InstallOptions::default()).await;
        let calls = manager.runner.calls();
        assert_eq!(calls[0][1..], ["-user", "-store", "Root", "AB12"]);
        assert_eq!(
            calls[1][1..],
            ["-user", "-addstore", "Root", "/data/ca.pem"]
        );
        let status = manager.status().await;
        assert_eq!(status[0].state, StoreState::Trusted);
        manager.uninstall().await;
        let calls = manager.runner.calls();
        assert!(
            calls
                .iter()
                .any(|c| c[1..] == ["-user", "-delstore", "Root", "AB12"])
        );
    }

    #[tokio::test]
    async fn linux_returns_privileged_steps_and_uses_nss() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("usr/local/share/ca-certificates")).unwrap();
        for tool in ["update-ca-certificates", "certutil"] {
            std::fs::write(bin.path().join(tool), "").unwrap();
        }
        std::fs::create_dir_all(home.path().join(".pki/nssdb")).unwrap();
        std::fs::write(home.path().join(".pki/nssdb/cert9.db"), "").unwrap();
        let path_var = std::env::join_paths([bin.path()]).unwrap();
        let runner = FakeRunner::default();
        runner.reply(false, ""); // certutil -L: not there yet
        let manager = TrustManager::new(
            env(Platform::Linux, home.path(), root.path(), Some(path_var)),
            cert(),
            runner,
        );
        let report = manager.install(InstallOptions::default()).await;
        assert_eq!(report.privileged.len(), 2);
        assert!(
            matches!(&report.privileged[0], PrivilegedAction::CopyFile { to, .. } if to.ends_with("teitunnel-local-ca.crt"))
        );
        let calls = manager.runner.calls();
        assert_eq!(calls[0][1], "-L");
        assert_eq!(calls[1][1], "-A");
        assert!(calls[1][3].starts_with("sql:"));
        // System store isn't trusted until the privileged steps run; NSS now is.
        assert_eq!(report.stores[0].state, StoreState::Absent);
        assert_eq!(report.stores[1].state, StoreState::Trusted);
    }

    #[tokio::test]
    async fn linux_without_certutil_reports_package() {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let bin = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("etc/pki/ca-trust/source/anchors")).unwrap();
        std::fs::write(bin.path().join("update-ca-trust"), "").unwrap();
        std::fs::create_dir_all(home.path().join(".mozilla/firefox/p.default")).unwrap();
        std::fs::write(home.path().join(".mozilla/firefox/p.default/cert9.db"), "").unwrap();
        let path_var = std::env::join_paths([bin.path()]).unwrap();
        let manager = TrustManager::new(
            env(Platform::Linux, home.path(), root.path(), Some(path_var)),
            cert(),
            FakeRunner::default(),
        );
        let status = manager.status().await;
        assert_eq!(
            status[0].store,
            TrustStore::LinuxSystem {
                flavor: Some(LinuxFlavor::Fedora)
            }
        );
        assert_eq!(
            status[1].state,
            StoreState::ToolMissing {
                tool: "certutil".into(),
                package: Some("nss-tools".into())
            }
        );
    }

    #[tokio::test]
    async fn firefox_enterprise_roots_only_with_consent() {
        let home = tempfile::tempdir().unwrap();
        let profile = home
            .path()
            .join("Library/Application Support/Firefox/Profiles/x.default");
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::write(profile.join("prefs.js"), "").unwrap();
        let manager = TrustManager::new(
            env(Platform::Macos, home.path(), Path::new("/"), None),
            cert(),
            FakeRunner::default(),
        );
        let report = manager.install(InstallOptions::default()).await;
        assert!(!profile.join("user.js").exists());
        assert_eq!(report.stores[1].state, StoreState::FollowsSystem);
        let report = manager
            .install(InstallOptions {
                firefox_enterprise_roots: true,
                ..InstallOptions::default()
            })
            .await;
        assert_eq!(report.stores[1].state, StoreState::Trusted);
        manager.uninstall().await;
        assert!(!profile.join("user.js").exists());
    }

    #[test]
    fn status_serializes_for_the_ui() {
        let status = StoreStatus {
            store: TrustStore::Nss {
                app: NssApp::Firefox,
                path: "/p".into(),
            },
            state: StoreState::ToolMissing {
                tool: "certutil".into(),
                package: Some("libnss3-tools".into()),
            },
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({
                "store": { "kind": "nss", "app": "firefox", "path": "/p" },
                "state": { "state": "toolMissing", "tool": "certutil", "package": "libnss3-tools" }
            })
        );
    }

    /// Changes the real trust store of the machine running it. Run by hand only:
    /// `cargo nextest run -p teitunnel-localdomains --run-ignored only real_store`.
    #[tokio::test]
    #[ignore = "modifies the real OS trust store and may show system dialogs"]
    async fn real_store_round_trip() {
        use crate::{CaIdentity, SystemClock, process::SystemRunner};
        let dir = tempfile::tempdir().unwrap();
        let (ca, _) = LocalCa::generate(&CaIdentity::current(), &SystemClock).unwrap();
        let path = dir.path().join("ca.pem");
        ca.write_cert_pem(&path).unwrap();
        let manager = TrustManager::new(
            TrustEnv::detect(dir.path().join("work")).unwrap(),
            CaCert::from_ca(&ca, path),
            SystemRunner,
        );
        let installed = manager.install(InstallOptions::default()).await;
        println!("{installed:#?}");
        let removed = manager.uninstall().await;
        println!("{removed:#?}");
    }
}
