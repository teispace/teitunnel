//! Pages Lens serves itself: sign-in, paused, blocked and error pages.
//!
//! Self-contained (no external assets), accessible (labels, landmarks, `lang`), light
//! and dark via `color-scheme`, and locked down with a strict CSP. Every interpolated
//! value is HTML-escaped by the caller-facing functions here.

use http::{HeaderValue, Response, StatusCode, header};

use crate::{LensBody, PausedPage, body::full, util::html_escape};

const STYLE: &str = r#"
:root { color-scheme: light dark; font-family: system-ui, -apple-system, "Segoe UI", sans-serif; }
body { margin: 0; min-height: 100vh; display: grid; place-items: center; background: Canvas; color: CanvasText; }
main { width: min(26rem, calc(100vw - 3rem)); padding: 2rem 0; }
h1 { font-size: 1.25rem; font-weight: 600; margin: 0 0 .5rem; }
p { margin: 0 0 1rem; line-height: 1.5; opacity: .8; }
form { display: grid; gap: .75rem; margin-top: 1.25rem; }
label { font-size: .875rem; font-weight: 500; }
input { font: inherit; padding: .5rem .625rem; border: 1px solid color-mix(in srgb, CanvasText 25%, transparent); border-radius: .375rem; background: Canvas; color: CanvasText; }
input:focus-visible, button:focus-visible { outline: 2px solid AccentColor; outline-offset: 2px; }
button { font: inherit; font-weight: 500; padding: .5rem .75rem; border: 0; border-radius: .375rem; background: AccentColor; color: AccentColorText; cursor: default; }
.error { color: #c42b1c; opacity: 1; }
@media (prefers-color-scheme: dark) { .error { color: #ff99a4; } }
.listing { list-style: none; padding: 0; margin: 1rem 0 0; font-family: ui-monospace, monospace; font-size: .875rem; }
.listing li { padding: .25rem 0; }
.size { opacity: .6; margin-left: .5rem; }
footer { margin-top: 2rem; font-size: .75rem; opacity: .5; }
"#;

/// A complete HTML document around `body` (already-escaped HTML).
pub(crate) fn document(title: &str, body: &str) -> String {
    format!(
        "<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<meta name=\"robots\" content=\"noindex\">\n<title>{title}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<main>\n{body}\n<footer>Teitunnel</footer>\n</main>\n</body>\n</html>\n"
    )
}

/// An HTML response with Lens's security headers.
pub(crate) fn html(status: StatusCode, document: String) -> Response<LensBody> {
    let mut response = Response::new(full(document));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static(
            "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'",
        ),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
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

/// The password form. `next` is where to go after signing in (a local path).
pub(crate) fn password(action: &str, next: &str, error: Option<&str>) -> String {
    let error = error
        .map(|text| {
            format!(
                "<p class=\"error\" id=\"error\" role=\"alert\">{}</p>",
                html_escape(text)
            )
        })
        .unwrap_or_default();
    let described = if error.is_empty() {
        ""
    } else {
        " aria-describedby=\"error\" aria-invalid=\"true\""
    };
    document(
        "Password required",
        &format!(
            "<h1>This site is protected</h1>\n<p>Enter the password to continue.</p>\n{error}\n<form method=\"post\" action=\"{}\">\n<input type=\"hidden\" name=\"next\" value=\"{}\">\n<label for=\"password\">Password</label>\n<input id=\"password\" name=\"password\" type=\"password\" autocomplete=\"current-password\" required autofocus{described}>\n<button type=\"submit\">Continue</button>\n</form>",
            html_escape(action),
            html_escape(next)
        ),
    )
}

/// A titled message page.
pub(crate) fn message(title: &str, text: &str) -> String {
    document(
        &html_escape(title),
        &format!(
            "<h1>{}</h1>\n<p>{}</p>",
            html_escape(title),
            html_escape(text)
        ),
    )
}

/// The paused page.
pub(crate) fn paused(page: &PausedPage) -> Response<LensBody> {
    let mut response = html(
        StatusCode::SERVICE_UNAVAILABLE,
        message(&page.title, &page.message),
    );
    response.headers_mut().insert(
        header::RETRY_AFTER,
        HeaderValue::from(page.retry_after_secs),
    );
    response
}

/// A plain-text response for clients that don't want HTML.
pub(crate) fn text(status: StatusCode, text: String) -> Response<LensBody> {
    let mut response = Response::new(full(text));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

/// A permanent redirect (308, keeps the method) to `location`.
pub(crate) fn redirect(location: &str) -> Response<LensBody> {
    let mut response = text(
        StatusCode::PERMANENT_REDIRECT,
        format!("Moved to {location}\n"),
    );
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_page_escapes_everything() {
        let page = password(
            "/__teitunnel/login",
            "/\"><script>alert(1)</script>",
            Some("<img onerror=x>"),
        );
        assert!(!page.contains("<script>alert"));
        assert!(!page.contains("<img onerror"));
        assert!(page.contains("&lt;script&gt;"));
        assert!(page.contains("<label for=\"password\">"));
        assert!(page.contains("lang=\"en\""));
    }

    #[test]
    fn html_has_security_headers() {
        let response = html(StatusCode::UNAUTHORIZED, message("a", "b"));
        let headers = response.headers();
        assert!(headers.contains_key(header::CONTENT_SECURITY_POLICY));
        assert_eq!(headers[header::X_FRAME_OPTIONS], "DENY");
        assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    }

    #[test]
    fn paused_has_retry_after() {
        let response = paused(&PausedPage::default());
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()[header::RETRY_AFTER], "60");
    }
}
