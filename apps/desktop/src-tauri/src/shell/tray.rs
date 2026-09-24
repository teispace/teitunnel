//! The menu bar extra: a template icon (tinted by macOS for light/dark menu bars) with
//! a native menu listing routes, Quick Shares and their actions, running services to
//! share in one click, recent addresses to copy, and pause/stop for every share. What
//! the menu lists and what a click does comes from `teitunnel_core::quick_actions`.

use std::sync::{
    PoisonError,
    atomic::{AtomicU64, Ordering},
};

use tauri::{
    AppHandle, Manager, Runtime,
    menu::{
        IsMenuItem, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, Submenu, SubmenuBuilder,
    },
    tray::TrayIconBuilder,
};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;
use teitunnel_core::{
    engine::RouteHealth,
    quick_actions::{self, PauseAll, ServiceItem, ShareNow},
    quick_share::{QuickShare, ShareStatus},
    text::{
        Text,
        msg::{notify as n, tray as m},
    },
};

use crate::{
    ipc::{MenuAction, MenuCommand},
    shell::windows,
    state::AppState,
};

const ID: &str = "main";
const OPEN: &str = "tray.open";
const NEW_SHARE: &str = "tray.new_share";
const COPY: &str = "tray.copy:";
const OPEN_URL: &str = "tray.open_url:";
const STOP: &str = "tray.stop:";
const ROUTE_COPY: &str = "tray.route_copy:";
const ROUTE_OPEN: &str = "tray.route_open:";
const ROUTES_TOGGLE: &str = "tray.routes_toggle";
const SHARE_SERVICE: &str = "tray.share_service:";
const RECENT: &str = "tray.recent:";
const PAUSE_ALL: &str = "tray.pause_all";
const RESUME_ALL: &str = "tray.resume_all";
const STOP_ALL: &str = "tray.stop_all";

/// Running services are looked up again when the menu is about to open, at most this
/// often (milliseconds).
const SERVICES_REFRESH_MS: u64 = 3000;

/// A route as the menu shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayRoute {
    /// Hostname.
    pub hostname: String,
    /// How it's doing.
    pub status: RouteHealth,
}

/// This Mac's connectors, as one switch in the menu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TrayConnectors {
    /// No tunnel on this Mac.
    #[default]
    None,
    /// At least one connector runs (or is restarting).
    Running,
    /// None runs; `on_purpose` when the user stopped them.
    Stopped {
        /// The user stopped them (so it's not a problem to flag).
        on_purpose: bool,
    },
}

/// Routes on this Mac as the menu shows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrayRoutes {
    /// Every route, with its status.
    pub routes: Vec<TrayRoute>,
    /// This Mac's connectors.
    pub connectors: TrayConnectors,
}

impl TrayRoutes {
    /// Whether the icon should show a problem: a route isn't working and it's not
    /// because the user stopped the connectors.
    fn alert(&self) -> bool {
        !self.routes.iter().all(|r| r.status.is_live())
            && self.connectors != (TrayConnectors::Stopped { on_purpose: true })
    }

    /// The switch's title, if there's anything to switch.
    fn toggle_title(&self) -> Option<Text> {
        match self.connectors {
            TrayConnectors::None => None,
            TrayConnectors::Running => Some(m::stop_routes()),
            TrayConnectors::Stopped { .. } => Some(m::start_routes()),
        }
    }
}

/// Everything the menu lists (rebuilt when any part changes).
#[derive(Debug, Clone)]
struct Contents {
    shares: Vec<QuickShare>,
    routes: TrayRoutes,
    services: Vec<ServiceItem>,
    pause: PauseAll,
}

impl Default for Contents {
    fn default() -> Self {
        Self {
            shares: Vec::new(),
            routes: TrayRoutes::default(),
            services: Vec::new(),
            pause: PauseAll::Unavailable,
        }
    }
}

/// What the menu currently lists.
#[derive(Default)]
struct TrayModel(std::sync::Mutex<Contents>);

/// When running services were last looked up (milliseconds since the epoch).
static SERVICES_AT: AtomicU64 = AtomicU64::new(0);

