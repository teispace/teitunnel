use crate::error::AppError;
use crate::models::TunnelMetrics;
use regex::Regex;

pub struct MetricsCollector;

impl MetricsCollector {
    /// Scrapes Prometheus metrics from the local cloudflared metrics endpoint
    pub async fn scrape_metrics(tunnel_id: &str, port: u16) -> Result<TunnelMetrics, AppError> {
        let url = format!("http://127.0.0.1:{}/metrics", port);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()?;

        let res = client.get(&url).send().await?;
        if !res.status().is_success() {
            return Err(AppError::NetworkError(format!(
                "Metrics endpoint returned HTTP {}",
                res.status()
            )));
        }

        let text = res.text().await?;
        let metrics = Self::parse_prometheus_text(tunnel_id, &text);
        Ok(metrics)
    }

    /// Parses Prometheus raw metrics text format
    fn parse_prometheus_text(tunnel_id: &str, raw: &str) -> TunnelMetrics {
        let mut active_connections = 0;
        let mut colos = Vec::new();
        let mut avg_rtt_ms = 0.0;
        let mut total_requests = 0;
        let mut response_2xx = 0;
        let mut response_4xx = 0;
        let mut response_5xx = 0;
        let bytes_in = 0;
        let bytes_out = 0;

        let colo_regex = Regex::new(r#"location="([a-zA-Z0-9]+)""#).unwrap();
        let status_code_regex = Regex::new(r#"status_code="(\d+)""#).unwrap();

        for line in raw.lines() {
            if line.starts_with('#') || line.trim().is_empty() {
                continue;
            }

            if line.starts_with("cloudflared_tunnel_active_connections") {
                if let Some(val) = line.split_whitespace().last().and_then(|v| v.parse::<usize>().ok()) {
                    active_connections = val;
                }
            } else if line.starts_with("cloudflared_tunnel_server_locations") {
                if let Some(mat) = colo_regex.captures(line) {
                    if let Some(colo) = mat.get(1) {
                        let colo_str = colo.as_str().to_string();
                        if !colos.contains(&colo_str) {
                            colos.push(colo_str);
                        }
                    }
                }
            } else if line.starts_with("cloudflared_tunnel_round_trip_time_seconds") {
                if let Some(val) = line.split_whitespace().last().and_then(|v| v.parse::<f64>().ok()) {
                    avg_rtt_ms = val * 1000.0; // convert to ms
                }
            } else if line.starts_with("cloudflared_tunnel_total_requests") {
                if let Some(val) = line.split_whitespace().last().and_then(|v| v.parse::<u64>().ok()) {
                    total_requests = val;
                }
            } else if line.starts_with("cloudflared_tunnel_response_by_code") {
                if let Some(mat) = status_code_regex.captures(line) {
                    if let Some(code_match) = mat.get(1) {
                        if let Ok(code) = code_match.as_str().parse::<u16>() {
                            if let Some(count) = line.split_whitespace().last().and_then(|v| v.parse::<u64>().ok()) {
                                match code {
                                    200..=299 => response_2xx += count,
                                    400..=499 => response_4xx += count,
                                    500..=599 => response_5xx += count,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        TunnelMetrics {
            tunnel_id: tunnel_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            active_connections,
            colos,
            avg_rtt_ms,
            total_requests,
            response_2xx,
            response_4xx,
            response_5xx,
            bytes_in,
            bytes_out,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prometheus_metrics() {
        let sample_raw = r#"
# HELP cloudflared_tunnel_active_connections Number of active connections to the edge
# TYPE cloudflared_tunnel_active_connections gauge
cloudflared_tunnel_active_connections 4
# HELP cloudflared_tunnel_round_trip_time_seconds Round trip time
# TYPE cloudflared_tunnel_round_trip_time_seconds gauge
cloudflared_tunnel_round_trip_time_seconds 0.0245
# HELP cloudflared_tunnel_server_locations Edge location
cloudflared_tunnel_server_locations{location="SJC"} 1
cloudflared_tunnel_server_locations{location="LAX"} 1
# HELP cloudflared_tunnel_total_requests Total requests
cloudflared_tunnel_total_requests 1420
cloudflared_tunnel_response_by_code{status_code="200"} 1300
cloudflared_tunnel_response_by_code{status_code="404"} 110
cloudflared_tunnel_response_by_code{status_code="502"} 10
"#;

        let parsed = MetricsCollector::parse_prometheus_text("test-uuid", sample_raw);
        assert_eq!(parsed.tunnel_id, "test-uuid");
        assert_eq!(parsed.active_connections, 4);
        assert_eq!(parsed.colos, vec!["SJC", "LAX"]);
        assert!((parsed.avg_rtt_ms - 24.5).abs() < 0.001);
        assert_eq!(parsed.total_requests, 1420);
        assert_eq!(parsed.response_2xx, 1300);
        assert_eq!(parsed.response_4xx, 110);
        assert_eq!(parsed.response_5xx, 10);
    }
}

