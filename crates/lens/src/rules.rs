//! Header rewrite rules and the CORS helper.

use http::{HeaderMap, HeaderName, HeaderValue, Method, Response, StatusCode, header};
use serde::{Deserialize, Serialize};

use crate::{LensBody, LensError, body::empty};

/// One header change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "op")]
pub enum HeaderOp {
    /// Replace every value of `name` with `value`.
    Set {
        /// Header name.
        name: String,
        /// Value.
        value: String,
    },
    /// Add a value, keeping existing ones.
    Append {
        /// Header name.
        name: String,
        /// Value.
        value: String,
    },
    /// Remove the header.
    Remove {
        /// Header name.
        name: String,
    },
}

/// Header rewrites for a tap.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeaderRules {
    /// Applied to requests before they go upstream (after Lens's own headers).
    pub request: Vec<HeaderOp>,
    /// Applied to responses before they go to the client.
    pub response: Vec<HeaderOp>,
    /// Permissive CORS for development: answer preflights and allow the caller's origin
    /// (with credentials).
    pub cors: bool,
}

/// A validated header change.
#[derive(Debug, Clone)]
pub(crate) enum CompiledOp {
    Set(HeaderName, HeaderValue),
    Append(HeaderName, HeaderValue),
    Remove(HeaderName),
}

pub(crate) fn compile(ops: &[HeaderOp]) -> Result<Vec<CompiledOp>, LensError> {
    let name = |raw: &str| {
        HeaderName::from_bytes(raw.as_bytes())
            .map_err(|_| LensError::InvalidConfig(format!("invalid header name {raw:?}")))
    };
    let value = |raw: &str, header: &str| {
        HeaderValue::from_str(raw)
            .map_err(|_| LensError::InvalidConfig(format!("invalid value for header {header:?}")))
    };
    ops.iter()
        .map(|op| {
            Ok(match op {
                HeaderOp::Set { name: n, value: v } => CompiledOp::Set(name(n)?, value(v, n)?),
                HeaderOp::Append { name: n, value: v } => {
                    CompiledOp::Append(name(n)?, value(v, n)?)
                }
                HeaderOp::Remove { name: n } => CompiledOp::Remove(name(n)?),
            })
        })
        .collect()
}

pub(crate) fn apply(ops: &[CompiledOp], headers: &mut HeaderMap) {
    for op in ops {
        match op {
            CompiledOp::Set(name, value) => {
                headers.insert(name.clone(), value.clone());
            }
            CompiledOp::Append(name, value) => {
                headers.append(name.clone(), value.clone());
            }
            CompiledOp::Remove(name) => {
                headers.remove(name);
            }
        }
    }
}

/// A CORS preflight (`OPTIONS` with `Origin` and `Access-Control-Request-Method`).
pub(crate) fn is_preflight(method: &Method, headers: &HeaderMap) -> bool {
    method == Method::OPTIONS
        && headers.contains_key(header::ORIGIN)
        && headers.contains_key(header::ACCESS_CONTROL_REQUEST_METHOD)
}

/// The answer to a preflight: everything the caller asked for is allowed.
pub(crate) fn preflight_response(request: &HeaderMap) -> Response<LensBody> {
    let mut response = Response::new(empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    let headers = response.headers_mut();
    allow_origin(request, headers);
    if let Some(method) = request.get(header::ACCESS_CONTROL_REQUEST_METHOD) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS, method.clone());
    }
    if let Some(asked) = request.get(header::ACCESS_CONTROL_REQUEST_HEADERS) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, asked.clone());
    }
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("600"),
    );
    if request.contains_key("access-control-request-private-network") {
        headers.insert(
            "access-control-allow-private-network",
            HeaderValue::from_static("true"),
        );
    }
    response
}

/// Adds permissive CORS headers to a response for `request`.
pub(crate) fn add_cors(request: &HeaderMap, response: &mut HeaderMap) {
    if !request.contains_key(header::ORIGIN) {
        return;
    }
    allow_origin(request, response);
    response.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("*"),
    );
}

fn allow_origin(request: &HeaderMap, response: &mut HeaderMap) {
    if let Some(origin) = request.get(header::ORIGIN) {
        response.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, origin.clone());
        response.insert(
            header::ACCESS_CONTROL_ALLOW_CREDENTIALS,
            HeaderValue::from_static("true"),
        );
        response.append(header::VARY, HeaderValue::from_static("Origin"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compiles_and_applies() {
        let ops = compile(&[
            HeaderOp::Set {
                name: "X-A".into(),
                value: "1".into(),
            },
            HeaderOp::Append {
                name: "x-a".into(),
                value: "2".into(),
            },
            HeaderOp::Remove {
                name: "x-remove".into(),
            },
        ])
        .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-a", HeaderValue::from_static("0"));
        headers.insert("x-remove", HeaderValue::from_static("gone"));
        apply(&ops, &mut headers);
        let values: Vec<&str> = headers
            .get_all("x-a")
            .iter()
            .map(|v| v.to_str().unwrap())
            .collect();
        assert_eq!(values, vec!["1", "2"]);
        assert!(!headers.contains_key("x-remove"));
    }

    #[test]
    fn rejects_invalid_ops() {
        assert!(
            compile(&[HeaderOp::Remove {
                name: "bad name".into()
            }])
            .is_err()
        );
        assert!(
            compile(&[HeaderOp::Set {
                name: "x".into(),
                value: "bad\nvalue".into()
            }])
            .is_err()
        );
    }

    #[test]
    fn cors_preflight_and_response() {
        let mut request = HeaderMap::new();
        request.insert(header::ORIGIN, HeaderValue::from_static("https://app.test"));
        request.insert(
            header::ACCESS_CONTROL_REQUEST_METHOD,
            HeaderValue::from_static("PUT"),
        );
        request.insert(
            header::ACCESS_CONTROL_REQUEST_HEADERS,
            HeaderValue::from_static("x-token, content-type"),
        );
        assert!(is_preflight(&Method::OPTIONS, &request));
        assert!(!is_preflight(&Method::GET, &request));
        let response = preflight_response(&request);
        let headers = response.headers();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            headers[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            "https://app.test"
        );
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_METHODS], "PUT");
        assert_eq!(
            headers[header::ACCESS_CONTROL_ALLOW_HEADERS],
            "x-token, content-type"
        );
        assert_eq!(headers[header::ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");

        let mut out = HeaderMap::new();
        add_cors(&request, &mut out);
        assert_eq!(out[header::VARY], "Origin");
        let mut none = HeaderMap::new();
        add_cors(&HeaderMap::new(), &mut none);
        assert!(none.is_empty());
    }
}