/// Adds the menu bar icon, hidden unless `visible`.
pub fn install<R: Runtime>(app: &AppHandle<R>, visible: bool) -> tauri::Result<()> {
    app.manage(TrayModel::default());
    #[cfg(target_os = "linux")]
    if !indicator::available() {
        // Creating the icon would abort the app; run without one (closing the window then
        // quits, see `is_visible`).
        tracing::warn!("no libayatana-appindicator3 or libappindicator3: no tray icon");
        return Ok(());
    }
    let tray = TrayIconBuilder::with_id(ID)
        .icon(tauri::include_image!("icons/tray-template.png"))
        .icon_as_template(true)
        .tooltip("Teitunnel")
        .menu(&build_menu(app, &Contents::default())?)
        // macOS and Linux (StatusNotifierItem) show the menu on click. On Windows a
        // left click opens the app and the menu is on the right click, as there.
        .show_menu_on_left_click(!cfg!(target_os = "windows"))
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::TrayIconEvent;
            // Hovering the icon (macOS, Windows) looks up running services, so the menu
            // lists them when it opens. Linux reports no tray events: there they're
            // looked up at launch and when shares change.
            if matches!(event, TrayIconEvent::Enter { .. }) {
                refresh_services(tray.app_handle());
            }
            if cfg!(target_os = "windows")
                && let TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    button_state: tauri::tray::MouseButtonState::Up,
                    ..
                } = event
            {
                windows::focus_main(tray.app_handle());
            }
        })
        .build(app)?;
    VISIBLE.store(visible, Ordering::SeqCst);
    tray.set_visible(visible)
}

/// Whether the icon is meant to show (the "Show in menu bar" setting, once installed).
static VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the menu bar (tray) icon is showing.
pub fn is_visible() -> bool {
    VISIBLE.load(Ordering::SeqCst)
}

/// Shows or hides the menu bar icon (the "Show in menu bar" setting).
pub fn set_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    // Without an icon (Linux without an indicator library) it can never show.
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    VISIBLE.store(visible, Ordering::SeqCst);
    if let Err(err) = tray.set_visible(visible) {
        tracing::warn!(error = %err, "failed to change menu bar icon visibility");
    }
}

/// Rebuilds the menu for the current Quick Shares.
pub fn refresh<R: Runtime>(app: &AppHandle<R>, shares: &[QuickShare]) {
    let pause = app
        .try_state::<AppState>()
        .map_or(PauseAll::Unavailable, |state| {
            quick_actions::pause_state(&state.inspector, shares)
        });
    update(app, |model| {
        model.shares = shares.to_vec();
        model.pause = pause;
        quick_actions::drop_shared(&mut model.services, shares);
    });
    if cfg!(target_os = "linux") {
        refresh_services(app);
    }
}

/// Rebuilds the menu for the current routes.
pub fn set_routes<R: Runtime>(app: &AppHandle<R>, routes: TrayRoutes) {
    update(app, |model| model.routes = routes);
}

fn now_ms() -> u64 {
    teitunnel_core::domain_shares::now_ms()
}

/// Looks up running services (off the async threads), unless it just did.
pub(crate) fn refresh_services<R: Runtime>(app: &AppHandle<R>) {
    let now = now_ms();
    let last = SERVICES_AT.load(Ordering::SeqCst);
    if now.saturating_sub(last) < SERVICES_REFRESH_MS
        || SERVICES_AT
            .compare_exchange(last, now, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let services = teitunnel_core::discovery::services().await;
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let items = quick_actions::menu_services(&services, &state.quick_shares.list());
        let changed = app.try_state::<TrayModel>().is_some_and(|model| {
            model
                .0
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .services
                != items
        });
        if changed {
            update(&app, |model| model.services = items);
        }
    });
}

fn update<R: Runtime>(app: &AppHandle<R>, change: impl FnOnce(&mut Contents)) {
    let Some(model) = app.try_state::<TrayModel>() else {
        return;
    };
    let contents = {
        let mut guard = model.0.lock().unwrap_or_else(PoisonError::into_inner);
        change(&mut guard);
        guard.clone()
    };
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    // A dot on the icon when a route isn't working (template images stay monochrome).
    let icon = if contents.routes.alert() {
        tauri::include_image!("icons/tray-template-alert.png")
    } else {
        tauri::include_image!("icons/tray-template.png")
    };
    if let Err(err) = tray
        .set_icon(Some(icon))
        .and_then(|()| tray.set_icon_as_template(true))
    {
        tracing::warn!(error = %err, "failed to update the menu bar icon");
    }
    match build_menu(app, &contents) {
        Ok(menu) => {
            if let Err(err) = tray.set_menu(Some(menu)) {
                tracing::warn!(error = %err, "failed to update the menu bar menu");
            }
        }
        Err(err) => tracing::warn!(error = %err, "failed to build the menu bar menu"),
    }
}

