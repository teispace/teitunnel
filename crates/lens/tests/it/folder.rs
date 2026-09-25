//! Folder sharing: static files, ranges, validators, traversal, SPA fallback, listing.

use std::fs;

use http::{Method, StatusCode};
use lens::{FolderConfig, Responder, Upstream};
use tempfile::TempDir;

use crate::support::*;

fn site() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("index.html"), "<h1>home</h1>").unwrap();
    fs::write(root.join("app.js"), "console.log('hi')").unwrap();
    fs::write(root.join("data.bin"), (0u8..=255).collect::<Vec<u8>>()).unwrap();
    fs::write(root.join(".env"), "SECRET=1").unwrap();
    fs::create_dir(root.join("docs")).unwrap();
    fs::write(root.join("docs/guide.md"), "# Guide").unwrap();
    fs::create_dir(root.join("empty dir")).unwrap();
    fs::write(root.join("empty dir/a b.txt"), "spaced").unwrap();
    dir
}

async fn folder_lens(
    root: &std::path::Path,
    configure: impl FnOnce(&mut FolderConfig),
) -> (lens::Lens, lens::TapHandle) {
    let mut config = FolderConfig::new(root);
    configure(&mut config);
    lens_with(Upstream::Folder(config), |_| {}).await
}

#[tokio::test]
async fn serves_files_with_types_and_validators() {
    let dir = site();
    let (lens, tap) = folder_lens(dir.path(), |_| {}).await;
    let home = get(tap.addr, "/").await;
    assert_eq!(home.status, StatusCode::OK);
    assert_eq!(home.text(), "<h1>home</h1>");
    assert_eq!(home.headers["content-type"], "text/html; charset=utf-8");
    let js = get(tap.addr, "/app.js").await;
    assert!(
        js.headers["content-type"]
            .to_str()
            .unwrap()
            .contains("javascript")
    );
    assert_eq!(js.headers["accept-ranges"], "bytes");
    let etag = js.headers["etag"].clone();
    let modified = js.headers["last-modified"].clone();

    let cached = fetch(
        tap.addr,
        request(Method::GET, "/app.js")
            .header("if-none-match", etag.clone())
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);
    assert!(cached.body.is_empty());
    let cached = fetch(
        tap.addr,
        request(Method::GET, "/app.js")
            .header("if-modified-since", modified)
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(cached.status, StatusCode::NOT_MODIFIED);

    let head = fetch(
        tap.addr,
        request(Method::HEAD, "/app.js")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(head.status, StatusCode::OK);
    assert_eq!(head.headers["content-length"], "17");
    assert!(head.body.is_empty());

    let post = fetch(
        tap.addr,
        request(Method::POST, "/app.js")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(post.status, StatusCode::METHOD_NOT_ALLOWED);

    let spaced = get(tap.addr, "/empty%20dir/a%20b.txt").await;
    assert_eq!(spaced.text(), "spaced");

    let exchange = exchange_for(&lens, "/app.js").await;
    assert_eq!(exchange.responder, Responder::Folder);
}

#[tokio::test]
async fn byte_ranges() {
    let dir = site();
    let (_lens, tap) = folder_lens(dir.path(), |_| {}).await;
    let range = |value: &'static str| {
        request(Method::GET, "/data.bin")
            .header("range", value)
            .body(lens::empty())
            .unwrap()
    };
    let part = fetch(tap.addr, range("bytes=10-19")).await;
    assert_eq!(part.status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(part.headers["content-range"], "bytes 10-19/256");
    assert_eq!(&part.body[..], &(10u8..20).collect::<Vec<u8>>()[..]);
    let suffix = fetch(tap.addr, range("bytes=-6")).await;
    assert_eq!(&suffix.body[..], &[250, 251, 252, 253, 254, 255]);
    let open = fetch(tap.addr, range("bytes=250-")).await;
    assert_eq!(open.body.len(), 6);
    let beyond = fetch(tap.addr, range("bytes=999-")).await;
    assert_eq!(beyond.status, StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(beyond.headers["content-range"], "bytes */256");
    let multi = fetch(tap.addr, range("bytes=0-1,4-5")).await;
    assert_eq!(multi.status, StatusCode::OK);
    assert_eq!(multi.body.len(), 256);
    let stale = fetch(
        tap.addr,
        request(Method::GET, "/data.bin")
            .header("range", "bytes=0-1")
            .header("if-range", "\"not-the-etag\"")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(
        stale.status,
        StatusCode::OK,
        "If-Range mismatch serves the whole file"
    );
}

#[tokio::test]
async fn traversal_and_hidden_files_are_refused() {
    let outer = tempfile::tempdir().unwrap();
    fs::write(outer.path().join("secret.txt"), "outside").unwrap();
    let dir = site();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(outer.path().join("secret.txt"), dir.path().join("link.txt"))
            .unwrap();
        std::os::unix::fs::symlink(outer.path(), dir.path().join("linkdir")).unwrap();
        fs::write(dir.path().join("docs/inside.txt"), "inside").unwrap();
        std::os::unix::fs::symlink(
            dir.path().join("docs/inside.txt"),
            dir.path().join("ok-link.txt"),
        )
        .unwrap();
    }
    let (_lens, tap) = folder_lens(dir.path(), |_| {}).await;
    for path in [
        "/../secret.txt",
        "/docs/../../secret.txt",
        "/%2e%2e/secret.txt",
        "/%2e%2e%2fsecret.txt",
        "/docs/%2E%2E/%2E%2E/secret.txt",
        "/..%5csecret.txt",
        "/.env",
        "/%2eenv",
        "/link.txt",
        "/linkdir/secret.txt",
        "/%00",
    ] {
        let reply = get(tap.addr, path).await;
        assert_ne!(reply.status, StatusCode::OK, "{path}");
        assert!(
            !reply.text().contains("outside") && !reply.text().contains("SECRET"),
            "{path}"
        );
    }
    #[cfg(unix)]
    assert_eq!(
        get(tap.addr, "/ok-link.txt").await.text(),
        "inside",
        "links inside the root work"
    );
}

#[tokio::test]
async fn directories_redirect_list_and_fall_back() {
    let dir = site();
    let (_lens, tap) = folder_lens(dir.path(), |config| {
        config.listing = true;
        config.spa_fallback = true;
    })
    .await;
    let redirect = get(tap.addr, "/docs?x=1").await;
    assert_eq!(redirect.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(redirect.headers["location"], "/docs/?x=1");
    let listing = get(tap.addr, "/docs/").await;
    assert_eq!(listing.status, StatusCode::OK);
    assert!(
        listing.text().contains("href=\"guide%2Emd\">guide.md</a>"),
        "{}",
        listing.text()
    );
    let root_listing_hidden = get(tap.addr, "/empty%20dir/").await;
    assert!(root_listing_hidden.text().contains("a b.txt"));

    // Unknown routes of a single-page app get index.html; missing assets stay 404.
    let route = fetch(
        tap.addr,
        request(Method::GET, "/settings/profile")
            .header("accept", "text/html")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(route.status, StatusCode::OK);
    assert_eq!(route.text(), "<h1>home</h1>");
    let asset = fetch(
        tap.addr,
        request(Method::GET, "/missing.png")
            .header("accept", "image/*")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(asset.status, StatusCode::NOT_FOUND);

    // Without listing, a directory without index is a 404.
    let (_lens, tap) = folder_lens(dir.path(), |_| {}).await;
    assert_eq!(get(tap.addr, "/docs/").await.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn large_files_stream() {
    let dir = tempfile::tempdir().unwrap();
    let data: Vec<u8> = (0..(8 * 1024 * 1024))
        .map(|i: u32| (i % 251) as u8)
        .collect();
    fs::write(dir.path().join("big.bin"), &data).unwrap();
    let (lens, tap) = folder_lens(dir.path(), |_| {}).await;
    let reply = get(tap.addr, "/big.bin").await;
    assert_eq!(reply.body.len(), data.len());
    assert_eq!(&reply.body[..], &data[..]);
    let exchange = exchange_for(&lens, "/big.bin").await;
    let body = &exchange.response.as_ref().unwrap().body;
    assert_eq!(body.size, data.len() as u64);
    assert!(body.truncated);
}

#[tokio::test]
async fn missing_folders_are_rejected() {
    let lens = lens::Lens::new(lens::LensOptions::default()).unwrap();
    let result = lens.add_tap(lens::TapConfig::new(Upstream::folder(
        "/definitely/missing",
    )));
    assert!(matches!(result, Err(lens::LensError::InvalidFolder { .. })));
}
