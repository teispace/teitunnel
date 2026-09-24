//! `teitunnel traffic …`: requests captured by the inspector (LocalCan parity): list,
//! show, follow, replay, clear and export.
//!
//! Commands read through the [`Traffic`] trait. Today's implementation is the history
//! the inspector keeps in the database ([`History`]): every process with Lens (the app,
//! `teitunnel share`/`inspect`/`serve`/`mcp`) writes its finished captures there, masked,
//! for a day, so this works across processes. When the local control connection to the
//! app lands, a second implementation can read the app's Lens directly (live, unmasked on
//! request, and replaying through its taps).

use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    process::ExitCode,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use clap::{Args, Subcommand, ValueEnum};
use teitunnel_core::{
    inspect::{
        HistoryQuery, StoredTap, history, history_after, history_clear, history_get, history_taps,
        is_masked,
        lens::{
            self, CaptureStore, Exchange, ExchangeId, ExchangeKind, Filter, Lens, LensOptions,
            MemoryStore, Redaction, ReplayOptions, RequestEdits, TapConfig, TapId, Upstream,
            export::ExportFormat,
        },
    },
    store::Store,
};

use crate::share::status;

/// How often `watch` looks for new requests.
const FOLLOW_EVERY: Duration = Duration::from_millis(400);

/// `teitunnel traffic …`
#[derive(Debug, Subcommand)]
pub(crate) enum TrafficCommand {
    /// List captured requests, newest first.
    Ls {
        #[command(flatten)]
        filter: FilterArgs,
        /// How many.
        #[arg(long, default_value_t = 20)]
        last: usize,
        /// Print JSON (one object per request, newest first).
        #[arg(long)]
        json: bool,
    },
    /// Show one request and its response.
    Get {
        /// The request's id (or the end of it, as `ls` shows).
        id: String,
        /// markdown, curl, httpie, fetch, http, har or json.
        #[arg(long, value_enum, default_value = "markdown")]
        format: Format,
        /// Show credentials (only possible in the process that captured the request;
        /// the history keeps them masked).
        #[arg(long)]
        reveal: bool,
    },
    /// Print requests as they arrive, until Ctrl-C.
    Watch {
        #[command(flatten)]
        filter: FilterArgs,
        /// Print JSON lines.
        #[arg(long)]
        json: bool,
    },
    /// Send a captured request to its service again.
    Replay {
        /// The request's id (or the end of it).
        id: String,
        /// Set a header, `Name: value` (repeatable).
        #[arg(long = "set-header", value_name = "NAME:VALUE")]
        set_header: Vec<String>,
        /// Another method.
        #[arg(long)]
        method: Option<String>,
        /// Another path (with query string).
        #[arg(long)]
        path: Option<String>,
        /// Another body: text, or `@file` to read it from a file.
        #[arg(long)]
        body: Option<String>,
        /// How many times, one after another (1–100).
        #[arg(long, default_value_t = 1)]
        times: u32,
    },
    /// Forget captured requests (all, or one share's or route's).
    Clear {
        /// Only this tap (as `ls --json` shows it).
        #[arg(long)]
        tap: Option<String>,
    },
    /// Export captured requests to a file (HAR by default).
    Export {
        /// Write a HAR file here.
        #[arg(long, value_name = "FILE")]
        har: Option<PathBuf>,
        /// Or write another format (markdown, curl, httpie, fetch, http, json) to stdout.
        #[arg(long, value_enum, conflicts_with = "har")]
        format: Option<Format>,
        #[command(flatten)]
        filter: FilterArgs,
        /// How many (newest first).
        #[arg(long, default_value_t = 100)]
        last: usize,
    },
}