fn status_label(status: &ShareStatus) -> Text {
    match status {
        ShareStatus::Starting => m::share_starting(),
        ShareStatus::Live => m::share_live(),
        ShareStatus::Reconnecting => m::share_reconnecting(),
        ShareStatus::Failed { .. } => m::share_failed(),
    }
}

fn share_submenu<R: Runtime>(app: &AppHandle<R>, share: &QuickShare) -> tauri::Result<Submenu<R>> {
    let origin = share
        .origin
        .as_str()
        .split("://")
        .nth(1)
        .unwrap_or(share.origin.as_str());
    let has_url = share.url.is_some();
    SubmenuBuilder::new(app, format!("{origin} — {}", status_label(&share.status)))
        .item(
            &MenuItemBuilder::with_id(format!("{COPY}{}", share.id), m::copy_url().to_string())
                .enabled(has_url)
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id(
                format!("{OPEN_URL}{}", share.id),
                m::open_in_browser().to_string(),
            )
            .enabled(share.status == ShareStatus::Live)
            .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id(format!("{STOP}{}", share.id), m::stop_sharing().to_string())
                .build(app)?,
        )
        .build()
}

fn route_submenu<R: Runtime>(app: &AppHandle<R>, route: &TrayRoute) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, format!("{} — {}", route.hostname, route.status.text()))
        .item(
            &MenuItemBuilder::with_id(
                format!("{ROUTE_COPY}{}", route.hostname),
                m::copy_url().to_string(),
            )
            .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id(
                format!("{ROUTE_OPEN}{}", route.hostname),
                m::open_in_browser().to_string(),
            )
            .build(app)?,
        )
        .build()
}

/// One line summarising the routes, e.g. "All 3 routes live" or "1 of 3 routes down".
fn health_line(model: &TrayRoutes) -> String {
    if model.connectors == (TrayConnectors::Stopped { on_purpose: true }) {
        return m::routes_stopped().to_string();
    }
    let routes = &model.routes;
    let live = routes.iter().filter(|r| r.status.is_live()).count() as u64;
    let total = routes.len() as u64;
    if live == total {
        if total == 1 {
            m::route_live().to_string()
        } else {
            m::all_live(total).to_string()
        }
    } else {
        m::not_working(total, total - live).to_string()
    }
}

/// A URL without its scheme, for a menu item.
fn short_url(url: &str) -> &str {
    url.split("://").nth(1).unwrap_or(url).trim_end_matches('/')
}

/// The "all shares" items: pause or resume, and stop.
fn all_share_items(contents: &Contents) -> Vec<(&'static str, Text)> {
    let mut items = Vec::new();
    match contents.pause {
        PauseAll::Unavailable => {}
        PauseAll::Pause => items.push((PAUSE_ALL, m::pause_all())),
        PauseAll::Resume => items.push((RESUME_ALL, m::resume_all())),
    }
    if !contents.shares.is_empty() {
        items.push((STOP_ALL, m::stop_all()));
    }
    items
}

