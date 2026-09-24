//! The menu bar extra: a template icon (tinted by macOS for light/dark menu bars) with
//! a native menu listing running Quick Shares and their actions.

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
    quick_share::{QuickShare, ShareStatus},
    text::{Text, msg::tray as m},
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

/// What the menu currently lists (rebuilt when either part changes).
#[derive(Default)]
struct TrayModel(std::sync::Mutex<(Vec<QuickShare>, TrayRoutes)>);

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
        .menu(&build_menu(app, &[], &TrayRoutes::default())?)
        // macOS and Linux (StatusNotifierItem) show the menu on click. On Windows a
        // left click opens the app and the menu is on the right click, as there.
        .show_menu_on_left_click(!cfg!(target_os = "windows"))
        .on_tray_icon_event(|tray, event| {
            if cfg!(target_os = "windows")
                && let tauri::tray::TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    button_state: tauri::tray::MouseButtonState::Up,
                    ..
                } = event
            {
                windows::focus_main(tray.app_handle());
            }
        })
        .build(app)?;
    VISIBLE.store(visible, std::sync::atomic::Ordering::SeqCst);
    tray.set_visible(visible)
}

/// Whether the icon is meant to show (the "Show in menu bar" setting, once installed).
static VISIBLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the menu bar (tray) icon is showing.
pub fn is_visible() -> bool {
    VISIBLE.load(std::sync::atomic::Ordering::SeqCst)
}

/// Shows or hides the menu bar icon (the "Show in menu bar" setting).
pub fn set_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    // Without an icon (Linux without an indicator library) it can never show.
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    VISIBLE.store(visible, std::sync::atomic::Ordering::SeqCst);
    if let Err(err) = tray.set_visible(visible) {
        tracing::warn!(error = %err, "failed to change menu bar icon visibility");
    }
}

/// Rebuilds the menu for the current Quick Shares.
pub fn refresh<R: Runtime>(app: &AppHandle<R>, shares: &[QuickShare]) {
    update(app, |model| model.0 = shares.to_vec());
}

/// Rebuilds the menu for the current routes.
pub fn set_routes<R: Runtime>(app: &AppHandle<R>, routes: TrayRoutes) {
    update(app, |model| model.1 = routes);
}

fn update<R: Runtime>(app: &AppHandle<R>, change: impl FnOnce(&mut (Vec<QuickShare>, TrayRoutes))) {
    let Some(model) = app.try_state::<TrayModel>() else {
        return;
    };
    let (shares, routes) = {
        let mut guard = model
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        change(&mut guard);
        guard.clone()
    };
    let Some(tray) = app.tray_by_id(ID) else {
        return;
    };
    // A dot on the icon when a route isn't working (template images stay monochrome).
    let icon = if routes.alert() {
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
    match build_menu(app, &shares, &routes) {
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

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    shares: &[QuickShare],
    model: &TrayRoutes,
) -> tauri::Result<Menu<R>> {
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
    MenuBuilder::new(app)
        .items(&route_section)
        .item(&header)
        .items(&submenu_refs)
        .item(&MenuItemBuilder::with_id(NEW_SHARE, m::share_local_port().to_string()).build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(OPEN, m::open().to_string()).build(app)?)
        .separator()
        .item(&PredefinedMenuItem::quit(
            app,
            Some(&m::quit().to_string()),
        )?)
        .build()
}

/// Handles a tray menu item. Returns `false` if `id` isn't a tray item.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, id: &str) -> bool {
    if id == OPEN {
        windows::focus_main(app);
    } else if id == NEW_SHARE {
        windows::focus_main(app);
        let _ = MenuAction {
            command: MenuCommand::NewQuickShare,
        }
        .emit(app);
    } else if let Some(share) = id.strip_prefix(COPY) {
        if let Some(url) = share_url(app, share)
            && let Err(err) = app.clipboard().write_text(url)
        {
            tracing::warn!(error = %err, "failed to copy URL");
        }
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
