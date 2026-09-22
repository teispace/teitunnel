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
    /// Pagination details, on list endpoints.
    #[serde(default)]
    pub result_info: Option<ResultInfo>,
}

/// Pagination details of a list response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct ResultInfo {
    /// Current page (1-based).
    #[serde(default)]
    pub page: u32,
    /// Items per page.
    #[serde(default)]
    pub per_page: u32,
    /// Total pages, when reported.
    #[serde(default)]
    pub total_pages: Option<u32>,
    /// Total items, when reported.
    #[serde(default)]
    pub total_count: Option<u32>,
}

impl<T: DeserializeOwned> Envelope<T> {
    /// Decodes a response body and converts an unsuccessful envelope into [`Error::Api`].
    ///
    /// # Errors
    /// Returns [`Error::Decode`] for malformed bodies and [`Error::Api`] when the API
    /// reports failure or omits the result.
    pub fn decode(status: u16, body: &[u8]) -> Result<T> {
        Self::decode_page(status, body).map(|(result, _)| result)
    }

    /// Like [`Envelope::decode`], also returning pagination details.
    ///
    /// # Errors
    /// See [`Envelope::decode`].
    pub fn decode_page(status: u16, body: &[u8]) -> Result<(T, Option<ResultInfo>)> {
        let envelope: Self = serde_json::from_slice(body).map_err(|err| {
            // An HTML error page from a proxy is an API failure with that status, not a
            // decoding bug.
            if (200..300).contains(&status) {
                Error::Decode(err)
            } else {
                Error::Api {
                    status,
                    errors: Vec::new(),
                }
            }
        })?;
        match (
            envelope.success && (200..300).contains(&status),
            envelope.result,
        ) {
            (true, Some(result)) => Ok((result, envelope.result_info)),
            _ => Err(Error::Api {
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
            other => panic!("expected API error, got {other:?}"),
        }
        assert!(err.to_string().contains("code 81053"));
    }

    #[test]
    fn html_error_pages_are_api_errors() {
        let err =
            Envelope::<serde_json::Value>::decode(502, b"<html>bad gateway</html>").unwrap_err();
        assert_eq!(err.status(), Some(502));
    }

    #[test]
    fn malformed_success_bodies_are_decode_errors() {
        let err = Envelope::<serde_json::Value>::decode(200, b"{").unwrap_err();
        assert!(matches!(err, Error::Decode(_)));
    }

    #[test]
    fn reads_pagination() {
        let body = br#"{"success":true,"errors":[],"messages":[],"result":[1,2],"result_info":{"page":2,"per_page":2,"total_pages":3,"total_count":6}}"#;
        let (items, info): (Vec<u8>, _) = Envelope::decode_page(200, body).unwrap();
        assert_eq!(items, [1, 2]);
        assert_eq!(info.unwrap().total_pages, Some(3));
    }
}