fn build_menu<R: Runtime>(app: &AppHandle<R>, contents: &Contents) -> tauri::Result<Menu<R>> {
    let model = &contents.routes;
    let shares = &contents.shares;
    let routes = &model.routes;
    let route_menus = routes
        .iter()
        .map(|route| route_submenu(app, route))
        .collect::<tauri::Result<Vec<_>>>()?;
    let route_refs: Vec<&dyn IsMenuItem<R>> = route_menus
        .iter()
        .map(|s| s as &dyn IsMenuItem<R>)
        .collect();
    let routes_header = MenuItemBuilder::new(health_line(model))
        .enabled(false)
        .build(app)?;
    let toggle = model
        .toggle_title()
        .map(|title| MenuItemBuilder::with_id(ROUTES_TOGGLE, title.to_string()).build(app))
        .transpose()?;
    let separator = PredefinedMenuItem::separator(app)?;
    let mut route_section: Vec<&dyn IsMenuItem<R>> = Vec::new();
    if !routes.is_empty() {
        route_section.push(&routes_header);
        route_section.extend(route_refs);
    }
    if let Some(toggle) = &toggle {
        route_section.push(toggle);
    }
    if !route_section.is_empty() {
        route_section.push(&separator);
    }
    let header = MenuItemBuilder::new(
        if shares.is_empty() {
            m::no_shares()
        } else {
            m::shares()
        }
        .to_string(),
    )
    .enabled(false)
    .build(app)?;
    let submenus = shares
        .iter()
        .map(|share| share_submenu(app, share))
        .collect::<tauri::Result<Vec<_>>>()?;
    let submenu_refs: Vec<&dyn IsMenuItem<R>> =
        submenus.iter().map(|s| s as &dyn IsMenuItem<R>).collect();
    // One click copies a live share's address, newest first.
    let recent = quick_actions::recent_urls(shares)
        .into_iter()
        .map(|(id, url)| {
            MenuItemBuilder::with_id(
                format!("{RECENT}{id}"),
                m::copy_recent(short_url(&url)).to_string(),
            )
            .build(app)
        })
        .collect::<tauri::Result<Vec<_>>>()?;
    let recent_refs: Vec<&dyn IsMenuItem<R>> =
        recent.iter().map(|i| i as &dyn IsMenuItem<R>).collect();
    let all = all_share_items(contents)
        .into_iter()
        .map(|(id, title)| MenuItemBuilder::with_id(id, title.to_string()).build(app))
        .collect::<tauri::Result<Vec<_>>>()?;
    let all_refs: Vec<&dyn IsMenuItem<R>> = all.iter().map(|i| i as &dyn IsMenuItem<R>).collect();
    let services = if contents.services.is_empty() {
        None
    } else {
        let mut submenu = SubmenuBuilder::new(app, m::share_service().to_string());
        for service in &contents.services {
            submenu = submenu.item(
                &MenuItemBuilder::with_id(
                    format!("{SHARE_SERVICE}{}", service.origin),
                    &service.label,
                )
                .build(app)?,
            );
        }
        Some(submenu.build()?)
    };
    let mut menu = MenuBuilder::new(app)
        .items(&route_section)
        .item(&header)
        .items(&submenu_refs);
    if !recent_refs.is_empty() || !all_refs.is_empty() {
        menu = menu
            .separator()
            .items(&recent_refs)
            .items(&all_refs)
            .separator();
    }
    if let Some(services) = &services {
        menu = menu.item(services);
    }
    menu.item(&MenuItemBuilder::with_id(NEW_SHARE, m::share_local_port().to_string()).build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(OPEN, m::open().to_string()).build(app)?)
        .separator()
        .item(&PredefinedMenuItem::quit(
            app,
            Some(&m::quit().to_string()),
        )?)
        .build()
}

/// Brings the window forward with the Quick Share sheet open.
pub(crate) fn open_quick_share<R: Runtime>(app: &AppHandle<R>) {
    windows::focus_main(app);
    let _ = MenuAction {
        command: MenuCommand::NewQuickShare,
    }
    .emit(app);
}

/// Copies a share's address and says so.
pub(crate) fn copy_address<R: Runtime>(app: &AppHandle<R>, url: &str) {
    match app.clipboard().write_text(url) {
        Ok(()) => crate::bootstrap::notify(app, &n::address_copied(), &n::address_copied_body(url)),
        Err(err) => tracing::warn!(error = %err, "failed to copy URL"),
    }
}

/// Shares `origin` without the window (a menu click or the global shortcut): the
/// exposure check first, then the address is copied. Findings open the Quick Share
/// sheet so the person decides there.
pub(crate) fn share_from_outside<R: Runtime>(app: &AppHandle<R>, origin: String) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        match quick_actions::share_now(&state.quick_shares, &state.store, &origin).await {
            ShareNow::Live(share) => {
                let url = share.url.unwrap_or_default();
                if let Err(err) = app.clipboard().write_text(&url) {
                    tracing::warn!(error = %err, "failed to copy URL");
                }
                crate::bootstrap::notify(
                    &app,
                    &n::shared(short_url(share.origin.as_str())),
                    &n::shared_body(&url),
                );
            }
            ShareNow::NeedsReview(report) => {
                tracing::info!(
                    findings = report.findings.len(),
                    "the exposure check found something; asking in the window"
                );
                open_quick_share(&app);
            }
            ShareNow::Failed(message) => {
                crate::bootstrap::notify(&app, &n::share_now_failed(short_url(&origin)), &message);
            }
        }
    });
}

