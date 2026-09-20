use tauri::State;
use crate::error::AppError;
use crate::models::QuickTunnelState;
use crate::services::ProcessManager;

#[tauri::command]
pub async fn start_quick_tunnel(
    local_port: u16,
    process_manager: State<'_, ProcessManager>,
) -> Result<QuickTunnelState, AppError> {
    process_manager.start_quick_tunnel(local_port).await
}

#[tauri::command]
pub async fn stop_quick_tunnel(
    process_manager: State<'_, ProcessManager>,
) -> Result<(), AppError> {
    process_manager.stop_quick_tunnel().await
}

#[tauri::command]
pub async fn get_quick_tunnel_state(
    process_manager: State<'_, ProcessManager>,
) -> Result<Option<QuickTunnelState>, AppError> {
    Ok(process_manager.get_quick_tunnel_state().await)
}
