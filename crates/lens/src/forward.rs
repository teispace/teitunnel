//! Header handling for forwarding: hop-by-hop headers (RFC 9110 §7.6.1), `Host`,
//! `X-Forwarded-*`, and the request line for HTTP/1.1 or HTTP/2 origins.

use std::net::IpAddr;

use http::{
    HeaderMap, HeaderName, HeaderValue, Method, Request, Uri, Version, header,
    uri::{Authority, PathAndQuery, Scheme},
};

use crate::{ForwardedHeaders, HostHeader, LensBody, OriginConfig};

/// Headers that describe one connection and are never forwarded.
const HOP_BY_HOP: [HeaderName; 7] = [
    header::CONNECTION,
    HeaderName::from_static("keep-alive"),
    HeaderName::from_static("proxy-connection"),
    header::PROXY_AUTHENTICATE,
    header::PROXY_AUTHORIZATION,
    header::TE,
    header::TRANSFER_ENCODING,
];

/// Whether `headers` ask to switch protocols (HTTP/1.1 `Connection: upgrade`).
pub(crate) fn is_upgrade(version: Version, headers: &HeaderMap) -> bool {
    version == Version::HTTP_11
        && headers.contains_key(header::UPGRADE)
        && connection_tokens(headers).any(|token| token.eq_ignore_ascii_case("upgrade"))
}

fn connection_tokens(headers: &HeaderMap) -> impl Iterator<Item = &str> {
    headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|token| !token.is_empty())
}

/// Removes hop-by-hop headers, including those named in `Connection`. With `upgrade`,
/// keeps `Upgrade` and sets `Connection: upgrade` so the switch reaches the other side.
/// `TE: trailers` survives (gRPC needs it).
pub(crate) fn strip_hop_by_hop(headers: &mut HeaderMap, upgrade: bool) {
    let named: Vec<HeaderName> = connection_tokens(headers)
        .filter_map(|token| HeaderName::from_bytes(token.as_bytes()).ok())
        .collect();
    let te_trailers = headers
        .get(header::TE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("trailers"))
        });
    for name in named {
        if !(upgrade && name == header::UPGRADE) {
            headers.remove(name);
        }
    }
    for name in &HOP_BY_HOP {
        headers.remove(name);
    }
    if te_trailers {
        headers.insert(header::TE, HeaderValue::from_static("trailers"));
    }
    if upgrade {
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
    } else {
        headers.remove(header::UPGRADE);
    }
}

/// What the forwarder knows about the incoming request.
#[derive(Debug)]
pub(crate) struct Incoming<'a> {
    pub(crate) method: &'a Method,
    pub(crate) uri: &'a Uri,
    /// The host the visitor asked for.
    pub(crate) host: &'a str,
    /// The client, per the tap's trust setting.
    pub(crate) client_ip: IpAddr,
    /// `http` or `https` as Lens received it.
    pub(crate) scheme: &'a str,
    pub(crate) upgrade: bool,
}

/// Adjusts request headers for the origin and builds the request.
pub(crate) fn origin_request(
    origin: &OriginConfig,
    host_header: &HostHeader,
    forwarded: ForwardedHeaders,
    incoming: &Incoming<'_>,
    mut headers: HeaderMap,
    body: LensBody,
) -> Request<LensBody> {
    strip_hop_by_hop(&mut headers, incoming.upgrade);
    add_forwarded(&mut headers, forwarded, incoming);
    let host = match host_header {
        HostHeader::Preserve => incoming.host.to_owned(),
        HostHeader::Upstream => origin.url.authority(),
        HostHeader::Custom(host) => host.clone(),
    };
    let path_and_query = incoming
        .uri
        .path_and_query()
        .cloned()
        .unwrap_or_else(|| PathAndQuery::from_static("/"));
    let (uri, version) = if origin.http2 {
        headers.remove(header::HOST);
        let authority = Authority::try_from(host.as_str())
            .or_else(|_| Authority::try_from(origin.url.authority().as_str()));
        let scheme = if origin.url.is_https() {
            Scheme::HTTPS
        } else {
            Scheme::HTTP
        };
        let uri = authority.ok().and_then(|authority| {
            Uri::builder()
                .scheme(scheme)
                .authority(authority)
                .path_and_query(path_and_query.clone())
                .build()
                .ok()
        });
        (
            uri.unwrap_or_else(|| Uri::from(path_and_query)),
            Version::HTTP_2,
        )
    } else {
        let value = HeaderValue::from_str(&host)
            .or_else(|_| HeaderValue::from_str(&origin.url.authority()))
            .ok();
        if let Some(value) = value {
            headers.insert(header::HOST, value);
        }
        (Uri::from(path_and_query), Version::HTTP_11)
    };
    let mut request = Request::new(body);
    *request.method_mut() = incoming.method.clone();
    *request.uri_mut() = uri;
    *request.version_mut() = version;
    *request.headers_mut() = headers;
    request
}

