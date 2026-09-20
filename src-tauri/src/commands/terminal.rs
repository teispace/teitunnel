use std::process::Command;
use crate::error::AppError;
use crate::services::BinaryManager;

#[derive(serde::Serialize, serde::Deserialize)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub success: bool,
}

#[tauri::command]
pub async fn run_terminal_command(
    command: String,
    args: Vec<String>,
) -> Result<CommandResult, AppError> {
    // If running cloudflared command, resolve to the found/managed binary
    let program = if command == "cloudflared" {
        if let Some(path) = BinaryManager::find_binary() {
            path.to_string_lossy().to_string()
        } else {
            command
        }
    } else {
        command
    };

    let output = Command::new(&program)
        .args(&args)
        .output()
        .map_err(|e| AppError::ProcessError(format!("Execution failed: {}", e)))?;

    Ok(CommandResult {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        exit_code: output.status.code(),
        success: output.status.success(),
    })
}
