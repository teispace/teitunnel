//! Pages that load fine but point visitors somewhere they can't follow.
//!
//! Two mistakes are common behind a tunnel. The page links to this computer
//! (`http://localhost:5173/@vite/client`, `http://127.0.0.1:8000/css/app.css`): a
//! Laravel or Rails app with its dev asset server, or an app whose base URL is set to
//! localhost. Or it links to its own public address over plain `http://`, because the
//! app saw an HTTP request from cloudflared and didn't trust `X-Forwarded-Proto`:
//! browsers block those scripts and styles as mixed content. Either way the share
//! "works" and the page is broken, so the check reads the page, finds such a link and
//! says what to change for the framework it is.

use serde::Serialize;

use crate::{
    discovery::ServiceKind,
    text::{Text, msg},
};

/// What's wrong with a page's links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum LinkKind {
    /// It loads something from this computer, which visitors can't reach.
    Local,
    /// It loads something from its public address over plain HTTP (mixed content).
    Insecure,
}

/// A page linking where visitors can't follow, with what to change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LinkProblem {
    /// Which mistake.
    pub kind: LinkKind,
    /// A link from the page, e.g. `http://localhost:5173/@vite/client`.
    pub example: String,
    /// What it means, in a sentence.
    pub message: Text,
    /// What to change, for the framework when it's known.
    pub fix: Text,
}

/// Hosts that mean "this computer".
const LOCAL_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]", "0.0.0.0"];

/// Attributes that make the browser load something (not plain links a visitor may
/// never follow): scripts, images, media, frames, stylesheets and form targets.
fn loads(tag: &str, attribute: &str) -> bool {
    match attribute {
        "src" | "srcset" | "action" | "poster" | "data" => true,
        "href" => tag == "link",
        _ => false,
    }
}

/// The first link in `html` that visitors of `public_host` can't follow, if any.
pub fn find(html: &str, public_host: &str) -> Option<(LinkKind, String)> {
    let insecure = format!("http://{}", public_host.to_ascii_lowercase());
    let mut insecure_found = None;
    for (tag, attribute, value) in attributes(html) {
        if !loads(&tag, &attribute) {
            continue;
        }
        // `srcset` lists several URLs; the first one says enough.
        let url = value.split_whitespace().next().unwrap_or_default();
        let lower = url.to_ascii_lowercase();
        let Some(rest) = lower
            .strip_prefix("http://")
            .or_else(|| lower.strip_prefix("https://"))
            .or_else(|| lower.strip_prefix("//"))
        else {
            continue;
        };
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let bare = host.rsplit_once(':').map_or(host, |(h, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                host
            }
        });
        if LOCAL_HOSTS.contains(&bare) {
            // The worse of the two: say it first.
            return Some((LinkKind::Local, url.to_owned()));
        }
        if insecure_found.is_none()
            && lower.starts_with(&insecure)
            && matches!(
                lower.as_bytes().get(insecure.len()),
                None | Some(b'/' | b':' | b'?')
            )
        {
            insecure_found = Some(url.to_owned());
        }
    }
    insecure_found.map(|url| (LinkKind::Insecure, url))
}

/// The problem in words, with the fix for `kind` of service when it's known.
pub fn problem(
    kind: LinkKind,
    example: String,
    public_host: &str,
    service: Option<ServiceKind>,
) -> LinkProblem {
    use msg::dev_server::links as m;
    let url = format!("https://{public_host}");
    let (message, fix) = match kind {
        LinkKind::Local => (
            m::local(&example),
            match service {
                Some(ServiceKind::Laravel | ServiceKind::Php) => m::local_laravel(&url),
                Some(ServiceKind::Rails | ServiceKind::Ruby) => m::local_rails(),
                Some(ServiceKind::Django | ServiceKind::Python) => m::local_django(),
                _ => m::local_other(&url),
            },
        ),
        LinkKind::Insecure => (
            m::insecure(&example),
            match service {
                Some(ServiceKind::Laravel | ServiceKind::Php) => m::insecure_laravel(),
                Some(ServiceKind::Rails | ServiceKind::Ruby) => m::insecure_rails(),
                Some(ServiceKind::Django | ServiceKind::Python) => m::insecure_django(),
                Some(ServiceKind::Flask | ServiceKind::FastApi) => m::insecure_python(),
                Some(
                    ServiceKind::Node
                    | ServiceKind::Next
                    | ServiceKind::Nuxt
                    | ServiceKind::Remix
                    | ServiceKind::Astro
                    | ServiceKind::SvelteKit,
                ) => m::insecure_node(),
                _ => m::insecure_other(),
            },
        ),
    };
    LinkProblem {
        kind,
        example,
        message,
        fix,
    }
}

