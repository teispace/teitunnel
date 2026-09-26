//! A small parser for the Prometheus text format served at `/metrics`, and typed
//! accessors for the metrics Teitunnel charts
//! (<https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/monitor-tunnels/metrics/>).

use serde::Serialize;

/// One sample: `name{labels} value`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Sample {
    /// Metric name.
    pub name: String,
    /// Label pairs in source order.
    pub labels: Vec<(String, String)>,
    /// Sample value.
    pub value: f64,
}

impl Sample {
    /// Value of a label, if present.
    pub fn label(&self, key: &str) -> Option<&str> {
        self.labels
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
}

/// All samples from one scrape.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MetricsSnapshot {
    /// Samples in source order.
    pub samples: Vec<Sample>,
}

impl MetricsSnapshot {
    /// Parses Prometheus text. Malformed lines are skipped, never fatal.
    pub fn parse(text: &str) -> Self {
        let samples = text
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .filter_map(parse_sample)
            .collect();
        Self { samples }
    }

    fn values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Sample> + 'a {
        self.samples
            .iter()
            .filter(move |sample| sample.name == name)
    }

    /// Sum of all samples of `name` (0 when absent).
    pub fn sum(&self, name: &str) -> f64 {
        self.values(name).map(|sample| sample.value).sum()
    }

    /// Requests proxied since the connector started.
    pub fn total_requests(&self) -> u64 {
        to_count(self.sum("cloudflared_tunnel_total_requests"))
    }

    /// Requests that failed since the connector started.
    pub fn request_errors(&self) -> u64 {
        to_count(self.sum("cloudflared_tunnel_request_errors"))
    }

    /// Requests in flight right now.
    pub fn concurrent_requests(&self) -> u64 {
        to_count(self.sum("cloudflared_tunnel_concurrent_requests_per_tunnel"))
    }

    /// Active edge connections.
    pub fn ha_connections(&self) -> u64 {
        to_count(self.sum("cloudflared_tunnel_ha_connections"))
    }

    /// Edge locations currently connected, e.g. `["ams01", "fra08"]`.
    pub fn edge_locations(&self) -> Vec<String> {
        let mut locations: Vec<String> = self
            .values("cloudflared_tunnel_server_locations")
            .filter(|sample| sample.value > 0.0)
            .filter_map(|sample| sample.label("edge_location").map(str::to_owned))
            .collect();
        locations.sort();
        locations.dedup();
        locations
    }

    /// Latest QUIC round-trip time in milliseconds, averaged over connections.
    pub fn rtt_ms(&self) -> Option<f64> {
        self.mean("quic_client_latest_rtt")
    }

    /// Smoothed QUIC round-trip time in milliseconds, averaged over connections; steadier
    /// than [`Self::rtt_ms`], so it's the one charted.
    pub fn smoothed_rtt_ms(&self) -> Option<f64> {
        self.mean("quic_client_smoothed_rtt")
            .or_else(|| self.rtt_ms())
    }

    /// Active proxied TCP and UDP sessions (`cloudflared access`, private networks).
    pub fn active_sessions(&self) -> u64 {
        to_count(
            self.sum("cloudflared_tcp_active_sessions")
                + self.sum("cloudflared_udp_active_sessions"),
        )
    }

    /// Responses so far by status class: `[2xx, 3xx, 4xx, 5xx]` (1xx counts as 2xx).
    pub fn responses_by_class(&self) -> [u64; 4] {
        let mut classes = [0; 4];
        for (code, count) in self.responses_by_code() {
            let index = match code {
                ..300 => 0,
                300..400 => 1,
                400..500 => 2,
                _ => 3,
            };
            classes[index] += count;
        }
        classes
    }

    fn mean(&self, name: &str) -> Option<f64> {
        let values: Vec<f64> = self
            .values(name)
            .map(|s| s.value)
            .filter(|v| v.is_finite())
            .collect();
        (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
    }

    /// Responses by HTTP status code.
    pub fn responses_by_code(&self) -> Vec<(u16, u64)> {
        let mut codes: Vec<(u16, u64)> = self
            .values("cloudflared_tunnel_response_by_code")
            .filter_map(|sample| {
                let code = sample.label("status_code")?.parse().ok()?;
                Some((code, to_count(sample.value)))
            })
            .collect();
        codes.sort_unstable();
        codes
    }

    /// The cloudflared version reported by `build_info`.
    pub fn version(&self) -> Option<&str> {
        self.values("build_info")
            .find_map(|sample| sample.label("version"))
    }
}

/// Counters are floats in the exposition format; clamp to a non-negative integer.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn to_count(value: f64) -> u64 {
    if value.is_finite() && value > 0.0 {
        value.min(u64::MAX as f64).round() as u64
    } else {
        0
    }
}

