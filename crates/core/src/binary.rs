//! The cloudflared binary Teitunnel runs: located once, cached, re-checked on demand.
//! Managed installs and updates join in M1-02.

use std::sync::Arc;

use cloudflared::{BinaryStatus, Locator};
use tokio::sync::RwLock;

/// Caches the located binary so every Quick Share doesn't re-run `--version`.
#[derive(Debug, Clone)]
pub struct BinaryManager {
    locator: Locator,
    current: Arc<RwLock<Option<BinaryStatus>>>,
}

impl BinaryManager {
    /// A manager using `locator`.
    pub fn new(locator: Locator) -> Self {
        Self {
            locator,
            current: Arc::default(),
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
}
