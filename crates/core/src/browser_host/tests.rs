//! The native messaging host: manifests, framing, what may be shared, and the relay to
//! the app (a fake one).

use teitunnel_control::{Decision, Limits, testing};
use tokio::io::{AsyncWriteExt, duplex};

use super::*;

fn layout(platform: Platform, home: &Path) -> Layout {
    Layout::new(platform, home, &home.join("Local"), &home.join("Roaming"))
}

#[tokio::test]
async fn manifests_go_to_installed_browsers_only_and_stay_ours() {
    for platform in [Platform::MacOs, Platform::Linux] {
        let home = tempfile::tempdir().unwrap();
        let layout = layout(platform, home.path());
        let exe = Path::new("/Applications/Teitunnel.app/Contents/MacOS/teitunnel-cli");
        // Chrome and Firefox are installed; the others aren't.
        let (chrome, firefox) = match platform {
            Platform::MacOs => (
                home.path()
                    .join("Library/Application Support/Google/Chrome"),
                home.path().join("Library/Application Support/Mozilla"),
            ),
            _ => (
                home.path().join(".config/google-chrome"),
                home.path().join(".mozilla"),
            ),
        };
        fs::create_dir_all(&chrome).unwrap();
        fs::create_dir_all(&firefox).unwrap();
        let status = layout.install(exe).await.unwrap();
        let installed: Vec<Browser> = status
            .iter()
            .filter(|s| s.installed)
            .map(|s| s.browser)
            .collect();
        assert_eq!(
            installed,
            [Browser::Chrome, Browser::Firefox],
            "{platform:?}"
        );
        assert!(
            status
                .iter()
                .any(|s| s.browser == Browser::Edge && !s.detected)
        );

        let hosts = if platform == Platform::MacOs {
            "NativeMessagingHosts"
        } else {
            "native-messaging-hosts"
        };
        let written: Value =
            serde_json::from_slice(&fs::read(firefox.join(hosts).join(file_name())).unwrap())
                .unwrap();
        assert_eq!(written["allowed_extensions"], json!([FIREFOX_EXTENSION_ID]));
        assert_eq!(written["path"], exe.to_string_lossy().as_ref());

        let status = layout.uninstall(exe).await.unwrap();
        assert!(status.iter().all(|s| !s.installed));
        assert!(
            !chrome
                .join("NativeMessagingHosts")
                .join(file_name())
                .exists()
        );
    }
}

