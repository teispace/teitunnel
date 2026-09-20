use crate::error::AppError;
use crate::models::TunnelMetrics;
use crate::services::MetricsCollector;

#[tauri::command]
pub async fn get_tunnel_metrics(
    tunnel_id: String,
    metrics_port: u16,
) -> Result<TunnelMetrics, AppError> {
    MetricsCollector::scrape_metrics(&tunnel_id, metrics_port).await
}
