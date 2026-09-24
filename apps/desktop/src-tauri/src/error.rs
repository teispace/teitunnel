//! The error type returned by every IPC command.

use serde::Serialize;
use specta::Type;
use teitunnel_core::{
    ErrorKind,
    text::{Text, UserText, msg::app as m},
};

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
    /// Something changed since the user looked, or a confirmation is missing: refresh
    /// the preview.
    Conflict,
    /// The Cloudflare credential lacks a permission.
    PermissionDenied,
}

/// An error as shown to the user: what happened, and what to do about it. The text is
/// translated by the UI (D-062).
#[derive(Debug, Clone, Serialize, Type, thiserror::Error)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    /// Category for programmatic handling.
    pub code: ErrorCode,
    /// One sentence describing what happened.
    pub message: Text,
    /// What the user can do about it (boxed: most errors have none, and a small error
    /// keeps every command's `Result` small).
    pub hint: Option<Box<Text>>,
    /// The input field the error refers to.
    pub field: Option<String>,
}

/// In English, for logs.
impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message.english())
    }
}

impl AppError {
    pub(crate) fn new(code: ErrorCode, message: Text) -> Self {
        Self {
            code,
            message,
            hint: None,
            field: None,
        }
    }

    /// An internal error with a user-safe message. Log the cause before calling this.
    pub fn internal(message: Text) -> Self {
        Self::new(ErrorCode::Internal, message)
    }

    /// Rejected input for `field`.
    pub fn invalid(field: &str, message: Text) -> Self {
        Self {
            field: Some(field.to_owned()),
            ..Self::new(ErrorCode::InvalidInput, message)
        }
    }

    /// Adds a hint telling the user what to do next.
    #[must_use]
    pub fn with_hint(mut self, hint: Text) -> Self {
        self.hint = Some(Box::new(hint));
        self
    }
}

impl From<teitunnel_core::Error> for AppError {
    fn from(err: teitunnel_core::Error) -> Self {
        let text = err.text();
        match err.kind() {
            ErrorKind::CloudflaredMissing => {
                Self::new(ErrorCode::CloudflaredMissing, m::cloudflared_missing())
                    .with_hint(m::cloudflared_missing_hint())
            }
            ErrorKind::NotFound => Self::new(ErrorCode::NotFound, text),
            ErrorKind::InvalidInput => Self::invalid(err.field().unwrap_or("credential"), text),
            ErrorKind::Conflict => Self::new(ErrorCode::Conflict, text),
            ErrorKind::PermissionDenied => {
                Self::new(ErrorCode::PermissionDenied, text).with_hint(m::permission_hint())
            }
            ErrorKind::Unavailable => Self::new(ErrorCode::Unavailable, text),
            ErrorKind::Internal => {
                tracing::error!(error = %err, "command failed");
                Self::internal(text).with_hint(m::internal_hint())
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
    teitunnel_core::accounts::AccountError,
    teitunnel_core::engine::EngineError,
    teitunnel_core::store::StoreError,
    teitunnel_core::quick_share::QuickShareError,
    teitunnel_core::analytics::AnalyticsError,
);

impl From<teitunnel_core::domain::OriginError> for AppError {
    fn from(err: teitunnel_core::domain::OriginError) -> Self {
        Self::invalid("origin", err.text())
    }
}

/// A failure reported as a message (the machine's connector operations).
impl From<Text> for AppError {
    fn from(message: Text) -> Self {
        tracing::warn!(error = %message.english(), "operation failed");
        Self::internal(message)
    }
}

impl From<tauri::Error> for AppError {
    fn from(err: tauri::Error) -> Self {
        tracing::error!(error = %err, "tauri error");
        Self::internal(m::unexpected()).with_hint(m::internal_hint())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_text_for_the_frontend_to_translate() {
        let err = AppError::invalid("url", m::qr_too_long()).with_hint(m::internal_hint());
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "code": "invalidInput",
                "message": { "key": "core.app.qrTooLong", "args": {} },
                "hint": { "key": "core.app.internalHint", "args": {} },
                "field": "url",
            })
        );
        assert_eq!(err.to_string(), "That URL is too long for a QR code.");
    }

    #[test]
    fn origin_errors_point_at_the_field() {
        let err = AppError::from(teitunnel_core::domain::OriginError::InvalidPort);
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(err.field.as_deref(), Some("origin"));
    }

    #[test]
    fn routes_errors_map_to_fields_and_conflicts() {
        use teitunnel_core::engine::{EngineError, InputError, PlanError};
        let err = AppError::from(EngineError::Input(InputError {
            field: "origin",
            message: teitunnel_core::text::msg::raw("bad"),
        }));
        assert_eq!(
            (err.code, err.field.as_deref()),
            (ErrorCode::InvalidInput, Some("origin"))
        );
        let err = AppError::from(EngineError::Plan(PlanError::NoZone("a.b.c".into())));
        assert_eq!(err.field.as_deref(), Some("hostname"));
        assert_eq!(
            AppError::from(EngineError::NeedsConfirmation).code,
            ErrorCode::Conflict
        );
        assert_eq!(
            AppError::from(EngineError::Plan(PlanError::NoTunnel)).code,
            ErrorCode::NotFound
        );
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
