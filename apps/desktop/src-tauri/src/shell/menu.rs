//! The macOS menu bar (DESIGN §9).
//!
//! Items the shell can handle itself (Settings, help links) are handled here; the rest
//! are forwarded to the webview as a typed [`MenuAction`] event. The Edit menu is not
//! optional: without it, ⌘C/⌘V/⌘A don't work in text fields on macOS.

use tauri::{AppHandle, Manager, Runtime, menu::MenuEvent};
// Building the menu bar is macOS-only (D-063); handling its items isn't (the tray uses
// the same event path).
#[cfg(target_os = "macos")]
use tauri::menu::{
    AboutMetadataBuilder, IsMenuItem, Menu, MenuBuilder, MenuItem, MenuItemBuilder, SubmenuBuilder,
};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;
use teitunnel_core::text::{Text, msg::menu as m};

use crate::{
    ipc::{MenuAction, MenuCommand, app::HelpLink},
    shell::{tray, windows},
};

const SETTINGS: &str = "settings";
const DOCS: &str = "help.docs";
const ISSUE: &str = "help.issue";
const CLOUDFLARE_DOCS: &str = "help.cloudflare_docs";
const RELEASES: &str = "help.releases";
const CHECK_UPDATES: &str = "app.check_updates";

/// A menu item the webview handles: (id, label, accelerator (empty: none), command).
type WebviewItem = (&'static str, fn() -> Text, &'static str, MenuCommand);

const FILE_ITEMS: &[WebviewItem] = &[
    (
        "file.new_route",
        m::new_route,
        "CmdOrCtrl+N",
        MenuCommand::NewRoute,
    ),
    (
        "file.new_quick_share",
        m::new_quick_share,
        "CmdOrCtrl+Shift+N",
        MenuCommand::NewQuickShare,
    ),
];

const GO_ITEMS: &[WebviewItem] = &[
    (
        "go.overview",
        m::overview,
        "CmdOrCtrl+1",
        MenuCommand::GoOverview,
    ),
    ("go.routes", m::routes, "CmdOrCtrl+2", MenuCommand::GoRoutes),
    (
        "go.quick_share",
        m::quick_share,
        "CmdOrCtrl+3",
        MenuCommand::GoQuickShare,
    ),
    (
        "go.domains",
        m::domains,
        "CmdOrCtrl+4",
        MenuCommand::GoDomains,
    ),
    (
        "go.tunnels",
        m::tunnels,
        "CmdOrCtrl+5",
        MenuCommand::GoTunnels,
    ),
    (
        "go.activity",
        m::activity,
        "CmdOrCtrl+6",
        MenuCommand::GoActivity,
    ),
    ("go.doctor", m::doctor, "CmdOrCtrl+7", MenuCommand::GoDoctor),
];

const VIEW_ITEMS: &[WebviewItem] = &[
    (
        "view.toggle_sidebar",
        m::toggle_sidebar,
        "CmdOrCtrl+Alt+S",
        MenuCommand::ToggleSidebar,
    ),
    (
        "view.toggle_inspector",
        m::toggle_inspector,
        "CmdOrCtrl+Alt+I",
        MenuCommand::ToggleInspector,
    ),
    (
        "view.palette",
        m::command_palette,
        "CmdOrCtrl+K",
        MenuCommand::CommandPalette,
    ),
    (
        "view.refresh",
        m::refresh,
        "CmdOrCtrl+R",
        MenuCommand::Refresh,
    ),
];

const HELP_ITEMS: &[WebviewItem] = &[
    ("help.doctor", m::check_problems, "", MenuCommand::GoDoctor),
    (
        "help.diagnostics",
        m::export_diagnostics,
        "",
        MenuCommand::ExportDiagnostics,
    ),
];

fn all_items() -> impl Iterator<Item = &'static WebviewItem> {
    FILE_ITEMS
        .iter()
        .chain(GO_ITEMS)
        .chain(VIEW_ITEMS)
        .chain(HELP_ITEMS)
}

/// Maps a menu item id to the command it forwards, if any.
fn command_for(id: &str) -> Option<MenuCommand> {
    all_items()
        .find(|(item, ..)| *item == id)
        .map(|(.., command)| *command)
}