/// Handles a tray menu item. Returns `false` if `id` isn't a tray item.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    if id == OPEN {
        windows::focus_main(app);
    } else if id == NEW_SHARE {
        open_quick_share(app);
    } else if let Some(share) = id.strip_prefix(COPY) {
        if let Some(url) = share_url(app, share)
            && let Err(err) = app.clipboard().write_text(url)
        {
            tracing::warn!(error = %err, "failed to copy URL");
        }
    } else if let Some(share) = id.strip_prefix(RECENT) {
        if let Some(url) = share_url(app, share)
            && let Err(err) = app.clipboard().write_text(url)
        {
            tracing::warn!(error = %err, "failed to copy URL");
        }
    } else if let Some(origin) = id.strip_prefix(SHARE_SERVICE) {
        share_from_outside(app, origin.to_owned());
    } else if let Some(share) = id.strip_prefix(OPEN_URL) {
        if let Some(url) = share_url(app, share)
            && let Err(err) = app.opener().open_url(url, None::<&str>)
        {
            tracing::warn!(error = %err, "failed to open URL");
        }
    } else if let Some(host) = id.strip_prefix(ROUTE_COPY) {
        if let Err(err) = app.clipboard().write_text(format!("https://{host}")) {
            tracing::warn!(error = %err, "failed to copy URL");
        }
    } else if let Some(host) = id.strip_prefix(ROUTE_OPEN) {
        if let Err(err) = app
            .opener()
            .open_url(format!("https://{host}"), None::<&str>)
        {
            tracing::warn!(error = %err, "failed to open URL");
        }
    } else if id == ROUTES_TOGGLE {
        crate::bootstrap::toggle_machine_routes(app);
    } else if id == PAUSE_ALL || id == RESUME_ALL {
        let Some(state) = app.try_state::<AppState>() else {
            return true;
        };
        let shares = state.quick_shares.list();
        quick_actions::set_all_paused(&state.inspector, &shares, id == PAUSE_ALL);
        refresh(app, &shares);
    } else if id == STOP_ALL {
        let Some(state) = app.try_state::<AppState>() else {
            return true;
        };
        let shares = state.quick_shares.clone();
        tauri::async_runtime::spawn(async move { shares.stop_all().await });
    } else if let Some(share) = id.strip_prefix(STOP) {
        let Some(state) = app.try_state::<AppState>() else {
            return true;
        };
        let shares = state.quick_shares.clone();
        let share = share.to_owned();
        tauri::async_runtime::spawn(async move {
            let _ = shares.stop(&share).await;
        });
    } else {
        return false;
    }
    true
}

fn share_url<R: Runtime>(app: &AppHandle<R>, id: &str) -> Option<String> {
    let state = app.try_state::<AppState>()?;
    state
        .quick_shares
        .list()
        .into_iter()
        .find(|s| s.id == id)?
        .url
}

#[cfg(test)]
mod tests {
    use super::*;

    use RouteHealth::{Live, NoDns, Stopped};

    fn route(status: RouteHealth) -> TrayRoute {
        TrayRoute {
            hostname: "a.xyz.com".into(),
            status,
        }
    }

    fn model(routes: &[RouteHealth], connectors: TrayConnectors) -> TrayRoutes {
        TrayRoutes {
            routes: routes.iter().map(|s| route(*s)).collect(),
            connectors,
        }
    }

    #[test]
    fn summarises_route_health() {
        let running = TrayConnectors::Running;
        assert_eq!(health_line(&model(&[Live], running)), "Route live");
        assert_eq!(
            health_line(&model(&[Live, Live], running)),
            "All 2 routes live"
        );
        assert_eq!(
            health_line(&model(&[Live, Stopped, NoDns], running)),
            "2 of 3 routes not working"
        );
    }

    #[test]
    fn stopping_on_purpose_is_not_a_problem() {
        let stopped = TrayConnectors::Stopped { on_purpose: true };
        let down = model(&[Stopped], stopped);
        // The wording names the platform ("this Mac", "this PC"…).
        assert_eq!(health_line(&down), m::routes_stopped().english());
        assert!(!down.alert());
        assert_eq!(
            down.toggle_title().map(|t| t.english()),
            Some(m::start_routes().english())
        );

        let crashed = model(&[Stopped], TrayConnectors::Stopped { on_purpose: false });
        assert!(crashed.alert());
        assert_eq!(
            model(&[Live], TrayConnectors::Running)
                .toggle_title()
                .map(|t| t.english()),
            Some(m::stop_routes().english())
        );
        assert_eq!(model(&[], TrayConnectors::None).toggle_title(), None);
    }
}

/// Linux shows tray icons through libayatana-appindicator (or the older libappindicator),
/// which the tray loads when the icon is created and aborts the app if it's missing. The
/// packages depend on it, but an AppImage or a trimmed system may not have it, so look for
/// it first where the dynamic loader would.
#[cfg(any(target_os = "linux", all(test, unix)))]
mod indicator {
    use std::path::{Path, PathBuf};

