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
//! browser pointing at them (under `HKEY_CURRENT_USER`, written and read back directly).
//! Only browsers that are installed get one (their app is found, not just a profile
//! folder, which stays behind after uninstalling), and only Teitunnel's own manifest is
//! ever replaced or removed.
//!
//! Whether the extension itself works is known only when a browser starts the host:
//! it records which browser did, and when, in `browser-extension.json` in the data
//! folder ([`record_started`]), which the app shows.

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

    /// The browser with this id (`chrome`, `firefox`…).
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|b| {
            serde_json::to_value(b)
                .ok()
                .and_then(|v| v.as_str().map(str::to_ascii_lowercase))
                .as_deref()
                == Some(value.as_str())
        })
    }

    /// Where its app is, relative to the system's root (macOS app bundles are looked
    /// for in `/Applications` and `~/Applications` too).
    fn apps(self, platform: Platform) -> &'static [&'static str] {
        match (platform, self) {
            (Platform::MacOs, Self::Chrome) => &["Google Chrome.app"],
            (Platform::MacOs, Self::Chromium) => &["Chromium.app"],
            (Platform::MacOs, Self::Edge) => &["Microsoft Edge.app"],
            (Platform::MacOs, Self::Brave) => &["Brave Browser.app"],
            (Platform::MacOs, Self::Vivaldi) => &["Vivaldi.app"],
            (Platform::MacOs, Self::Arc) => &["Arc.app"],
            (Platform::MacOs, Self::Firefox) => &["Firefox.app"],
            (Platform::Linux, Self::Chrome) => &[
                "opt/google/chrome/chrome",
                "usr/bin/google-chrome",
                "usr/bin/google-chrome-stable",
            ],
            (Platform::Linux, Self::Chromium) => &[
                "usr/bin/chromium",
                "usr/bin/chromium-browser",
                "usr/lib/chromium/chromium",
                "snap/bin/chromium",
            ],
            (Platform::Linux, Self::Edge) => &[
                "opt/microsoft/msedge/msedge",
                "usr/bin/microsoft-edge",
                "usr/bin/microsoft-edge-stable",
            ],
            (Platform::Linux, Self::Brave) => {
                &["opt/brave.com/brave/brave", "usr/bin/brave-browser"]
            }
            (Platform::Linux, Self::Vivaldi) => &[
                "opt/vivaldi/vivaldi",
                "usr/bin/vivaldi",
                "usr/bin/vivaldi-stable",
            ],
            (Platform::Linux, Self::Firefox) => &[
                "usr/bin/firefox",
                "usr/lib/firefox/firefox",
                "usr/lib64/firefox/firefox",
                "opt/firefox/firefox",
                "snap/bin/firefox",
            ],
            // Relative to a Program Files folder or %LOCALAPPDATA%.
            (Platform::Windows, Self::Chrome) => &["Google/Chrome/Application/chrome.exe"],
            (Platform::Windows, Self::Chromium) => &["Chromium/Application/chrome.exe"],
            (Platform::Windows, Self::Edge) => &["Microsoft/Edge/Application/msedge.exe"],
            (Platform::Windows, Self::Brave) => {
                &["BraveSoftware/Brave-Browser/Application/brave.exe"]
            }
            (Platform::Windows, Self::Vivaldi) => &["Vivaldi/Application/vivaldi.exe"],
            (Platform::Windows, Self::Firefox) => &["Mozilla Firefox/firefox.exe"],
            (Platform::Windows | Platform::Linux, Self::Arc) => &[],
        }
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
    /// Its app: it's installed when one of these exists.
    apps: Vec<PathBuf>,
    /// Where the manifest goes.
    manifest: PathBuf,
    /// Windows: the registry key (under `HKEY_CURRENT_USER`) naming the manifest.
    registry: Option<String>,
}

impl Place {
    fn app(&self) -> Option<&PathBuf> {
        self.apps.iter().find(|p| p.exists())
    }
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
    /// It's installed on this computer (its app was found).
    pub detected: bool,
    /// Where its app was found.
    pub app: Option<String>,
    /// Teitunnel's manifest is there and points at `exe` (on Windows, with the
    /// registry key naming it): the browser can start the host.
    pub installed: bool,
    /// Where the manifest is (or goes).
    pub manifest: String,
    /// When this browser last started the host for the extension (milliseconds since
    /// the epoch): the extension is installed and working.
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub extension_seen_at: Option<u64>,
}

fn file_name() -> String {
    format!("{HOST_NAME}.json")
}