/// Which requests.
#[derive(Debug, Clone, Default, Args)]
pub(crate) struct FilterArgs {
    /// Only this host (text in it).
    #[arg(long)]
    host: Option<String>,
    /// Only this method.
    #[arg(long)]
    method: Option<String>,
    /// A status (`404`) or class (`5xx`).
    #[arg(long)]
    status: Option<String>,
    /// Text in the path.
    #[arg(long)]
    path: Option<String>,
    /// http, ws or sse.
    #[arg(long, value_enum)]
    kind: Option<Kind>,
    /// Text anywhere in the request or response (secrets can't be searched).
    #[arg(long)]
    text: Option<String>,
}

/// Exchange kinds for `--kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Kind {
    /// Plain HTTP.
    Http,
    /// WebSocket.
    Ws,
    /// Server-sent events.
    Sse,
}

/// Output formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Format {
    /// Markdown (issues, chats, agents).
    Markdown,
    /// A curl command.
    Curl,
    /// An HTTPie command.
    Httpie,
    /// A JavaScript fetch call.
    Fetch,
    /// Raw HTTP.
    Http,
    /// HAR 1.2.
    Har,
    /// JSON.
    Json,
}

impl From<Format> for ExportFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::Markdown => Self::Markdown,
            Format::Curl => Self::Curl,
            Format::Httpie => Self::Httpie,
            Format::Fetch => Self::Fetch,
            Format::Http => Self::Raw,
            Format::Har => Self::Har,
            Format::Json => Self::Json,
        }
    }
}

impl FilterArgs {
    /// As Lens's filter.
    pub(crate) fn filter(&self) -> Result<Filter, String> {
        let mut filter = Filter {
            methods: self.method.iter().map(|m| m.to_ascii_uppercase()).collect(),
            host: self.host.clone(),
            path: self.path.clone(),
            text: self.text.clone(),
            kinds: self
                .kind
                .map(|k| match k {
                    Kind::Http => ExchangeKind::Http,
                    Kind::Ws => ExchangeKind::WebSocket,
                    Kind::Sse => ExchangeKind::Sse,
                })
                .into_iter()
                .collect(),
            ..Filter::default()
        };
        if let Some(status) = &self.status {
            let lower = status.trim().to_ascii_lowercase();
            match lower.strip_suffix("xx") {
                Some(class) => filter.status_classes.push(
                    class
                        .parse()
                        .map_err(|_| format!("\"{status}\" isn't a status. Try 404 or 5xx."))?,
                ),
                None => filter.statuses.push(
                    lower
                        .parse()
                        .map_err(|_| format!("\"{status}\" isn't a status. Try 404 or 5xx."))?,
                ),
            }
        }
        Ok(filter)
    }
}

/// Changes to a request before it's sent again.
#[derive(Debug, Clone, Default)]
pub(crate) struct Replay {
    pub(crate) edits: RequestEdits,
    pub(crate) times: u32,
}

/// Where captured traffic is read from.
pub(crate) trait Traffic {
    /// The newest requests matching `filter`.
    async fn list(&self, filter: Filter, limit: usize) -> Result<Vec<Exchange>, String>;
    /// One request by id or the end of its id.
    async fn get(&self, id: &str) -> Result<Exchange, String>;
    /// Requests captured after `cursor` (`None`: from now), and the next cursor.
    async fn follow(&self, cursor: Option<i64>) -> Result<(Vec<Exchange>, i64), String>;
    /// Sends a request again; returns the replays (captured).
    async fn replay(&self, id: &str, replay: Replay) -> Result<Vec<Exchange>, String>;
    /// Forgets requests of one tap, or all.
    async fn clear(&self, tap: Option<TapId>) -> Result<(), String>;
    /// Whether it can show credentials.
    fn can_reveal(&self) -> bool;
}

/// The inspector's history in the database, written by every process with Lens.
#[derive(Debug, Clone)]
pub(crate) struct History {
    store: Store,
}

impl History {
    pub(crate) fn new(store: Store) -> Self {
        Self { store }
    }

    async fn tap(&self, tap: &TapId) -> Result<Option<StoredTap>, String> {
        Ok(history_taps(&self.store)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|t| t.id == *tap))
    }
}

