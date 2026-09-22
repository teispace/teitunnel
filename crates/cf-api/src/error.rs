use crate::envelope::ApiMessage;

/// Errors returned by the Cloudflare API client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The API answered with `success: false` or an error status.
    #[error("Cloudflare API error: {}", first_message(.errors, *.status))]
    Api {
        /// HTTP status code of the response.
        status: u16,
        /// Error messages from the response envelope.
        errors: Vec<ApiMessage>,
    },
    /// The response body could not be decoded.
    #[error("unexpected response from Cloudflare: {0}")]
    Decode(#[from] serde_json::Error),
    /// The request never got a response (network, TLS, timeout).
    #[error("couldn't reach Cloudflare: {0}")]
    Network(String),
}

impl Error {
    /// Cloudflare error codes in the response, e.g. `81053` for a duplicate DNS record.
    pub fn codes(&self) -> Vec<u32> {
        match self {
            Self::Api { errors, .. } => errors.iter().map(|e| e.code).collect(),
            _ => Vec::new(),
        }
    }

    /// The HTTP status, when there was a response.
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::Api { status, .. } => Some(*status),
            _ => None,
        }
    }

    /// Whether the credential was rejected or lacks permission (401/403, or code
    /// 10000 "Authentication error").
    pub fn is_auth(&self) -> bool {
        matches!(self.status(), Some(401 | 403)) || self.codes().contains(&10000)
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Self::Network(err.without_url().to_string())
    }
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

fn first_message(errors: &[ApiMessage], status: u16) -> String {
    errors.first().map_or_else(
        || format!("HTTP {status}"),
        |e| format!("{} (code {})", e.message, e.code),
    )
}