    const LIBRARIES: [&str; 4] = [
        "libayatana-appindicator3.so.1",
        "libappindicator3.so.1",
        "libayatana-appindicator3.so",
        "libappindicator3.so",
    ];

    #[cfg(target_os = "linux")]
    const SYSTEM_DIRS: [&str; 10] = [
        "/usr/lib",
        "/usr/lib64",
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/lib",
        "/lib64",
        "/lib/x86_64-linux-gnu",
        "/lib/aarch64-linux-gnu",
        "/usr/local/lib",
        "/usr/local/lib64",
    ];

    /// Whether an indicator library is installed.
    #[cfg(target_os = "linux")]
    pub(super) fn available() -> bool {
        let mut dirs = env_dirs(
            std::env::var_os("LD_LIBRARY_PATH").as_deref(),
            std::env::var_os("APPDIR").as_deref(),
        );
        dirs.extend(ld_so_conf_dirs(Path::new("/etc/ld.so.conf.d")));
        dirs.extend(SYSTEM_DIRS.iter().map(PathBuf::from));
        found_in(&dirs)
    }

    /// `LD_LIBRARY_PATH`, and an AppImage's own libraries.
    pub(super) fn env_dirs(
        ld_library_path: Option<&std::ffi::OsStr>,
        appdir: Option<&std::ffi::OsStr>,
    ) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = ld_library_path
            .map(|p| std::env::split_paths(p).collect())
            .unwrap_or_default();
        if let Some(appdir) = appdir {
            let appdir = Path::new(appdir);
            dirs.push(appdir.join("usr/lib"));
            dirs.push(appdir.join("usr/lib64"));
        }
        dirs
    }

    /// Directories listed in the loader's `*.conf` files (absolute paths only).
    pub(super) fn ld_so_conf_dirs(conf_d: &Path) -> Vec<PathBuf> {
        let Ok(entries) = std::fs::read_dir(conf_d) else {
            return Vec::new();
        };
        let mut files: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "conf"))
            .collect();
        files.sort();
        files
            .iter()
            .filter_map(|f| std::fs::read_to_string(f).ok())
            .flat_map(|text| {
                text.lines()
                    .map(|l| l.split('#').next().unwrap_or_default().trim().to_owned())
                    .filter(|l| l.starts_with('/'))
                    .map(PathBuf::from)
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Whether any of the libraries is in any of the directories.
    pub(super) fn found_in(dirs: &[PathBuf]) -> bool {
        dirs.iter()
            .any(|dir| LIBRARIES.iter().any(|lib| dir.join(lib).exists()))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn finds_the_library_in_any_listed_directory() {
            let empty = tempfile::tempdir().unwrap();
            let lib = tempfile::tempdir().unwrap();
            let dirs = [empty.path().to_owned(), lib.path().to_owned()];
            assert!(!found_in(&dirs));
            std::fs::write(lib.path().join("libayatana-appindicator3.so.1"), b"").unwrap();
            assert!(found_in(&dirs));
        }

        #[test]
        fn reads_the_loader_configuration() {
            let conf = tempfile::tempdir().unwrap();
            std::fs::write(
                conf.path().join("x86_64-linux-gnu.conf"),
                "# Multiarch support\n/usr/local/lib/x86_64-linux-gnu\n/lib/x86_64-linux-gnu # trailing\n",
            )
            .unwrap();
            std::fs::write(conf.path().join("other.txt"), "/ignored\n").unwrap();
            std::fs::write(
                conf.path().join("include.conf"),
                "include /etc/more/*.conf\n",
            )
            .unwrap();
            assert_eq!(
                ld_so_conf_dirs(conf.path()),
                [
                    PathBuf::from("/usr/local/lib/x86_64-linux-gnu"),
                    PathBuf::from("/lib/x86_64-linux-gnu"),
                ]
            );
            assert!(ld_so_conf_dirs(&conf.path().join("missing")).is_empty());
        }

        #[test]
        fn includes_ld_library_path_and_the_appimage() {
            let dirs = env_dirs(Some("/a:/b".as_ref()), Some("/tmp/.mount_x".as_ref()));
            assert_eq!(
                dirs,
                [
                    "/a",
                    "/b",
                    "/tmp/.mount_x/usr/lib",
                    "/tmp/.mount_x/usr/lib64"
                ]
                .map(PathBuf::from)
            );
            assert!(env_dirs(None, None).is_empty());
        }
    }
}
