//! The pages a visitor sees while connecting: waiting for the person's approval (it
//! refreshes itself; no scripts), and short messages. Strict CSP, never framed.

use http::{HeaderValue, Response, StatusCode, header};
use lens::LensBody;

use super::protocol::WAIT;

/// Seconds between checks while waiting.
const REFRESH_SECS: u32 = 2;

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

fn page(status: StatusCode, head: &str, body: &str) -> Response<LensBody> {
    let html = format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<meta name=\"color-scheme\" content=\"light dark\">{head}<title>Teitunnel</title><style>\
body{{margin:0;min-height:100vh;display:grid;place-items:center;font:15px/1.5 -apple-system,BlinkMacSystemFont,\"Segoe UI\",system-ui,sans-serif;background:Canvas;color:CanvasText}}\
main{{max-width:28rem;padding:2rem;text-align:center}}h1{{font-size:1.25rem;margin:0 0 .5rem}}\
p{{margin:.5rem 0;opacity:.8}}.code{{font:600 2rem/1.2 ui-monospace,SFMono-Regular,Menlo,monospace;letter-spacing:.3em;margin:1.25rem 0;opacity:1}}\
</style></head><body><main>{body}</main></body></html>"
    );
    let mut response = Response::new(lens::full(html));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'",
        ),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

/// A short message.
pub(super) fn message(status: StatusCode, title: &str, text: &str) -> Response<LensBody> {
    page(
        status,
        "",
        &format!("<h1>{}</h1><p>{}</p>", escape(title), escape(text)),
    )
}

/// Waiting for the person to approve `client` in Teitunnel; checks again by itself.
pub(super) fn waiting(host: &str, client: &str, code: &str, request: &str) -> Response<LensBody> {
    let next = format!("{WAIT}?request={}", escape(request));
    page(
        StatusCode::OK,
        &format!("<meta http-equiv=\"refresh\" content=\"{REFRESH_SECS};url={next}\">"),
        &format!(
            "<h1>Approve in Teitunnel</h1>\
<p>{client} wants to connect to {host}. The person sharing it approves this in Teitunnel on their computer.</p>\
<p class=\"code\">{code}</p>\
<p>Teitunnel shows the same code. This page moves on by itself once it's answered.</p>",
            client = escape(client),
            host = escape(host),
            code = escape(code),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pages_escape_what_clients_say_and_never_run_scripts() {
        let response = waiting(
            "mcp.xyz.com",
            "<script>alert(1)</script>",
            "K7Q2",
            "ttreq_abc",
        );
        let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .to_owned();
        assert!(csp.starts_with("default-src 'none'"));
        let html = String::from_utf8(super::super::body_of(response).await.to_vec()).unwrap();
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("url=/__teitunnel/oauth/wait?request=ttreq_abc"));
        assert!(html.contains("K7Q2"));
    }
}