#[tokio::test]
async fn someone_elses_manifest_is_left_alone() {
    let home = tempfile::tempdir().unwrap();
    let layout = layout(Platform::Linux, home.path());
    let hosts = home.path().join(".config/chromium/NativeMessagingHosts");
    fs::create_dir_all(&hosts).unwrap();
    let foreign = hosts.join(file_name());
    fs::write(&foreign, r#"{"name": "com.example.other"}"#).unwrap();
    layout
        .install(Path::new("/usr/bin/teitunnel"))
        .await
        .unwrap();
    layout
        .uninstall(Path::new("/usr/bin/teitunnel"))
        .await
        .unwrap();
    assert_eq!(
        fs::read_to_string(&foreign).unwrap(),
        r#"{"name": "com.example.other"}"#
    );
}

#[test]
fn chromium_manifests_name_the_extension_and_windows_uses_the_registry() {
    let chrome = manifest(Browser::Edge, Path::new("C:\\Teitunnel\\teitunnel-cli.exe"));
    assert_eq!(chrome["type"], "stdio");
    assert_eq!(
        chrome["allowed_origins"][0],
        format!("chrome-extension://{}/", CHROMIUM_EXTENSION_IDS[0])
    );
    let home = Path::new("C:\\Users\\me");
    let windows = Layout::new(
        Platform::Windows,
        home,
        &home.join("AppData\\Local"),
        &home.join("AppData\\Roaming"),
    );
    let edge = windows
        .places
        .iter()
        .find(|p| p.browser == Browser::Edge)
        .unwrap();
    assert_eq!(
        edge.registry.as_deref(),
        Some(r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.teispace.teitunnel")
    );
    assert!(edge.manifest.ends_with("chromium.json"));
}

#[test]
fn knows_when_a_browser_starts_it() {
    let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert!(started_by_browser(&args(&[
        "teitunnel",
        "chrome-extension://kfefjidmbgiebhjcgkphjigickhclpic/"
    ])));
    assert!(started_by_browser(&args(&[
        "teitunnel",
        "/home/me/.mozilla/native-messaging-hosts/com.teispace.teitunnel.json",
        FIREFOX_EXTENSION_ID
    ])));
    assert!(!started_by_browser(&args(&["teitunnel", "share", "3000"])));
    assert!(!started_by_browser(&args(&[
        "teitunnel",
        "config.json",
        "x"
    ])));
}

#[test]
fn only_local_pages_can_be_shared() {
    assert_eq!(
        local_origin("http://localhost:5173/app?x=1").as_deref(),
        Some("http://localhost:5173")
    );
    assert_eq!(
        local_origin("https://shop.localhost/").as_deref(),
        Some("https://shop.localhost")
    );
    assert_eq!(
        local_origin("http://192.168.1.20:8000/").as_deref(),
        Some("http://192.168.1.20:8000")
    );
    assert_eq!(
        local_origin("http://[::1]:3000/").as_deref(),
        Some("http://[::1]:3000")
    );
    for public in [
        "https://example.com/",
        "https://8.8.8.8/",
        "file:///etc/passwd",
        "chrome://settings",
        "http://localhost.evil.com/",
    ] {
        assert_eq!(local_origin(public), None, "{public}");
    }
}

#[tokio::test]
async fn frames_are_length_prefixed_and_bounded() {
    let (mut a, mut b) = duplex(64 * 1024);
    write_message(&mut a, &json!({"id": 1, "method": "status"}))
        .await
        .unwrap();
    assert_eq!(
        read_message(&mut b).await.unwrap(),
        Some(json!({"id": 1, "method": "status"}))
    );
    a.write_all(&(MAX_MESSAGE + 1).to_le_bytes()).await.unwrap();
    assert!(read_message(&mut b).await.is_err());
    drop(a);
    let (a, mut b) = duplex(16);
    drop(a);
    assert_eq!(read_message(&mut b).await.unwrap(), None, "closed pipe");
}

/// Sends `requests` to a host serving `endpoint`, returning its answers.
async fn exchange(endpoint: &Endpoint, requests: &[Value]) -> Vec<Value> {
    let (mut browser, host_side) = duplex(256 * 1024);
    let (host_in, host_out) = tokio::io::split(host_side);
    let endpoint = endpoint.clone();
    let host = tokio::spawn(async move { serve(host_in, host_out, &endpoint).await });
    let mut answers = Vec::new();
    for request in requests {
        write_message(&mut browser, request).await.unwrap();
        answers.push(read_message(&mut browser).await.unwrap().unwrap());
    }
    drop(browser);
    host.await.unwrap().unwrap();
    answers
}

#[tokio::test]
async fn relays_to_the_app_what_the_extension_may_ask() {
    let data = tempfile::tempdir().unwrap();
    let app = testing::serve(data.path(), Limits::default())
        .await
        .unwrap();
    app.host.answer(Decision::Once);
    let answers = exchange(
        &app.endpoint,
        &[
            json!({"id": 1, "method": "status"}),
            json!({"id": 2, "method": "shares.list"}),
            json!({"id": 3, "method": "shares.start", "params": {"url": "https://bank.example.com/"}}),
            json!({"id": 4, "method": "shares.start", "params": {"url": "http://localhost:5173/login"}}),
            json!({"id": 5, "method": "routes.apply", "params": {}}),
        ],
    )
    .await;
    assert!(
        answers[0]["result"]["app"]["version"].is_string(),
        "{}",
        answers[0]
    );
    assert!(answers[1]["result"].is_array());
    assert_eq!(answers[2]["error"]["code"], "notLocal");
    assert_eq!(answers[3]["id"], 4);
    assert!(answers[3]["result"]["id"].is_string(), "{}", answers[3]);
    assert_eq!(
        answers[4]["error"]["code"], "unknown",
        "only a few calls pass"
    );
    let started = app.host.started.lock().unwrap().clone();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].origin, "http://localhost:5173");
    assert_eq!(app.host.asked(), 1, "the app asked the person first");
}

#[tokio::test]
async fn says_when_the_app_isnt_running() {
    let data = tempfile::tempdir().unwrap();
    let answers = exchange(
        &Endpoint::new(data.path()),
        &[json!({"id": "a", "method": "shares.list"})],
    )
    .await;
    assert_eq!(answers[0]["id"], "a");
    assert_eq!(answers[0]["error"]["code"], "appNotRunning");
}
