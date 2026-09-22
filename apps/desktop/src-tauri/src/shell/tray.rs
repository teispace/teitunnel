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
use teitunnel_core::quick_share::{QuickShare, ShareStatus};

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

/// A route as the menu shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayRoute {
    /// Hostname.
    pub hostname: String,
    /// Short status, e.g. "Live" or "Connector stopped".
    pub status: String,
}

/// What the menu currently lists (rebuilt when either part changes).
#[derive(Default)]
struct TrayModel(std::sync::Mutex<(Vec<QuickShare>, Vec<TrayRoute>)>);

/// Adds the menu bar icon, hidden unless `visible`.
pub fn install<R: Runtime>(app: &AppHandle<R>, visible: bool) -> tauri::Result<()> {
    app.manage(TrayModel::default());
    let tray = TrayIconBuilder::with_id(ID)
        .icon(tauri::include_image!("icons/tray-template.png"))
        .icon_as_template(true)
        .tooltip("Teitunnel")
        .menu(&build_menu(app, &[], &[])?)
        .show_menu_on_left_click(true)
        .build(app)?;
    tray.set_visible(visible)
}

/// Shows or hides the menu bar icon (the "Show in menu bar" setting).
pub fn set_visible<R: Runtime>(app: &AppHandle<R>, visible: bool) {
    if let Some(tray) = app.tray_by_id(ID)
        && let Err(err) = tray.set_visible(visible)
    {
        tracing::warn!(error = %err, "failed to change menu bar icon visibility");
    }
}

/// Rebuilds the menu for the current Quick Shares.
pub fn refresh<R: Runtime>(app: &AppHandle<R>, shares: &[QuickShare]) {
    update(app, |model| model.0 = shares.to_vec());
}

/// Rebuilds the menu for the current routes.
pub fn set_routes<R: Runtime>(app: &AppHandle<R>, routes: Vec<TrayRoute>) {
    update(app, |model| model.1 = routes);
}

fn update<R: Runtime>(
    app: &AppHandle<R>,
    change: impl FnOnce(&mut (Vec<QuickShare>, Vec<TrayRoute>)),
) {
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
    match build_menu(app, &shares, &routes) {
        Ok(menu) => {
            if let Err(err) = tray.set_menu(Some(menu)) {
                tracing::warn!(error = %err, "failed to update the menu bar menu");
            }
        }
        Err(err) => tracing::warn!(error = %err, "failed to build the menu bar menu"),
    }
}

fn status_label(status: &ShareStatus) -> &'static str {
    match status {
        ShareStatus::Starting => "Starting",
        ShareStatus::Live => "Live",
        ShareStatus::Reconnecting => "Reconnecting",
        ShareStatus::Failed { .. } => "Failed",
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
            &MenuItemBuilder::with_id(format!("{COPY}{}", share.id), "Copy URL")
                .enabled(has_url)
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id(format!("{OPEN_URL}{}", share.id), "Open in Browser")
                .enabled(share.status == ShareStatus::Live)
                .build(app)?,
        )
        .separator()
        .item(&MenuItemBuilder::with_id(format!("{STOP}{}", share.id), "Stop Sharing").build(app)?)
        .build()
}

fn route_submenu<R: Runtime>(app: &AppHandle<R>, route: &TrayRoute) -> tauri::Result<Submenu<R>> {
    SubmenuBuilder::new(app, format!("{} — {}", route.hostname, route.status))
        .item(
            &MenuItemBuilder::with_id(format!("{ROUTE_COPY}{}", route.hostname), "Copy URL")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id(format!("{ROUTE_OPEN}{}", route.hostname), "Open in Browser")
                .build(app)?,
        )
        .build()
}

fn build_menu<R: Runtime>(
    app: &AppHandle<R>,
    shares: &[QuickShare],
    routes: &[TrayRoute],
) -> tauri::Result<Menu<R>> {
    let route_menus = routes
        .iter()
        .map(|route| route_submenu(app, route))
        .collect::<tauri::Result<Vec<_>>>()?;
    let route_refs: Vec<&dyn IsMenuItem<R>> = route_menus
        .iter()
        .map(|s| s as &dyn IsMenuItem<R>)
        .collect();
    let routes_header = MenuItemBuilder::new("Routes").enabled(false).build(app)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let mut route_section: Vec<&dyn IsMenuItem<R>> = Vec::new();
    if !routes.is_empty() {
        route_section.push(&routes_header);
        route_section.extend(route_refs);
        route_section.push(&separator);
    }
    let header = MenuItemBuilder::new(if shares.is_empty() {
        "No Quick Shares"
    } else {
        "Quick Shares"
    })
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
        .item(&MenuItemBuilder::with_id(NEW_SHARE, "Share a Local Port…").build(app)?)
        .separator()
        .item(&MenuItemBuilder::with_id(OPEN, "Open Teitunnel").build(app)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Quit Teitunnel"))?)
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