impl Traffic for History {
    async fn list(&self, filter: Filter, limit: usize) -> Result<Vec<Exchange>, String> {
        history(&self.store, HistoryQuery { filter, limit })
            .await
            .map_err(|e| e.to_string())
    }

    async fn get(&self, id: &str) -> Result<Exchange, String> {
        let id = id.trim();
        if let Ok(exact) = ExchangeId::from_str(id) {
            return history_get(&self.store, exact)
                .await
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("No captured request {id}."));
        }
        let recent = self.list(Filter::default(), lens::MAX_PAGE).await?;
        let mut found: Vec<Exchange> = recent
            .into_iter()
            .filter(|e| e.id.to_string().ends_with(&id.to_ascii_lowercase()))
            .collect();
        match found.len() {
            0 => Err(format!(
                "No captured request ends with {id}. `teitunnel traffic ls` lists them."
            )),
            1 => Ok(found.remove(0)),
            _ => Err(format!(
                "Several requests end with {id}; give more of the id."
            )),
        }
    }

    async fn follow(&self, cursor: Option<i64>) -> Result<(Vec<Exchange>, i64), String> {
        history_after(&self.store, cursor)
            .await
            .map_err(|e| e.to_string())
    }

    async fn replay(&self, id: &str, replay: Replay) -> Result<Vec<Exchange>, String> {
        let exchange = self.get(id).await?;
        let tap = self.tap(&exchange.tap).await?.ok_or_else(|| {
            "The share or route that captured this request isn't known any more.".to_owned()
        })?;
        // Credentials were masked when the request was stored: leave them out rather than
        // send the mask (unless the edits set them).
        let mut edits = replay.edits;
        for (name, value) in &exchange.request.headers {
            let set = edits
                .set_headers
                .iter()
                .any(|(n, _)| n.eq_ignore_ascii_case(name.as_str()));
            if !set && value.to_str().is_ok_and(is_masked) {
                status(&format!(
                    "Leaving out {name}: credentials aren't kept in the history (set it with --set-header)."
                ));
                edits.remove_headers.push(name.as_str().to_owned());
            }
        }
        let store = Arc::new(MemoryStore::default());
        let captures: Arc<dyn CaptureStore> = store.clone();
        let lens = Lens::new(LensOptions {
            store: Some(captures),
            ..LensOptions::default()
        })
        .map_err(|e| e.to_string())?;
        let mut config = TapConfig::new(
            Upstream::origin(tap.origin.trim_end_matches('/')).map_err(|e| e.to_string())?,
        );
        config.id = Some(exchange.tap.clone());
        config.name = tap.name;
        lens.add_tap(config).map_err(|e| e.to_string())?;
        let id = exchange.id;
        store.put(Arc::new(exchange));
        let replays = lens
            .replay(
                id,
                &ReplayOptions {
                    edits,
                    times: replay.times.clamp(1, lens::MAX_REPLAYS),
                    ..ReplayOptions::default()
                },
            )
            .await
            .map_err(|e| e.to_string())?;
        lens.shutdown().await;
        Ok(replays.iter().map(|e| (**e).clone()).collect())
    }

    async fn clear(&self, tap: Option<TapId>) -> Result<(), String> {
        history_clear(&self.store, tap.as_ref())
            .await
            .map_err(|e| e.to_string())
    }

    fn can_reveal(&self) -> bool {
        false
    }
}

/// A byte count in words: `512 B`, `1.2 KB`, `3.4 MB`.
fn size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64;
    if bytes < 1_024 {
        format!("{bytes} B")
    } else if bytes < 1_024 * 1_024 {
        format!("{:.1} KB", value / 1_024.0)
    } else {
        format!("{:.1} MB", value / (1_024.0 * 1_024.0))
    }
}

/// `HH:MM:SS` (UTC) of a time in milliseconds.
fn clock(ms: u64) -> String {
    let secs = ms / 1_000 % 86_400;
    format!("{:02}:{:02}:{:02}", secs / 3_600, secs / 60 % 60, secs % 60)
}

