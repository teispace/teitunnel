use serde::{Deserialize, de::DeserializeOwned};

use crate::error::{Error, Result};

/// A message (error or info) inside a Cloudflare response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ApiMessage {
    /// Numeric Cloudflare error code, e.g. `81053` for a duplicate DNS record.
    pub code: u32,
    /// Human-readable message.
    pub message: String,
}

/// The standard `{ success, errors, messages, result }` wrapper of API v4 responses.
#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    /// Whether the request succeeded.
    pub success: bool,
    /// Errors, present when `success` is false.
    #[serde(default)]
    pub errors: Vec<ApiMessage>,
    /// Informational messages.
    #[serde(default)]
    pub messages: Vec<ApiMessage>,
    /// The payload, present when `success` is true.
    pub result: Option<T>,
}

impl<T: DeserializeOwned> Envelope<T> {
    /// Decodes a response body and converts an unsuccessful envelope into [`Error::Api`].
    ///
    /// # Errors
    /// Returns [`Error::Decode`] for malformed bodies and [`Error::Api`] when the API
    /// reports failure or omits the result.
    pub fn decode(status: u16, body: &[u8]) -> Result<T> {
        let envelope: Self = serde_json::from_slice(body)?;
        match (envelope.success, envelope.result) {
            (true, Some(result)) => Ok(result),
            (_, _) => Err(Error::Api {
                status,
                errors: envelope.errors,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_successful_envelope() {
        let body = br#"{"success":true,"errors":[],"messages":[],"result":{"id":"abc"}}"#;
        let value: serde_json::Value = Envelope::decode(200, body).unwrap();
        assert_eq!(value["id"], "abc");
    }

    #[test]
    fn surfaces_api_errors_with_code() {
        let body = br#"{"success":false,"errors":[{"code":81053,"message":"An A, AAAA, or CNAME record with that host already exists."}],"messages":[],"result":null}"#;
        let err = Envelope::<serde_json::Value>::decode(400, body).unwrap_err();
        match &err {
            Error::Api { status, errors } => {
                assert_eq!(*status, 400);
                assert_eq!(errors[0].code, 81053);
            }
            Error::Decode(_) => panic!("expected API error"),
        }
        assert!(err.to_string().contains("code 81053"));
    }

    #[test]
    fn rejects_malformed_body() {
        let err =
            Envelope::<serde_json::Value>::decode(502, b"<html>bad gateway</html>").unwrap_err();
        assert!(matches!(err, Error::Decode(_)));
    }
}
