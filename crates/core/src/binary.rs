//! The cloudflared binary Teitunnel runs: located once, cached, re-checked on demand,
//! and installed (verified) into the app data directory when the user asks.

use std::sync::Arc;

use cloudflared::install::Installer;
pub use cloudflared::{BinaryStatus, Locator, install::Progress as InstallStep};
use tokio::sync::{Mutex, RwLock};

/// Caches the located binary so every Quick Share doesn't re-run `--version`.
#[derive(Debug, Clone)]
pub struct BinaryManager {
    locator: Locator,
    current: Arc<RwLock<Option<BinaryStatus>>>,
    installing: Arc<Mutex<()>>,
}

impl BinaryManager {
    /// A manager using `locator`.
    pub fn new(locator: Locator) -> Self {
        Self {
            locator,
            current: Arc::default(),
            installing: Arc::default(),
        }
    }

    /// The binary to use, locating it on first use.
    ///
    /// # Errors
    /// [`cloudflared::Error::NotFound`] if cloudflared isn't installed.
    pub async fn current(&self) -> cloudflared::Result<BinaryStatus> {
        if let Some(status) = self.current.read().await.clone() {
            return Ok(status);
        }
        self.refresh().await
    }

    /// Locates the binary again (after an install, or when the user asks).
    ///
    /// # Errors
    /// [`cloudflared::Error::NotFound`] if cloudflared isn't installed.
    pub async fn refresh(&self) -> cloudflared::Result<BinaryStatus> {
        let status = self.locator.locate().await;
        *self.current.write().await = status.as_ref().ok().cloned();
        status
    }

    /// The latest release version published by Cloudflare.
    ///
    /// # Errors
    /// Network failures or an unexpected response.
    pub async fn latest_version(&self) -> cloudflared::Result<cloudflared::Version> {
        let installer = Installer::new(self.locator.managed_dir().to_path_buf())?;
        Ok(installer.latest().await?.version)
    }

    /// Downloads, verifies and installs the latest release as the managed binary.
    /// Concurrent calls wait for the first to finish.
    ///
    /// # Errors
    /// Network, verification or file-system failures; the previous binary is kept.
    pub async fn install_latest(
        &self,
        progress: impl FnMut(InstallStep),
    ) -> cloudflared::Result<BinaryStatus> {
        let _guard = self.installing.lock().await;
        let installer = Installer::new(self.locator.managed_dir().to_path_buf())?;
        let release = installer.latest().await?;
        installer.install(&release, progress).await?;
        self.refresh().await
    }
}
