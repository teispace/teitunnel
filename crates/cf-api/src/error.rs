use crate::envelope::ApiMessage;

/// Errors returned by the Cloudflare API client.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The API answered with `success: false`. The first message is the most specific.
    #[error("Cloudflare API error: {}", first_message(.errors))]
    Api {
        /// HTTP status code of the response.
        status: u16,
        /// Error messages from the response envelope.
        errors: Vec<ApiMessage>,
    },
    /// The response body could not be decoded.
    #[error("unexpected response from Cloudflare: {0}")]
    Decode(#[from] serde_json::Error),
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

fn first_message(errors: &[ApiMessage]) -> String {
    errors.first().map_or_else(
        || "no error details".to_owned(),
        |e| format!("{} (code {})", e.message, e.code),
    )
}