/// One line per request: time, method, status, duration, size, host and path, id.
pub(crate) fn line(exchange: &Exchange) -> String {
    let redaction = Redaction::masked();
    let request = &exchange.request;
    let path = match request.query() {
        Some(query) => format!(
            "{}?{}",
            lens::mask_text(request.path(), &redaction),
            lens::mask_query(query, &redaction)
        ),
        None => lens::mask_text(request.path(), &redaction).into_owned(),
    };
    let status = match (exchange.status(), &exchange.error) {
        (Some(status), _) => status.as_u16().to_string(),
        (None, Some(_)) => "ERR".to_owned(),
        (None, None) => "…".to_owned(),
    };
    let duration = exchange
        .duration()
        .map_or_else(|| "-".to_owned(), |d| format!("{} ms", d.as_millis()));
    let bytes = exchange.response.as_ref().map_or(0, |r| r.body.size);
    let id = exchange.id.to_string();
    let short = &id[id.len().saturating_sub(8)..];
    let kind = match exchange.kind {
        ExchangeKind::WebSocket => " [ws]",
        ExchangeKind::Sse => " [sse]",
        ExchangeKind::Upgrade => " [upgrade]",
        ExchangeKind::Http => "",
    };
    let replay = if exchange.replay_of.is_some() {
        " (replay)"
    } else {
        ""
    };
    format!(
        "{}  {:<7} {:<4} {:>8} {:>9}  {}{path}{kind}{replay}  {short}",
        clock(exchange.started_at_ms),
        request.method,
        status,
        duration,
        size(bytes),
        request.host,
    )
}

fn json(exchange: &Exchange) -> Result<String, String> {
    serde_json::to_string(&exchange.view(&Redaction::masked())).map_err(|e| e.to_string())
}

fn read_body(body: &str) -> Result<bytes::Bytes, String> {
    match body.strip_prefix('@') {
        Some(path) => std::fs::read(path)
            .map(bytes::Bytes::from)
            .map_err(|e| format!("Couldn't read {path}: {e}")),
        None => Ok(bytes::Bytes::from(body.to_owned())),
    }
}

fn header(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once(':')
        .ok_or_else(|| format!("\"{value}\" isn't a header. Write it as Name: value."))?;
    Ok((name.trim().to_owned(), value.trim().to_owned()))
}