fn add_forwarded(headers: &mut HeaderMap, mode: ForwardedHeaders, incoming: &Incoming<'_>) {
    let replace = match mode {
        ForwardedHeaders::Off => return,
        ForwardedHeaders::Preserve => false,
        ForwardedHeaders::Replace => true,
    };
    let mut set = |name: &'static str, value: &str| {
        if (replace || !headers.contains_key(name))
            && let Ok(value) = HeaderValue::from_str(value)
        {
            headers.insert(name, value);
        }
    };
    set("x-forwarded-for", &incoming.client_ip.to_string());
    set("x-forwarded-proto", incoming.scheme);
    set("x-forwarded-host", incoming.host);
}

/// Response headers from the origin, made fit for the client.
pub(crate) fn response_headers(headers: &mut HeaderMap, upgrade: bool) {
    strip_hop_by_hop(headers, upgrade);
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use http_body::Body as _;

    use super::*;
    use crate::{OriginUrl, body::empty};

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_static(value));
        }
        map
    }

    #[test]
    fn strips_hop_by_hop() {
        let mut h = headers(&[
            ("connection", "keep-alive, x-private"),
            ("keep-alive", "timeout=5"),
            ("x-private", "1"),
            ("transfer-encoding", "chunked"),
            ("te", "trailers, deflate"),
            ("proxy-authorization", "Basic x"),
            ("upgrade", "websocket"),
            ("x-keep", "yes"),
        ]);
        strip_hop_by_hop(&mut h, false);
        assert_eq!(h.len(), 2);
        assert_eq!(h["x-keep"], "yes");
        assert_eq!(h["te"], "trailers");
    }

    #[test]
    fn keeps_upgrade_when_upgrading() {
        let mut h = headers(&[("connection", "Upgrade"), ("upgrade", "websocket")]);
        assert!(is_upgrade(Version::HTTP_11, &h));
        assert!(!is_upgrade(Version::HTTP_2, &h));
        strip_hop_by_hop(&mut h, true);
        assert_eq!(h["upgrade"], "websocket");
        assert_eq!(h["connection"], "upgrade");
    }

    fn incoming<'a>(uri: &'a Uri, method: &'a Method) -> Incoming<'a> {
        Incoming {
            method,
            uri,
            host: "app.example.com",
            client_ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 9)),
            scheme: "http",
            upgrade: false,
        }
    }

    #[test]
    fn builds_h1_request_with_forwarded_headers() {
        let origin = OriginConfig::new(OriginUrl::parse("http://localhost:3000").unwrap());
        let uri: Uri = "/a?b=1".parse().unwrap();
        let method = Method::POST;
        let request = origin_request(
            &origin,
            &HostHeader::Preserve,
            ForwardedHeaders::Preserve,
            &incoming(&uri, &method),
            headers(&[("x-forwarded-proto", "https")]),
            empty(),
        );
        assert_eq!(request.uri(), "/a?b=1");
        assert_eq!(request.version(), Version::HTTP_11);
        let h = request.headers();
        assert_eq!(h["host"], "app.example.com");
        assert_eq!(
            h["x-forwarded-proto"], "https",
            "cloudflared's value is kept"
        );
        assert_eq!(h["x-forwarded-for"], "203.0.113.9");
        assert_eq!(h["x-forwarded-host"], "app.example.com");
        assert!(request.into_body().is_end_stream());
    }

    #[test]
    fn host_header_modes_and_replace() {
        let origin = OriginConfig::new(OriginUrl::parse("http://localhost:3000").unwrap());
        let uri: Uri = "/".parse().unwrap();
        let method = Method::GET;
        let request = origin_request(
            &origin,
            &HostHeader::Upstream,
            ForwardedHeaders::Replace,
            &incoming(&uri, &method),
            headers(&[("x-forwarded-proto", "https")]),
            empty(),
        );
        assert_eq!(request.headers()["host"], "localhost:3000");
        assert_eq!(request.headers()["x-forwarded-proto"], "http");
        let request = origin_request(
            &origin,
            &HostHeader::Custom("custom.test".into()),
            ForwardedHeaders::Off,
            &incoming(&uri, &method),
            HeaderMap::new(),
            empty(),
        );
        assert_eq!(request.headers()["host"], "custom.test");
        assert!(!request.headers().contains_key("x-forwarded-for"));
    }

    #[test]
    fn builds_h2_request_with_authority() {
        let mut origin = OriginConfig::new(OriginUrl::parse("http://127.0.0.1:50051").unwrap());
        origin.http2 = true;
        let uri: Uri = "/pkg.Service/Method".parse().unwrap();
        let method = Method::POST;
        let request = origin_request(
            &origin,
            &HostHeader::Preserve,
            ForwardedHeaders::Off,
            &incoming(&uri, &method),
            headers(&[("host", "app.example.com"), ("te", "trailers")]),
            empty(),
        );
        assert_eq!(request.version(), Version::HTTP_2);
        assert_eq!(request.uri(), "http://app.example.com/pkg.Service/Method");
        assert!(!request.headers().contains_key("host"));
        assert_eq!(request.headers()["te"], "trailers");
    }
}