fn parse_sample(line: &str) -> Option<Sample> {
    let (name, rest) = line.split_at(line.find(['{', ' '])?);
    let (labels, rest) = if let Some(body) = rest.strip_prefix('{') {
        let (labels, after) = parse_labels(body)?;
        (labels, after)
    } else {
        (Vec::new(), rest)
    };
    let value = rest.split_whitespace().next()?;
    let value = match value {
        "+Inf" => f64::INFINITY,
        "-Inf" => f64::NEG_INFINITY,
        "NaN" => f64::NAN,
        other => other.parse().ok()?,
    };
    (!name.is_empty()).then(|| Sample {
        name: name.to_owned(),
        labels,
        value,
    })
}

/// Parses `k="v",k2="v2"}` and returns the labels and the text after `}`.
fn parse_labels(body: &str) -> Option<(Vec<(String, String)>, &str)> {
    let mut labels = Vec::new();
    let mut rest = body;
    loop {
        rest = rest.trim_start_matches([',', ' ']);
        if let Some(after) = rest.strip_prefix('}') {
            return Some((labels, after));
        }
        let eq = rest.find('=')?;
        let key = rest[..eq].trim().to_owned();
        let mut chars = rest[eq + 1..].char_indices();
        if chars.next()?.1 != '"' {
            return None;
        }
        let mut value = String::new();
        let mut escaped = false;
        let mut end = None;
        for (i, c) in chars {
            match (escaped, c) {
                (true, 'n') => {
                    value.push('\n');
                    escaped = false;
                }
                (true, c) => {
                    value.push(c);
                    escaped = false;
                }
                (false, '\\') => escaped = true,
                (false, '"') => {
                    end = Some(i);
                    break;
                }
                (false, c) => value.push(c),
            }
        }
        let end = end?;
        labels.push((key, value));
        rest = &rest[eq + 1 + end + 1..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REAL: &str = include_str!("../fixtures/2026.9.1/metrics.prom");

    #[test]
    fn reads_the_metrics_we_chart_from_a_real_scrape() {
        let snapshot = MetricsSnapshot::parse(REAL);
        assert!(snapshot.samples.len() > 100);
        assert_eq!(snapshot.ha_connections(), 1);
        assert_eq!(snapshot.total_requests(), 0);
        assert_eq!(snapshot.edge_locations(), ["ktm01"]);
        assert_eq!(snapshot.version(), Some("2026.9.1"));
        assert!(snapshot.rtt_ms().is_some());
    }

    #[test]
    fn parses_labels_with_escapes_and_special_values() {
        let text = "# HELP x y\nreqs{code=\"200\",path=\"/a\\\"b\\\\c\"} 12\nup 1 1700000000\nratio NaN\nbig +Inf\n";
        let snapshot = MetricsSnapshot::parse(text);
        assert_eq!(snapshot.samples.len(), 4);
        assert_eq!(snapshot.samples[0].label("path"), Some("/a\"b\\c"));
        assert!((snapshot.sum("up") - 1.0).abs() < f64::EPSILON);
        assert!(snapshot.samples[2].value.is_nan());
    }

    #[test]
    fn response_codes_are_sorted() {
        let text = "cloudflared_tunnel_response_by_code{status_code=\"502\"} 2\ncloudflared_tunnel_response_by_code{status_code=\"200\"} 40\n";
        assert_eq!(
            MetricsSnapshot::parse(text).responses_by_code(),
            [(200, 40), (502, 2)]
        );
    }

    #[test]
    fn derives_classes_sessions_and_smoothed_rtt() {
        let text = "cloudflared_tunnel_response_by_code{status_code=\"101\"} 1\ncloudflared_tunnel_response_by_code{status_code=\"200\"} 40\ncloudflared_tunnel_response_by_code{status_code=\"304\"} 3\ncloudflared_tunnel_response_by_code{status_code=\"404\"} 5\ncloudflared_tunnel_response_by_code{status_code=\"502\"} 2\ncloudflared_tcp_active_sessions 2\ncloudflared_udp_active_sessions 1\nquic_client_smoothed_rtt{conn_index=\"0\"} 20\nquic_client_smoothed_rtt{conn_index=\"1\"} 30\n";
        let snapshot = MetricsSnapshot::parse(text);
        assert_eq!(snapshot.responses_by_class(), [41, 3, 5, 2]);
        assert_eq!(snapshot.active_sessions(), 3);
        assert_eq!(snapshot.smoothed_rtt_ms(), Some(25.0));
        // Falls back to the latest RTT when the smoothed one is missing.
        let latest = MetricsSnapshot::parse("quic_client_latest_rtt 12\n");
        assert_eq!(latest.smoothed_rtt_ms(), Some(12.0));
        assert_eq!(MetricsSnapshot::default().smoothed_rtt_ms(), None);
    }

    #[test]
    fn skips_garbage() {
        let snapshot = MetricsSnapshot::parse("{}\nname{unterminated=\"x 1\nok 2\n=\n");
        assert_eq!(snapshot.samples.len(), 1);
        assert_eq!(snapshot.samples[0].name, "ok");
    }

    proptest::proptest! {
        #[test]
        fn never_panics(text in "(?s).{0,400}") {
            let snapshot = MetricsSnapshot::parse(&text);
            let _ = (snapshot.total_requests(), snapshot.edge_locations(), snapshot.rtt_ms());
        }
    }
}
