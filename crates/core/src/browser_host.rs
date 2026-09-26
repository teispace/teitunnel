//! The browser extension's link to the app: Chrome, Edge, Brave and
//! other Chromium browsers, and Firefox, start the bundled `teitunnel` command as a
//! *native messaging host* when the extension asks, and exchange length-prefixed JSON
//! with it over standard input and output. The host relays a few calls to the running
//! app over its control connection, as the client "Teitunnel browser extension", so
//! anything that changes something is approved in the app like any other program.
//!
//! What it may ask: the app's status, the shares, sharing the page's own local server
//! (only an address on this computer or a private network), stopping a share, and
//! opening a view. Browsers only start the host for the extensions its manifest names.
//!
//! Manifests: one file per browser in its `NativeMessagingHosts` folder on macOS and
//! Linux; on Windows, two files under `%LOCALAPPDATA%\Teitunnel` and a registry key per
//! browser pointing at them (written with `reg.exe`, arguments passed one by one). Only
//! browsers that are installed get one, and only Teitunnel's own manifest is ever
//! replaced or removed.

use std::{
    fs, io,
    net::IpAddr,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use teitunnel_control::{
    ClientError, ControlClient, Endpoint,
    protocol::{ClientInfo, HostHeader, StartShare, View, code},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// The host's name, as the extension calls it.
pub const HOST_NAME: &str = "com.teispace.teitunnel";

/// The Chromium extension ids allowed to start the host: the unpacked extension's (its
/// manifest `key`), and the stores' once published.
pub const CHROMIUM_EXTENSION_IDS: &[&str] = &["kfefjidmbgiebhjcgkphjigickhclpic"];

/// The Firefox extension id (`browser_specific_settings.gecko.id`).
pub const FIREFOX_EXTENSION_ID: &str = "browser@teitunnel.teispace.com";

/// Largest message the host reads (browsers send at most 4 GB; ours are tiny).
const MAX_MESSAGE: u32 = 1024 * 1024;

/// A browser that can start the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Browser {
    Chrome,
    Chromium,
    Edge,
    Brave,
    Vivaldi,
    Arc,
    Firefox,
}

impl Browser {
    const ALL: [Self; 7] = [
        Self::Chrome,
        Self::Chromium,
        Self::Edge,
        Self::Brave,
        Self::Vivaldi,
        Self::Arc,
        Self::Firefox,
    ];

    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Chrome => "Google Chrome",
            Self::Chromium => "Chromium",
            Self::Edge => "Microsoft Edge",
            Self::Brave => "Brave",
            Self::Vivaldi => "Vivaldi",
            Self::Arc => "Arc",
            Self::Firefox => "Firefox",
        }
    }

    fn firefox(self) -> bool {
        self == Self::Firefox
    }
}

/// Which system's folders to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOs,
    Linux,
    Windows,
}

impl Platform {
    /// This system.
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// Where one browser looks.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Place {
    browser: Browser,
    /// The browser's own folder: it's installed when this exists.
    profile: PathBuf,
    /// Where the manifest goes.
    manifest: PathBuf,
    /// Windows: the registry key naming the manifest.
    registry: Option<String>,
}

/// The folders of every supported browser on a system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    places: Vec<Place>,
    platform: Platform,
}

/// One browser's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostStatus {
    pub browser: Browser,
    /// Its name.
    pub name: String,
    /// It's installed on this computer.
    pub detected: bool,
    /// Teitunnel's manifest is there and points at `exe`.
    pub installed: bool,
}

fn file_name() -> String {
    format!("{HOST_NAME}.json")
}