/// Every `(tag, attribute, value)` of the start tags in `html`, lowercase names, values
/// unquoted. Comments are skipped, and inline scripts and styles aren't looked into.
fn attributes(html: &str) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    while let Some(offset) = html[i..].find('<') {
        let start = i + offset + 1;
        if html[start..].starts_with("!--") {
            i = html[start..]
                .find("-->")
                .map_or(html.len(), |end| start + end + 3);
            continue;
        }
        let name_len = html[start..]
            .find(|c: char| !c.is_ascii_alphanumeric())
            .unwrap_or(html.len() - start);
        if name_len == 0 {
            i = start;
            continue;
        }
        let tag = html[start..start + name_len].to_ascii_lowercase();
        let mut j = start + name_len;
        // Attributes until the tag's end.
        loop {
            while j < bytes.len() && (bytes[j].is_ascii_whitespace() || bytes[j] == b'/') {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] == b'>' {
                break;
            }
            let name_start = j;
            while j < bytes.len()
                && !bytes[j].is_ascii_whitespace()
                && !matches!(bytes[j], b'=' | b'>' | b'/')
            {
                j += 1;
            }
            let name = html[name_start..j].to_ascii_lowercase();
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            if j >= bytes.len() || bytes[j] != b'=' {
                if name.is_empty() {
                    j += 1;
                }
                continue;
            }
            j += 1;
            while j < bytes.len() && bytes[j].is_ascii_whitespace() {
                j += 1;
            }
            let value = match bytes.get(j) {
                Some(&quote @ (b'"' | b'\'')) => {
                    let from = j + 1;
                    let end = html[from..]
                        .find(char::from(quote))
                        .map_or(html.len(), |e| from + e);
                    j = (end + 1).min(html.len());
                    &html[from..end]
                }
                _ => {
                    let from = j;
                    while j < bytes.len() && !bytes[j].is_ascii_whitespace() && bytes[j] != b'>' {
                        j += 1;
                    }
                    &html[from..j]
                }
            };
            out.push((tag.clone(), name, value.trim().to_owned()));
        }
        i = j.min(html.len());
        // Inline scripts and styles hold text, not tags.
        if matches!(tag.as_str(), "script" | "style") {
            let close = format!("</{tag}");
            i = html[i..]
                .to_ascii_lowercase()
                .find(&close)
                .map_or(html.len(), |end| i + end);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOST: &str = "shop.teispace.com";

    #[test]
    fn finds_links_to_this_computer() {
        // Laravel with its Vite dev server.
        let laravel = r#"<!DOCTYPE html><html><head>
            <script type="module" src="http://[::1]:5173/@vite/client"></script>
            <link rel="stylesheet" href="http://[::1]:5173/resources/css/app.css" />
            </head><body><a href="http://localhost:8000/about">About</a></body></html>"#;
        assert_eq!(
            find(laravel, HOST),
            Some((LinkKind::Local, "http://[::1]:5173/@vite/client".to_owned()))
        );
        let base_url = r"<img src='http://127.0.0.1:8000/logo.png' alt=logo>";
        assert_eq!(
            find(base_url, HOST).map(|(kind, _)| kind),
            Some(LinkKind::Local)
        );
        let protocol_relative = r#"<img srcset="//localhost:3000/a.png 1x, /a@2x.png 2x">"#;
        assert_eq!(
            find(protocol_relative, HOST),
            Some((LinkKind::Local, "//localhost:3000/a.png".to_owned()))
        );
    }

    #[test]
    fn finds_plain_http_to_the_public_address() {
        let page = r#"<link href="https://fonts.example.com/a.css" rel="stylesheet">
            <link rel="stylesheet" href="http://Shop.teispace.com/build/app.css">
            <form action="http://shop.teispace.com/login" method="post"></form>"#;
        assert_eq!(
            find(page, HOST),
            Some((
                LinkKind::Insecure,
                "http://Shop.teispace.com/build/app.css".to_owned()
            ))
        );
        // Another host that starts the same, or a page link, is fine.
        assert_eq!(
            find(
                r#"<script src="http://shop.teispace.com.cdn.net/a.js">"#,
                HOST
            ),
            None
        );
        assert_eq!(
            find(r#"<a href="http://shop.teispace.com/">home</a>"#, HOST),
            None
        );
    }

    #[test]
    fn a_working_page_is_fine() {
        let page = r#"<!doctype html><html><head>
            <!-- <script src="http://localhost:5173/old.js"></script> -->
            <script type="module" src="/assets/index-4f1c.js"></script>
            <script>const api = "http://localhost:3000/api";</script>
            <link rel="stylesheet" href="https://shop.teispace.com/app.css">
            </head><body><img src=/logo.svg><input disabled></body></html>"#;
        assert_eq!(find(page, HOST), None);
        assert_eq!(find("", HOST), None);
        assert_eq!(find("<", HOST), None);
        assert_eq!(find("<a href=", HOST), None);
        assert_eq!(
            find("<img src=\"http://localhost", HOST).map(|(k, _)| k),
            Some(LinkKind::Local)
        );
    }

    #[test]
    fn says_what_to_change_for_the_framework() {
        let local = problem(
            LinkKind::Local,
            "http://[::1]:5173/@vite/client".into(),
            HOST,
            Some(ServiceKind::Laravel),
        );
        assert!(
            local.fix.english().contains("npm run build"),
            "{}",
            local.fix.english()
        );
        let django = problem(
            LinkKind::Insecure,
            "http://shop.teispace.com/static/app.css".into(),
            HOST,
            Some(ServiceKind::Django),
        );
        assert!(django.fix.english().contains("SECURE_PROXY_SSL_HEADER"));
        assert!(
            django
                .message
                .english()
                .contains("http://shop.teispace.com/static/app.css")
        );
        let other = problem(
            LinkKind::Local,
            "http://localhost:3000/a.js".into(),
            HOST,
            None,
        );
        assert!(other.fix.english().contains("https://shop.teispace.com"));
    }
}
