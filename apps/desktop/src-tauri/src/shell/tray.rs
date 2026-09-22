//! The menu bar extra: a template icon (tinted by macOS for light/dark menu bars) with
//! a native menu. Route and Quick Share status items join it in M1/M3.

use tauri::{
    AppHandle, Runtime,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::TrayIconBuilder,
};

/// Menu id of "Open Teitunnel"; handled by [`crate::shell::menu::on_event`].
pub const OPEN: &str = "tray.open";

/// Adds the menu bar icon.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let menu = MenuBuilder::new(app)
        .item(&MenuItemBuilder::with_id(OPEN, "Open Teitunnel").build(app)?)
        .separator()
        .item(&PredefinedMenuItem::quit(app, Some("Quit Teitunnel"))?)
        .build()?;
    TrayIconBuilder::with_id("main")
        .icon(tauri::include_image!("icons/tray-template.png"))
        .icon_as_template(true)
        .tooltip("Teitunnel")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .build(app)?;
    Ok(())
}