impl Layout {
    /// The folders under `home` (and, on Windows, the local and roaming app data), with
    /// apps looked for on this system.
    pub fn new(platform: Platform, home: &Path, local_data: &Path, roaming_data: &Path) -> Self {
        let program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
            .into_iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        Self::with_roots(
            platform,
            home,
            local_data,
            roaming_data,
            Path::new("/"),
            &program_files,
        )
    }

    /// [`Layout::new`] with apps looked for under `root` (macOS and Linux) or in
    /// `program_files` and `local_data` (Windows): tests use a folder of their own.
    pub fn with_roots(
        platform: Platform,
        home: &Path,
        local_data: &Path,
        roaming_data: &Path,
        root: &Path,
        program_files: &[PathBuf],
    ) -> Self {
        let apps = |browser: Browser| -> Vec<PathBuf> {
            let relative = browser.apps(platform);
            match platform {
                Platform::MacOs => [root.join("Applications"), home.join("Applications")]
                    .iter()
                    .flat_map(|dir| relative.iter().map(move |app| dir.join(app)))
                    .collect(),
                Platform::Linux => relative.iter().map(|p| root.join(p)).collect(),
                Platform::Windows => program_files
                    .iter()
                    .chain(std::iter::once(&local_data.to_path_buf()))
                    .flat_map(|dir| {
                        relative
                            .iter()
                            .map(move |p| p.split('/').fold(dir.clone(), |d, c| d.join(c)))
                    })
                    .collect(),
            }
        };
        let place = |browser: Browser, profile: PathBuf, hosts: &str| Place {
            browser,
            apps: apps(browser),
            manifest: profile.join(hosts).join(file_name()),
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
                let windows = |browser: Browser, key: &str| Place {
                    browser,
                    apps: apps(browser),
                    manifest: ours.join(if browser.firefox() {
                        "firefox.json"
                    } else {
                        "chromium.json"
                    }),
                    registry: Some(format!(r"Software\{key}\NativeMessagingHosts\{HOST_NAME}")),
                };
                let _ = roaming_data;
                vec![
                    windows(Browser::Chrome, r"Google\Chrome"),
                    windows(Browser::Chromium, "Chromium"),
                    windows(Browser::Edge, r"Microsoft\Edge"),
                    windows(Browser::Brave, r"BraveSoftware\Brave-Browser"),
                    windows(Browser::Vivaldi, "Vivaldi"),
                    windows(Browser::Firefox, "Mozilla"),
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
                detected: place.app().is_some(),
                app: place.app().map(|p| p.display().to_string()),
                installed: self.ready(place, exe),
                manifest: place.manifest.display().to_string(),
                extension_seen_at: None,
            })
            .collect()
    }

    /// Whether `place`'s browser would start `exe`.
    fn ready(&self, place: &Place, exe: &Path) -> bool {
        let manifest = ours(&place.manifest).is_some_and(|m| points_at(&m, exe));
        match &place.registry {
            Some(key) if self.platform == Platform::Windows => {
                manifest
                    && registry::get(key)
                        .is_some_and(|value| Path::new(&value) == place.manifest.as_path())
            }
            _ => manifest,
        }
    }

    /// The places of `only` (every installed browser when `None`).
    fn chosen(&self, only: Option<Browser>) -> impl Iterator<Item = &Place> {
        self.places
            .iter()
            .filter(move |p| only.is_none_or(|b| b == p.browser))
    }

    /// Writes the manifest for every installed browser (or `only` that one), pointing at
    /// `exe` (on Windows, with the registry keys). Returns the browsers' state after.
    ///
    /// # Errors
    /// A manifest or registry key couldn't be written.
    pub fn install(&self, exe: &Path, only: Option<Browser>) -> io::Result<Vec<BrowserHostStatus>> {
        for place in self.chosen(only).filter(|p| p.app().is_some()) {
            if let Some(parent) = place.manifest.parent() {
                fs::create_dir_all(parent)?;
            }
            // Something else by this name is left alone.
            if place.manifest.exists() && ours(&place.manifest).is_none() {
                continue;
            }
            let body = serde_json::to_vec_pretty(&manifest(place.browser, exe))?;
            fs::write(&place.manifest, body)?;
            if let Some(key) = &place.registry
                && self.platform == Platform::Windows
            {
                registry::set(key, &place.manifest.to_string_lossy())?;
            }
        }
        Ok(self.status(exe))
    }