/// Runs a traffic command against `traffic`.
pub(crate) async fn run(
    traffic: &impl Traffic,
    command: TrafficCommand,
) -> Result<ExitCode, String> {
    match command {
        TrafficCommand::Ls { filter, last, json } => {
            let exchanges = traffic.list(filter.filter()?, last.max(1)).await?;
            if exchanges.is_empty() && !json {
                status(
                    "No captured requests. Shares are inspected by default; routes with `teitunnel inspect <hostname>`.",
                );
            }
            for exchange in &exchanges {
                if json {
                    out!("{}", self::json(exchange)?)?;
                } else {
                    out!("{}", line(exchange))?;
                }
            }
        }
        TrafficCommand::Get { id, format, reveal } => {
            let exchange = traffic.get(&id).await?;
            let redaction = if reveal && traffic.can_reveal() {
                Redaction::revealed()
            } else {
                if reveal {
                    status(
                        "Credentials are masked in the history kept on disk; only the process that captured the request (the app's inspector) can reveal them.",
                    );
                }
                Redaction::masked()
            };
            let text = if format == Format::Har {
                lens::export::har_string(&[&exchange], &redaction)
            } else {
                lens::export::export(format.into(), &exchange, &redaction)
            };
            out!("{text}")?;
        }
        TrafficCommand::Watch { filter, json } => {
            let filter = filter.filter()?;
            let (_, mut cursor) = traffic.follow(None).await?;
            status("Watching for requests. Press Ctrl-C to stop.");
            let stop = crate::share::interrupted();
            tokio::pin!(stop);
            loop {
                tokio::select! {
                    () = &mut stop => break,
                    () = tokio::time::sleep(FOLLOW_EVERY) => {}
                }
                let (exchanges, next) = traffic.follow(Some(cursor)).await?;
                cursor = next;
                for exchange in exchanges
                    .iter()
                    .filter(|e| filter.matches(e, &Redaction::masked()))
                {
                    if json {
                        out!("{}", self::json(exchange)?)?;
                    } else {
                        out!("{}", line(exchange))?;
                    }
                }
            }
        }
        TrafficCommand::Replay {
            id,
            set_header,
            method,
            path,
            body,
            times,
        } => {
            let edits = RequestEdits {
                method: method.map(|m| m.to_ascii_uppercase()),
                path_and_query: path,
                set_headers: set_header
                    .iter()
                    .map(|h| header(h))
                    .collect::<Result<_, _>>()?,
                remove_headers: Vec::new(),
                body: body.as_deref().map(read_body).transpose()?,
            };
            let replays = traffic.replay(&id, Replay { edits, times }).await?;
            for replay in &replays {
                out!("{}", line(replay))?;
            }
        }
        TrafficCommand::Clear { tap } => {
            let tap = tap
                .map(|t| TapId::new(&t).map_err(|e| e.to_string()))
                .transpose()?;
            traffic.clear(tap).await?;
            status("Captured requests cleared.");
        }
        TrafficCommand::Export {
            har,
            format,
            filter,
            last,
        } => {
            let exchanges = traffic.list(filter.filter()?, last.max(1)).await?;
            let redaction = Redaction::masked();
            let refs: Vec<&Exchange> = exchanges.iter().collect();
            match (har, format) {
                (Some(path), _) => {
                    std::fs::write(&path, lens::export::har_string(&refs, &redaction))
                        .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
                    status(&format!(
                        "Wrote {} request(s) to {} (credentials masked).",
                        refs.len(),
                        path.display()
                    ));
                }
                (None, Some(Format::Har) | None) => {
                    out!("{}", lens::export::har_string(&refs, &redaction))?;
                }
                (None, Some(format)) => {
                    for exchange in &refs {
                        out!(
                            "{}\n",
                            lens::export::export(format.into(), exchange, &redaction)
                        )?;
                    }
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Prints a line for each finished request `inspector` captures, to stderr (stdout stays
/// for the URL), until the returned task is dropped or aborted.
pub(crate) fn print_requests(
    inspector: &teitunnel_core::inspect::Inspector,
) -> Option<tokio::task::JoinHandle<()>> {
    use tokio::sync::broadcast::error::RecvError;
    let mut events = inspector.live().ok()?;
    let tty = io::stderr().is_terminal();
    Some(tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(lens::LensEvent::Exchange {
                    change: lens::Change::Completed,
                    exchange,
                }) => {
                    let text = line(&exchange);
                    status(&if tty { format!("  {text}") } else { text });
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_filters() {
        let filter = FilterArgs {
            method: Some("post".into()),
            status: Some("5xx".into()),
            kind: Some(Kind::Ws),
            ..FilterArgs::default()
        }
        .filter()
        .unwrap();
        assert_eq!(filter.methods, ["POST"]);
        assert_eq!(filter.status_classes, [5]);
        assert_eq!(filter.kinds, [ExchangeKind::WebSocket]);
        let exact = FilterArgs {
            status: Some("404".into()),
            ..FilterArgs::default()
        }
        .filter()
        .unwrap();
        assert_eq!(exact.statuses, [404]);
        assert!(
            FilterArgs {
                status: Some("teapot".into()),
                ..FilterArgs::default()
            }
            .filter()
            .is_err()
        );
    }

    #[test]
    fn formats_sizes_times_and_headers() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(1_536), "1.5 KB");
        assert_eq!(size(3 * 1_024 * 1_024), "3.0 MB");
        assert_eq!(clock(3_723_000), "01:02:03");
        assert_eq!(
            header("X-Debug: 1").unwrap(),
            ("X-Debug".to_owned(), "1".to_owned())
        );
        assert!(header("nope").is_err());
    }
}
