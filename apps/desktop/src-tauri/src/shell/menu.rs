//! The macOS menu bar (DESIGN §9).
//!
//! Items the shell can handle itself (Settings, help links) are handled here; the rest
//! are forwarded to the webview as a typed [`MenuAction`] event. The Edit menu is not
//! optional: without it, ⌘C/⌘V/⌘A don't work in text fields on macOS.

use tauri::{
    AppHandle, Runtime,
    menu::{
        AboutMetadataBuilder, IsMenuItem, Menu, MenuBuilder, MenuEvent, MenuItem, MenuItemBuilder,
        SubmenuBuilder,
    },
};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;

use crate::{
    ipc::{MenuAction, MenuCommand},
    shell::{tray, windows},
};

const SETTINGS: &str = "settings";
const DOCS: &str = "help.docs";
const ISSUE: &str = "help.issue";
const CLOUDFLARE_DOCS: &str = "help.cloudflare_docs";
const RELEASES: &str = "help.releases";
const DOCS_URL: &str = "https://github.com/teispace/teitunnel#readme";
const ISSUE_URL: &str = "https://github.com/teispace/teitunnel/issues/new/choose";
const CLOUDFLARE_DOCS_URL: &str =
    "https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/";
const RELEASES_URL: &str = "https://github.com/teispace/teitunnel/releases";

/// A menu item the webview handles: (id, label, accelerator (empty: none), command).
type WebviewItem = (&'static str, &'static str, &'static str, MenuCommand);

const FILE_ITEMS: &[WebviewItem] = &[
    (
        "file.new_route",
        "New Route…",
        "CmdOrCtrl+N",
        MenuCommand::NewRoute,
    ),
    (
        "file.new_quick_share",
        "New Quick Share…",
        "CmdOrCtrl+Shift+N",
        MenuCommand::NewQuickShare,
    ),
];

const GO_ITEMS: &[WebviewItem] = &[
    (
        "go.overview",
        "Overview",
        "CmdOrCtrl+1",
        MenuCommand::GoOverview,
    ),
    ("go.routes", "Routes", "CmdOrCtrl+2", MenuCommand::GoRoutes),
    (
        "go.quick_share",
        "Quick Share",
        "CmdOrCtrl+3",
        MenuCommand::GoQuickShare,
    ),
    (
        "go.domains",
        "Domains",
        "CmdOrCtrl+4",
        MenuCommand::GoDomains,
    ),
    (
        "go.tunnels",
        "Tunnels",
        "CmdOrCtrl+5",
        MenuCommand::GoTunnels,
    ),
    (
        "go.activity",
        "Activity",
        "CmdOrCtrl+6",
        MenuCommand::GoActivity,
    ),
    ("go.doctor", "Doctor", "CmdOrCtrl+7", MenuCommand::GoDoctor),
];

const VIEW_ITEMS: &[WebviewItem] = &[
    (
        "view.toggle_sidebar",
        "Toggle Sidebar",
        "CmdOrCtrl+Alt+S",
        MenuCommand::ToggleSidebar,
    ),
    (
        "view.toggle_inspector",
        "Toggle Inspector",
        "CmdOrCtrl+Alt+I",
        MenuCommand::ToggleInspector,
    ),
    (
        "view.palette",
        "Command Palette…",
        "CmdOrCtrl+K",
        MenuCommand::CommandPalette,
    ),
    (
        "view.refresh",
        "Refresh",
        "CmdOrCtrl+R",
        MenuCommand::Refresh,
    ),
];

const HELP_ITEMS: &[WebviewItem] = &[
    (
        "help.doctor",
        "Check for Problems",
        "",
        MenuCommand::GoDoctor,
    ),
    (
        "help.diagnostics",
        "Export Diagnostics…",
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

fn build_items<R: Runtime>(
    app: &AppHandle<R>,
    table: &[WebviewItem],
) -> tauri::Result<Vec<MenuItem<R>>> {
    table
        .iter()
        .map(|(id, label, accelerator, _)| {
            let item = MenuItemBuilder::with_id(*id, *label);
            if accelerator.is_empty() {
                item.build(app)
            } else {
                item.accelerator(*accelerator).build(app)
            }
        })
        .collect()
}

fn as_refs<R: Runtime>(items: &[MenuItem<R>]) -> Vec<&dyn IsMenuItem<R>> {
    items
        .iter()
        .map(|item| item as &dyn IsMenuItem<R>)
        .collect()
}

/// Builds the application menu bar.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let info = app.package_info();
    let about = AboutMetadataBuilder::new()
        .name(Some(info.name.clone()))
        .version(Some(info.version.to_string()))
        .copyright(Some("© 2026 Teispace. MIT License."))
        .website(Some("https://github.com/teispace/teitunnel"))
        .credits(Some(
            "Routes and Quick Shares run on cloudflared by Cloudflare (Apache-2.0), downloaded from its GitHub releases and verified before use.",
        ))
        .build();

    let app_menu = SubmenuBuilder::new(app, &info.name)
        .about(Some(about))
        .separator()
        .item(
            &MenuItemBuilder::with_id(SETTINGS, "Settings…")
                .accelerator("CmdOrCtrl+,")
                .build(app)?,
        )
        .separator()
        .services()
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let file_items = build_items(app, FILE_ITEMS)?;
    let file = SubmenuBuilder::new(app, "File")
        .items(&as_refs(&file_items))
        .separator()
        .close_window()
        .build()?;

    let edit = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let go_items = build_items(app, GO_ITEMS)?;
    let view_items = build_items(app, VIEW_ITEMS)?;
    let view = SubmenuBuilder::new(app, "View")
        .items(&as_refs(&go_items))
        .separator()
        .items(&as_refs(&view_items))
        .separator()
        .fullscreen()
        .build()?;

    let window = SubmenuBuilder::new(app, "Window")
        .minimize()
        .maximize()
        .separator()
        .bring_all_to_front()
        .build()?;

    let help_items = build_items(app, HELP_ITEMS)?;
    let help = SubmenuBuilder::new(app, "Help")
        .text(DOCS, "Teitunnel Documentation")
        .text(CLOUDFLARE_DOCS, "Cloudflare Tunnel Documentation")
        .separator()
        .items(&as_refs(&help_items))
        .separator()
        .text(RELEASES, "Release Notes")
        .text(ISSUE, "Report an Issue…")
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
        DOCS => open_url(app, DOCS_URL),
        ISSUE => open_url(app, ISSUE_URL),
        CLOUDFLARE_DOCS => open_url(app, CLOUDFLARE_DOCS_URL),
        RELEASES => open_url(app, RELEASES_URL),
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