    /// Removes Teitunnel's manifests (and registry keys), for every browser or `only` one.
    /// On Windows the Chromium browsers share one manifest: it stays while another
    /// browser's key still names it.
    ///
    /// # Errors
    /// A manifest couldn't be removed.
    pub fn uninstall(
        &self,
        exe: &Path,
        only: Option<Browser>,
    ) -> io::Result<Vec<BrowserHostStatus>> {
        for place in self.chosen(only) {
            if let Some(key) = &place.registry
                && self.platform == Platform::Windows
            {
                registry::remove(key);
            }
            let shared = self.places.iter().any(|other| {
                other.browser != place.browser
                    && other.manifest == place.manifest
                    && other
                        .registry
                        .as_deref()
                        .is_some_and(|key| registry::get(key).is_some())
            });
            if !shared && ours(&place.manifest).is_some() {
                match fs::remove_file(&place.manifest) {
                    Ok(()) => {}
                    Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                    Err(err) => return Err(err),
                }
            }
        }
        Ok(self.status(exe))
    }
}

/// `HKEY_CURRENT_USER` keys naming the manifests (Windows only; elsewhere nothing).
mod registry {
    /// The key's default value.
    #[cfg(windows)]
    pub(super) fn get(key: &str) -> Option<String> {
        windows_registry::CURRENT_USER
            .open(key)
            .ok()?
            .get_string("")
            .ok()
    }

    #[cfg(not(windows))]
    pub(super) fn get(_key: &str) -> Option<String> {
        None
    }

    /// Creates the key with `value` as its default value.
    #[cfg(windows)]
    pub(super) fn set(key: &str, value: &str) -> std::io::Result<()> {
        windows_registry::CURRENT_USER
            .create(key)
            .and_then(|k| k.set_string("", value))
            .map_err(std::io::Error::other)
    }

    #[cfg(not(windows))]
    pub(super) fn set(_key: &str, _value: &str) -> std::io::Result<()> {
        Ok(())
    }

    /// Deletes the key (already gone is fine).
    #[cfg(windows)]
    pub(super) fn remove(key: &str) {
        let _ = windows_registry::CURRENT_USER.remove_tree(key);
    }

    #[cfg(not(windows))]
    pub(super) fn remove(_key: &str) {}
}

/// The file in the data folder recording which browsers started the host, and when.
pub const SEEN_FILE: &str = "browser-extension.json";

/// When each browser last started the host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Seen {
    /// By browser (milliseconds since the epoch).
    pub browsers: std::collections::BTreeMap<Browser, u64>,
    /// A Chromium browser Teitunnel couldn't tell.
    pub other: Option<u64>,
}

/// Reads [`SEEN_FILE`] from `dir` (empty when there's none).
pub fn seen(dir: &Path) -> Seen {
    fs::read(dir.join(SEEN_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// Adds when each browser last started the host to `statuses`.
pub fn with_seen(mut statuses: Vec<BrowserHostStatus>, seen: &Seen) -> Vec<BrowserHostStatus> {
    for status in &mut statuses {
        status.extension_seen_at = seen.browsers.get(&status.browser).copied();
    }
    statuses
}

/// Records that a browser started the host at `now`: Firefox by its arguments, a
/// Chromium browser by the app that started this process (or its parent, when a shell
/// started it on Windows). Failures only lose the record.
pub fn record_started(dir: &Path, args: &[String], layout: &Layout, now: u64) {
    let browser = if args.iter().any(|a| a == FIREFOX_EXTENSION_ID) {
        Some(Browser::Firefox)
    } else {
        starting_browser(layout)
    };
    let mut record = seen(dir);
    match browser {
        Some(browser) => {
            record.browsers.insert(browser, now);
        }
        None => record.other = Some(now),
    }
    let path = dir.join(SEEN_FILE);
    let temp = dir.join(format!("{SEEN_FILE}.{}", std::process::id()));
    if let Ok(body) = serde_json::to_vec(&record)
        && fs::write(&temp, body).is_ok()
        && fs::rename(&temp, &path).is_err()
    {
        let _ = fs::remove_file(&temp);
    }
}

/// The browser whose app started this process, looking up to three processes up.
fn starting_browser(layout: &Layout) -> Option<Browser> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    let mut pid = system.process(Pid::from_u32(std::process::id()))?.parent();
    for _ in 0..3 {
        let process = system.process(pid?)?;
        if let Some(exe) = process.exe()
            && let Some(place) = layout
                .places
                .iter()
                .find(|p| p.apps.iter().any(|app| exe.starts_with(app) || exe == app))
        {
            return Some(place.browser);
        }
        pid = process.parent();
    }
    None
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