impl Layout {
    /// The folders under `home` (and, on Windows, the local and roaming app data).
    pub fn new(platform: Platform, home: &Path, local_data: &Path, roaming_data: &Path) -> Self {
        let place = |browser: Browser, profile: PathBuf, hosts: &str| Place {
            browser,
            manifest: profile.join(hosts).join(file_name()),
            profile,
            registry: None,
        };
        let places = match platform {
            Platform::MacOs => {
                let support = home.join("Library/Application Support");
                vec![
                    place(
                        Browser::Chrome,
                        support.join("Google/Chrome"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Chromium,
                        support.join("Chromium"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Edge,
                        support.join("Microsoft Edge"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Brave,
                        support.join("BraveSoftware/Brave-Browser"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Vivaldi,
                        support.join("Vivaldi"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Arc,
                        support.join("Arc/User Data"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Firefox,
                        support.join("Mozilla"),
                        "NativeMessagingHosts",
                    ),
                ]
            }
            Platform::Linux => {
                let config = home.join(".config");
                vec![
                    place(
                        Browser::Chrome,
                        config.join("google-chrome"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Chromium,
                        config.join("chromium"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Edge,
                        config.join("microsoft-edge"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Brave,
                        config.join("BraveSoftware/Brave-Browser"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Vivaldi,
                        config.join("vivaldi"),
                        "NativeMessagingHosts",
                    ),
                    place(
                        Browser::Firefox,
                        home.join(".mozilla"),
                        "native-messaging-hosts",
                    ),
                ]
            }
            Platform::Windows => {
                let ours = local_data.join("Teitunnel").join("NativeMessagingHosts");
                let windows = |browser: Browser, profile: PathBuf, key: &str| Place {
                    browser,
                    profile,
                    manifest: ours.join(if browser.firefox() {
                        "firefox.json"
                    } else {
                        "chromium.json"
                    }),
                    registry: Some(format!(
                        r"HKCU\Software\{key}\NativeMessagingHosts\{HOST_NAME}"
                    )),
                };
                vec![
                    windows(
                        Browser::Chrome,
                        local_data.join(r"Google\Chrome"),
                        r"Google\Chrome",
                    ),
                    windows(Browser::Chromium, local_data.join("Chromium"), "Chromium"),
                    windows(
                        Browser::Edge,
                        local_data.join(r"Microsoft\Edge"),
                        r"Microsoft\Edge",
                    ),
                    windows(
                        Browser::Brave,
                        local_data.join(r"BraveSoftware\Brave-Browser"),
                        r"BraveSoftware\Brave-Browser",
                    ),
                    windows(Browser::Vivaldi, local_data.join("Vivaldi"), r"Vivaldi"),
                    windows(Browser::Firefox, roaming_data.join("Mozilla"), "Mozilla"),
                ]
            }
        };
        Self { places, platform }
    }

    /// This system's folders.
    pub fn detect() -> Option<Self> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)?;
        let local = std::env::var_os("LOCALAPPDATA").map_or_else(|| home.clone(), PathBuf::from);
        let roaming = std::env::var_os("APPDATA").map_or_else(|| home.clone(), PathBuf::from);
        Some(Self::new(Platform::current(), &home, &local, &roaming))
    }

    /// Each supported browser, and whether it can start `exe` as the host.
    pub fn status(&self, exe: &Path) -> Vec<BrowserHostStatus> {
        let mut seen = std::collections::HashSet::new();
        Browser::ALL
            .iter()
            .filter_map(|browser| self.places.iter().find(|p| p.browser == *browser))
            .filter(|p| seen.insert(p.browser))
            .map(|place| BrowserHostStatus {
                browser: place.browser,
                name: place.browser.name().to_owned(),
                detected: place.profile.is_dir(),
                installed: ours(&place.manifest).is_some_and(|m| points_at(&m, exe)),
            })
            .collect()
    }

    /// Writes the manifest for every installed browser, pointing at `exe` (on Windows,
    /// with the registry keys). Returns the browsers' state after.
    ///
    /// # Errors
    /// A manifest couldn't be written, or `reg.exe` failed.
    pub async fn install(&self, exe: &Path) -> io::Result<Vec<BrowserHostStatus>> {
        for place in self.places.iter().filter(|p| p.profile.is_dir()) {
            if let Some(parent) = place.manifest.parent() {
                fs::create_dir_all(parent)?;
            }
            // Something else by this name is left alone.
            if place.manifest.exists() && ours(&place.manifest).is_none() {
                continue;
            }
            let body = serde_json::to_vec_pretty(&manifest(place.browser, exe))?;
            fs::write(&place.manifest, body)?;
            if let Some(key) = &place.registry {
                reg(&[
                    "add",
                    key,
                    "/ve",
                    "/t",
                    "REG_SZ",
                    "/d",
                    &place.manifest.to_string_lossy(),
                    "/f",
                ])
                .await?;
            }
        }
        Ok(self.status(exe))
    }

    /// Removes Teitunnel's manifests (and registry keys).
    ///
    /// # Errors
    /// A manifest couldn't be removed.
    pub async fn uninstall(&self, exe: &Path) -> io::Result<Vec<BrowserHostStatus>> {
        for place in &self.places {
            if ours(&place.manifest).is_some() {
                match fs::remove_file(&place.manifest) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err),
                }
            }
            if let Some(key) = &place.registry
                && self.platform == Platform::Windows
            {
                // Already gone is fine.
                let _ = reg(&["delete", key, "/f"]).await;
            }
        }
        Ok(self.status(exe))
    }
}

/// The manifest a browser reads.
pub fn manifest(browser: Browser, exe: &Path) -> Value {
    let mut manifest = json!({
        "name": HOST_NAME,
        "description": "Teitunnel: share local servers from your browser",
        "path": exe.to_string_lossy(),
        "type": "stdio",
    });
    if browser.firefox() {
        manifest["allowed_extensions"] = json!([FIREFOX_EXTENSION_ID]);
    } else {
        manifest["allowed_origins"] = json!(
            CHROMIUM_EXTENSION_IDS
                .iter()
                .map(|id| format!("chrome-extension://{id}/"))
                .collect::<Vec<_>>()
        );
    }
    manifest
}

/// The manifest at `path` if it's Teitunnel's.
fn ours(path: &Path) -> Option<Value> {
    let manifest: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    (manifest.get("name").and_then(Value::as_str) == Some(HOST_NAME)).then_some(manifest)
}

fn points_at(manifest: &Value, exe: &Path) -> bool {
    manifest.get("path").and_then(Value::as_str) == Some(&*exe.to_string_lossy())
}

async fn reg(args: &[&str]) -> io::Result<()> {
    if !cfg!(windows) {
        return Ok(());
    }
    let status = tokio::process::Command::new(r"C:\Windows\System32\reg.exe")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "reg.exe {} failed",
            args.first().unwrap_or(&"")
        )))
    }
}

/// Whether the arguments this process got are a browser starting it as the host: Chrome
/// passes the caller's origin (`chrome-extension://<id>/`), Firefox the manifest's path
/// and the extension's id.
pub fn started_by_browser(args: &[String]) -> bool {
    let first = args.get(1).map(String::as_str).unwrap_or_default();
    first.starts_with("chrome-extension://")
        || (args.len() >= 3
            && Path::new(first).extension().is_some_and(|e| e == "json")
            && args.get(2).is_some_and(|id| id == FIREFOX_EXTENSION_ID))
}

/// Reads one message; `None` when the browser closed the pipe.
///
/// # Errors
/// A message larger than 1 MiB, or invalid JSON.
pub async fn read_message<R: AsyncRead + Unpin>(input: &mut R) -> io::Result<Option<Value>> {
    let mut length = [0u8; 4];
    match input.read_exact(&mut length).await {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }
    let length = u32::from_le_bytes(length);
    if length > MAX_MESSAGE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    let mut body = vec![0u8; length as usize];
    input.read_exact(&mut body).await?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Writes one message.
///
/// # Errors
/// The pipe is closed.
pub async fn write_message<W: AsyncWrite + Unpin>(
    output: &mut W,
    message: &Value,
) -> io::Result<()> {
    let body = serde_json::to_vec(message)?;
    let length = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "message too large"))?;
    output.write_all(&length.to_le_bytes()).await?;
    output.write_all(&body).await?;
    output.flush().await
}

/// A request from the extension.
#[derive(Debug, Clone, Deserialize)]
struct Request {
    id: Value,
    method: String,
    #[serde(default)]
    params: Value,
}

/// The origin of a page served from this computer or a private network, which the
/// extension may share: `http://localhost:5173` from `http://localhost:5173/app?x=1`.
pub fn local_origin(page: &str) -> Option<String> {
    let uri: http::Uri = page.parse().ok()?;
    let scheme = uri.scheme_str().filter(|s| *s == "http" || *s == "https")?;
    let host = uri.host()?.to_ascii_lowercase();
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    let local = host == "localhost"
        || host.ends_with(".localhost")
        || bare.parse::<IpAddr>().is_ok_and(|ip| match ip {
            IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
        });
    if !local {
        return None;
    }
    Some(match uri.port_u16() {
        Some(port) => format!("{scheme}://{host}:{port}"),
        None => format!("{scheme}://{host}"),
    })
}

fn error(id: &Value, code: &str, message: impl Into<String>) -> Value {
    json!({ "id": id, "error": { "code": code, "message": message.into() } })
}

fn client_error(id: &Value, err: &ClientError) -> Value {
    match err {
        ClientError::NotRunning => error(
            id,
            "appNotRunning",
            "Teitunnel isn't running. Open it, then try again.",
        ),
        _ => match err.code() {
            Some(code::DISABLED) => error(
                id,
                "disabled",
                "Teitunnel's connection for other programs is off (Settings ▸ Integrations).",
            ),
            Some(code::DECLINED) => error(id, "declined", "Not allowed in Teitunnel."),
            Some(code::TIMEOUT) => error(id, "timeout", "Teitunnel didn't get an answer in time."),
            _ => error(id, "failed", err.to_string()),
        },
    }
}

fn client_info() -> ClientInfo {
    ClientInfo {
        name: "Teitunnel browser extension".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// Answers one request through `client`.
async fn answer(client: &ControlClient, request: Request) -> Value {
    let id = request.id.clone();
    let result = match request.method.as_str() {
        "status" => client.status().await.map(|s| json!(s)),
        "shares.list" => client.shares().await.map(|s| json!(s)),
        "shares.start" => {
            let Some(origin) = request
                .params
                .get("url")
                .and_then(Value::as_str)
                .and_then(local_origin)
            else {
                return error(
                    &id,
                    "notLocal",
                    "Only pages served from this computer or your network can be shared.",
                );
            };
            client
                .start_share(&StartShare {
                    origin,
                    stop_after_seconds: None,
                    host_header: HostHeader::default(),
                    folder: None,
                })
                .await
                .map(|s| json!(s))
        }
        "shares.stop" => match request.params.get("id").and_then(Value::as_str) {
            Some(share) => client.stop_share(share).await.map(|()| json!({})),
            None => return error(&id, "invalid", "Say which share to stop."),
        },
        "open" => {
            let view = match request.params.get("view").and_then(Value::as_str) {
                Some("shares") => View::Share { id: None },
                Some("inspector") => match request.params.get("share").and_then(Value::as_str) {
                    Some(share) => View::Inspector {
                        share: share.to_owned(),
                    },
                    None => return error(&id, "invalid", "Say which share to inspect."),
                },
                _ => View::Overview,
            };
            client.open(&view).await.map(|()| json!({}))
        }
        other => return error(&id, "unknown", format!("Unknown request {other}.")),
    };
    match result {
        Ok(value) => json!({ "id": id, "result": value }),
        Err(err) => client_error(&id, &err),
    }
}

/// Serves the extension until the browser closes the pipe: each request is relayed to
/// the app of `endpoint` (connecting on first use, and again after the app restarts).
///
/// # Errors
/// Reading or writing the pipe failed.
pub async fn serve<R, W>(mut input: R, mut output: W, endpoint: &Endpoint) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut client: Option<ControlClient> = None;
    while let Some(message) = read_message(&mut input).await? {
        let Ok(request) = serde_json::from_value::<Request>(message) else {
            write_message(
                &mut output,
                &error(&Value::Null, "invalid", "Not a request."),
            )
            .await?;
            continue;
        };
        if client.is_none() {
            match ControlClient::connect(endpoint, client_info()).await {
                Ok(connected) => client = Some(connected),
                Err(err) => {
                    write_message(&mut output, &client_error(&request.id, &err)).await?;
                    continue;
                }
            }
        }
        let Some(connected) = client.as_ref() else {
            continue;
        };
        let reply = answer(connected, request).await;
        // A broken connection (the app quit) is opened again next time.
        if reply
            .pointer("/error/code")
            .and_then(Value::as_str)
            .is_some_and(|c| c == "failed" || c == "appNotRunning")
        {
            client = None;
        }
        write_message(&mut output, &reply).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
