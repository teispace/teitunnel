//! The error type returned by every IPC command.

use serde::Serialize;
use specta::Type;

/// Machine-readable error category. The frontend branches on this, never on `message`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// An unexpected failure. The message is safe to show; details are in the app log.
    Internal,
}

/// An error as shown to the user: what happened, and what to do about it.
#[derive(Debug, Clone, Serialize, Type, thiserror::Error)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct AppError {
    /// Category for programmatic handling.
    pub code: ErrorCode,
    /// One sentence describing what happened.
    pub message: String,
    /// What the user can do about it.
    pub hint: Option<String>,
    /// The input field the error refers to.
    pub field: Option<String>,
}

impl AppError {
    /// An internal error with a user-safe message. Log the cause before calling this.
    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            message: message.into(),
            hint: None,
            field: None,
        }
    }

    /// Adds a hint telling the user what to do next.
    #[must_use]
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

impl From<teitunnel_core::Error> for AppError {
    fn from(err: teitunnel_core::Error) -> Self {
        tracing::error!(error = %err, "core error");
        Self::internal(err.to_string())
    }
}

impl From<teitunnel_core::store::StoreError> for AppError {
    fn from(err: teitunnel_core::store::StoreError) -> Self {
        teitunnel_core::Error::from(err).into()
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        tracing::error!(error = %err, "tauri error");
        Self::internal("Something went wrong inside Teitunnel.")
            .with_hint("Try again. If it keeps happening, export diagnostics from the Help menu.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_camel_case_for_the_frontend() {
        let err = AppError::internal("Couldn't read settings.").with_hint("Restart Teitunnel.");
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "code": "internal",
                "message": "Couldn't read settings.",
                "hint": "Restart Teitunnel.",
                "field": null,
            })
        );
    }
}
