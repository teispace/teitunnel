//! The error type returned by every IPC command.

use serde::Serialize;
use specta::Type;
use teitunnel_core::ErrorKind;

/// Machine-readable error category. The frontend branches on this, never on `message`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// An unexpected failure. The message is safe to show; details are in the app log.
    Internal,
    /// The input was rejected; `field` names the offending field.
    InvalidInput,
    /// The thing acted on doesn't exist (any more).
    NotFound,
    /// cloudflared isn't installed.
    CloudflaredMissing,
    /// Busy or exhausted; retrying later may work.
    Unavailable,
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
    fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            field: None,
        }
    }

    /// An internal error with a user-safe message. Log the cause before calling this.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// Rejected input for `field`.
    pub fn invalid(field: &str, message: impl Into<String>) -> Self {
        Self {
            field: Some(field.to_owned()),
            ..Self::new(ErrorCode::InvalidInput, message)
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
        match err.kind() {
            ErrorKind::CloudflaredMissing => Self::new(
                ErrorCode::CloudflaredMissing,
                "cloudflared isn't installed.",
            )
            .with_hint("Install it from the Quick Share page, or run `brew install cloudflared`."),
            ErrorKind::NotFound => Self::new(ErrorCode::NotFound, err.to_string()),
            ErrorKind::Unavailable => Self::new(ErrorCode::Unavailable, err.to_string()),
            ErrorKind::Internal => {
                tracing::error!(error = %err, "command failed");
                Self::internal(err.to_string()).with_hint(
                    "Try again. If it keeps happening, report an issue from the Help menu.",
                )
            }
        }
    }
}

macro_rules! via_core {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for AppError {
            fn from(err: $ty) -> Self {
                teitunnel_core::Error::from(err).into()
            }
        }
    )*};
}

via_core!(
    teitunnel_core::store::StoreError,
    teitunnel_core::quick_share::QuickShareError,
);

impl From<teitunnel_core::domain::OriginError> for AppError {
    fn from(err: teitunnel_core::domain::OriginError) -> Self {
        Self::invalid("origin", err.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        tracing::error!(error = %err, "tauri error");
        Self::internal("Something went wrong inside Teitunnel.")
            .with_hint("Try again. If it keeps happening, report an issue from the Help menu.")
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

    #[test]
    fn origin_errors_point_at_the_field() {
        let err = AppError::from(teitunnel_core::domain::OriginError::InvalidPort);
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(err.field.as_deref(), Some("origin"));
    }

    #[test]
    fn missing_binary_has_a_fix() {
        let err = AppError::from(teitunnel_core::quick_share::QuickShareError::Binary(
            teitunnel_core::CloudflaredError::NotFound,
        ));
        assert_eq!(err.code, ErrorCode::CloudflaredMissing);
        assert!(err.hint.is_some());
    }
}
