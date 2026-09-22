pub mod commands;
pub mod error;
pub mod models;
pub mod services;

use services::ProcessManager;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let mut process_manager = ProcessManager::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            process_manager.set_app_handle(app.handle().clone());
            app.manage(process_manager);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Binary
            commands::check_binary_status,
            commands::download_managed_binary,
            // Auth & Account
            commands::verify_and_save_token,
            commands::get_saved_token,
            commands::delete_saved_token,
            commands::list_accounts,
            commands::list_zones,
            commands::check_cert_status,
            commands::start_browser_login,
            commands::cancel_browser_login,
            commands::delete_cert,
            // Tunnels Lifecycle
            commands::list_tunnels,
            commands::create_tunnel,
            commands::start_tunnel,
            commands::stop_tunnel,
            commands::delete_tunnel,
            commands::get_active_processes,
            commands::start_tunnel_by_token,
            commands::start_named_tunnel,
            commands::list_cert_tunnels,
            commands::create_cert_tunnel,
            commands::delete_cert_tunnel,
            // Quick Ephemeral Tunnel
            commands::start_quick_tunnel,
            commands::stop_quick_tunnel,
            commands::get_quick_tunnel_state,
            // Ingress Rules
            commands::get_tunnel_configuration,
            commands::update_tunnel_configuration,
            // DNS & Hygiene
            commands::list_dns_records,
            commands::create_dns_cname,
            commands::delete_dns_record,
            commands::scan_dns_hygiene,
            // Telemetry & Metrics
            commands::get_tunnel_metrics,
            // Embedded Terminal
            commands::run_terminal_command,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Teitunnel desktop application");
}