#[cfg(target_os = "macos")]
fn build_items<R: Runtime>(
    app: &AppHandle<R>,
    table: &[WebviewItem],
) -> tauri::Result<Vec<MenuItem<R>>> {
    table
        .iter()
        .map(|(id, label, accelerator, _)| {
            let item = MenuItemBuilder::with_id(*id, label().to_string());
            if accelerator.is_empty() {
                item.build(app)
            } else {
                item.accelerator(*accelerator).build(app)
            }
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn as_refs<R: Runtime>(items: &[MenuItem<R>]) -> Vec<&dyn IsMenuItem<R>> {
    items
        .iter()
        .map(|item| item as &dyn IsMenuItem<R>)
        .collect()
}

/// Builds the application menu bar (macOS only, D-063).
#[cfg(target_os = "macos")]
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let info = app.package_info();
    let about = AboutMetadataBuilder::new()
        .name(Some(info.name.clone()))
        .version(Some(info.version.to_string()))
        .copyright(Some("© 2026 Teispace. MIT License."))
        .website(Some("https://github.com/teispace/teitunnel"))
        .credits(Some(m::credits().to_string()))
        .build();
    let name = &info.name;

    let app_menu = SubmenuBuilder::new(app, name)
        .about_with_text(m::about(name).to_string(), Some(about))
        .text(CHECK_UPDATES, m::check_updates().to_string())
        .separator()
        .item(
            &MenuItemBuilder::with_id(SETTINGS, m::settings().to_string())
                .accelerator("CmdOrCtrl+,")
                .build(app)?,
        )
        .separator()
        .services_with_text(m::services().to_string())
        .separator()
        .hide_with_text(m::hide(name).to_string())
        .hide_others_with_text(m::hide_others().to_string())
        .show_all_with_text(m::show_all().to_string())
        .separator()
        .quit_with_text(m::quit(name).to_string())
        .build()?;

    let file_items = build_items(app, FILE_ITEMS)?;
    let file = SubmenuBuilder::new(app, m::file().to_string())
        .items(&as_refs(&file_items))
        .separator()
        .close_window_with_text(m::close_window().to_string())
        .build()?;

    let edit = SubmenuBuilder::new(app, m::edit().to_string())
        .undo_with_text(m::undo().to_string())
        .redo_with_text(m::redo().to_string())
        .separator()
        .cut_with_text(m::cut().to_string())
        .copy_with_text(m::copy().to_string())
        .paste_with_text(m::paste().to_string())
        .select_all_with_text(m::select_all().to_string())
        .build()?;

    let go_items = build_items(app, GO_ITEMS)?;
    let view_items = build_items(app, VIEW_ITEMS)?;
    let view = SubmenuBuilder::new(app, m::view().to_string())
        .items(&as_refs(&go_items))
        .separator()
        .items(&as_refs(&view_items))
        .separator()
        .fullscreen_with_text(m::fullscreen().to_string())
        .build()?;

    let window = SubmenuBuilder::new(app, m::window().to_string())
        .minimize_with_text(m::minimize().to_string())
        .maximize_with_text(m::zoom().to_string())
        .separator()
        .bring_all_to_front_with_text(m::bring_all_to_front().to_string())
        .build()?;

    let help_items = build_items(app, HELP_ITEMS)?;
    let help = SubmenuBuilder::new(app, m::help().to_string())
        .text(DOCS, m::docs().to_string())
        .text(CLOUDFLARE_DOCS, m::cloudflare_docs().to_string())
        .separator()
        .items(&as_refs(&help_items))
        .separator()
        .text(RELEASES, m::release_notes().to_string())
        .text(ISSUE, m::report_issue().to_string())
        .build()?;

    #[cfg(target_os = "macos")]
    {
        window.set_as_windows_menu_for_nsapp()?;
        help.set_as_help_menu_for_nsapp()?;
    }

    MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &view, &window, &help])
        .build()
}

/// Handles a menu-bar selection.
pub fn on_event<R: Runtime>(app: &AppHandle<R>, event: &MenuEvent) {
    let id = event.id().as_ref();
    let result = match id {
        SETTINGS => windows::open_settings(app),
        DOCS => open_url(app, HelpLink::Docs.url()),
        ISSUE => open_url(app, HelpLink::ReportIssue.url()),
        CLOUDFLARE_DOCS => open_url(app, HelpLink::CloudflareDocs.url()),
        RELEASES => open_url(app, HelpLink::ReleaseNotes.url()),
        CHECK_UPDATES => check_updates(app),
        other if tray::on_event(app, other) => Ok(()),
        other => match command_for(other) {
            Some(command) => {
                windows::focus_main(app);
                MenuAction { command }.emit(app)
            }
            None => Ok(()),
        },
    };
    if let Err(err) = result {
        tracing::warn!(menu = id, error = %err, "menu action failed");
    }
}

/// App menu ▸ Check for Updates…: Settings shows the result.
fn check_updates<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    windows::open_settings(app)?;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(updates) = app.try_state::<crate::shell::updates::Updates>() {
            updates.check(&app, true).await;
        }
    });
    Ok(())
}

fn open_url<R: Runtime>(app: &AppHandle<R>, url: &str) -> tauri::Result<()> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| tauri::Error::Anyhow(err.into()))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn ids_and_accelerators_are_unique() {
        let count = all_items().count();
        let ids: HashSet<_> = all_items().map(|(id, ..)| id).collect();
        let shortcuts: Vec<_> = all_items()
            .map(|(_, _, key, _)| key)
            .filter(|key| !key.is_empty())
            .chain([&"CmdOrCtrl+,"])
            .collect();
        let unique: HashSet<_> = shortcuts.iter().collect();
        assert_eq!(ids.len(), count);
        assert_eq!(unique.len(), shortcuts.len());
    }

    #[test]
    fn maps_ids_to_commands() {
        assert_eq!(command_for("go.doctor"), Some(MenuCommand::GoDoctor));
        assert_eq!(
            command_for("help.diagnostics"),
            Some(MenuCommand::ExportDiagnostics)
        );
        assert_eq!(command_for(SETTINGS), None);
        assert_eq!(command_for("nope"), None);
    }
}
