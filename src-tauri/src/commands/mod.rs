pub mod auth;
pub mod binary;
pub mod dns;
pub mod ingress;
pub mod metrics;
pub mod quick_tunnel;
pub mod terminal;
pub mod tunnels;

pub use auth::*;
pub use binary::*;
pub use dns::*;
pub use ingress::*;
pub use metrics::*;
pub use quick_tunnel::*;
pub use terminal::*;
pub use tunnels::*;

use crate::error::AppError;
use crate::services::{BinaryManager, KeyringStore};

/// Resolves an API token by checking, in order:
/// 1. Passed token argument (from frontend session / cert login)
/// 2. OS Keyring token (from manual API token setup)
/// 3. Embedded ARGO TUNNEL TOKEN in ~/.cloudflared/cert.pem
pub fn resolve_token(token: Option<String>) -> Result<String, AppError> {
    if let Some(t) = token {
        if !t.trim().is_empty() {
            return Ok(t.trim().to_string());
        }
    }
    if let Ok(Some(saved)) = KeyringStore::get_token() {
        if !saved.trim().is_empty() {
            return Ok(saved.trim().to_string());
        }
    }
    let cert = BinaryManager::check_cert_status();
    if let Some(cert_tok) = cert.api_token {
        if !cert_tok.trim().is_empty() {
            return Ok(cert_tok.trim().to_string());
        }
    }
    Err(AppError::KeyringError(
        "No Cloudflare API token configured or Origin Certificate found. Please log in via browser or add an API token.".into()
    ))
}

