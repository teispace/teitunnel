//! `teitunnel top`: a live terminal dashboard of shares, routes, traffic and (when the
//! app inspects a share) requests. With the app running it reads through the control
//! connection and can share and stop; otherwise it reads this machine's records.

mod model;
mod render;

use std::{
    io::{self, Write as _},
    path::{Path, PathBuf},
    process::ExitCode,
    sync::Arc,
    time::{Duration, Instant},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    crossterm::{
        event::{self, Event as TermEvent, KeyCode, KeyEventKind, KeyModifiers},
        execute,
        terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    },
};
use teitunnel_control::{
    ControlClient,
    protocol::{Event, ShareInfo, ShareKind, StartShare},
};
use teitunnel_core::{
    accounts::Accounts,
    engine::Local,
    secrets::MemoryStore,
    store::Store,
    uptime::{self, UptimeStore},
};
use tokio::sync::{broadcast, mpsc};

use self::model::{
    Command, Dashboard, Health, Key, Origin, RequestRow, RouteRow, ShareRow, Snapshot,
};
use crate::app::{self, Where};

/// How often the screen refreshes.
const TICK: Duration = Duration::from_secs(1);
/// How often routes are read again (through the app they come from Cloudflare).
const ROUTES_EVERY: Duration = Duration::from_secs(15);

/// This machine's records, read without the app.
struct Records {
    runs: PathBuf,
    store: Option<Store>,
}

impl Records {
    fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("teitunnel.db");
        Self {
            runs: data_dir.join("run-cli"),
            store: path.exists().then(|| Store::open(&path).ok()).flatten(),
        }
    }

    fn local(&self) -> Option<(Local, Accounts)> {
        let store = self.store.clone()?;
        let accounts = Accounts::new(store.clone(), Arc::new(MemoryStore::default()));
        Some((Local::new(store), accounts))
    }

    /// Terminals' shares and shares on your domains.
    async fn shares(&self) -> Vec<ShareRow> {
        let mut rows: Vec<ShareRow> = teitunnel_core::cli_shares::list(&self.runs)
            .into_iter()
            .map(|s| ShareRow {
                id: s.owner,
                url: Some(s.url),
                origin: s.origin,
                by: "terminal",
                status: "live".into(),
                started_at: s.started_at,
                requests: None,
                rate: None,
            })
            .collect();
        if let Some((local, _)) = self.local() {
            rows.extend(
                local
                    .shares(None)
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|s| ShareRow {
                        url: Some(format!("https://{}", s.hostname)),
                        id: s.hostname,
                        origin: s.origin,
                        by: "domain",
                        status: "live".into(),
                        started_at: s.created_at,
                        requests: None,
                        rate: None,
                    }),
            );
        }
        rows
    }

    /// Routes from the configuration Teitunnel last applied, with their uptime.
    async fn routes(&self, now_ms: u64) -> Vec<RouteRow> {
        let Some((local, accounts)) = self.local() else {
            return Vec::new();
        };
        let now = i64::try_from(now_ms).unwrap_or(i64::MAX);
        let targets = uptime::targets(&accounts, &local).await;
        let summaries = match &self.store {
            Some(store) => UptimeStore::new(store.clone())
                .summaries(&targets, now)
                .await
                .unwrap_or_default(),
            None => Vec::new(),
        };
        let mut rows = Vec::new();
        for account in accounts.list().await.unwrap_or_default() {
            for tunnel in local.tunnels(&account.id).await.unwrap_or_default() {
                let rules = local
                    .applied_ingress(&tunnel.tunnel_id)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                for rule in rules {
                    let Some(hostname) = rule.hostname.clone() else {
                        continue;
                    };
                    let key = match &rule.path {
                        Some(path) => format!("{hostname} {path}"),
                        None => hostname.clone(),
                    };
                    let summary = summaries.iter().find(|s| {
                        s.route.hostname == hostname
                            && (rule.path.is_none() || s.route.path.is_some())
                    });
                    rows.push(RouteRow {
                        hostname: key,
                        origin: rule.service.clone(),
                        health: match summary.and_then(|s| s.up) {
                            Some(true) => Health::Up,
                            Some(false) => Health::Down,
                            None => Health::Unknown,
                        },
                        uptime: summary.and_then(|s| s.uptime_day),
                    });
                }
            }
        }
        rows
    }

    /// Requests per minute over the last hour, from the connectors' records.
    async fn traffic(&self, now_ms: u64) -> Option<Vec<u64>> {
        let (local, accounts) = self.local()?;
        let minute = i64::try_from(now_ms / 60_000).unwrap_or(i64::MAX);
        let since = minute - 59;
        let mut per_minute = vec![0u64; 60];
        for account in accounts.list().await.unwrap_or_default() {
            for tunnel in local.tunnels(&account.id).await.unwrap_or_default() {
                for rollup in local
                    .rollups(&tunnel.tunnel_id, since)
                    .await
                    .unwrap_or_default()
                {
                    if let Some(slot) = usize::try_from(rollup.minute - since)
                        .ok()
                        .and_then(|i| per_minute.get_mut(i))
                    {
                        *slot += rollup.requests;
                    }
                }
            }
        }
        Some(per_minute)
    }

    fn uptime_for(&self) -> Option<UptimeStore> {
        self.store.clone().map(UptimeStore::new)
    }
}

fn share_row(share: ShareInfo) -> ShareRow {
    ShareRow {
        id: share.id,
        url: share.url,
        origin: share.origin,
        by: match share.kind {
            ShareKind::Quick => "app",
            ShareKind::Terminal => "terminal",
            ShareKind::Domain => "domain",
        },
        status: share.status,
        started_at: share.started_at,
        requests: share.requests,
        rate: None,
    }
}

fn health(status: &str) -> Health {
    match status {
        "live" => Health::Up,
        "connecting" | "restarting" => Health::Pending,
        _ => Health::Down,
    }
}

/// Where the data comes from.
enum Source {
    App {
        client: Arc<ControlClient>,
        events: broadcast::Receiver<Event>,
    },
    Local,
}

/// One refresh.
async fn snapshot(
    source: &mut Source,
    records: &Records,
    routes_due: bool,
    now_ms: u64,
) -> Result<Snapshot, String> {
    match source {
        Source::Local => Ok(Snapshot {
            shares: records.shares().await,
            routes: if routes_due {
                Some(records.routes(now_ms).await)
            } else {
                None
            },
            traffic: records.traffic(now_ms).await,
            requests: Vec::new(),
        }),
        Source::App { client, events } => {
            let mut requests = Vec::new();
            while let Ok(event) = events.try_recv() {
                if let Event::RequestArrived {
                    share,
                    method,
                    path,
                    status,
                    duration_ms,
                } = event
                {
                    requests.push(RequestRow {
                        share,
                        method,
                        path,
                        status,
                        duration_ms,
                    });
                }
            }
            let status = client.status().await.map_err(|e| app::describe(&e))?;
            let routes = if routes_due {
                Some(app_routes(client, &status, records, now_ms).await)
            } else {
                None
            };
            Ok(Snapshot {
                shares: status.shares.into_iter().map(share_row).collect(),
                routes,
                traffic: None,
                requests,
            })
        }
    }
}

/// Every account's routes through the app, with uptime from the records.
async fn app_routes(
    client: &ControlClient,
    status: &teitunnel_control::protocol::Status,
    records: &Records,
    now_ms: u64,
) -> Vec<RouteRow> {
    let uptimes = match (records.uptime_for(), records.local()) {
        (Some(store), Some((local, accounts))) => {
            let targets = uptime::targets(&accounts, &local).await;
            store
                .summaries(&targets, i64::try_from(now_ms).unwrap_or(i64::MAX))
                .await
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    let mut rows = Vec::new();
    for account in &status.accounts {
        let Ok(list) = client.routes(Some(&account.id)).await else {
            continue;
        };
        for route in list.routes {
            let uptime = uptimes
                .iter()
                .find(|u| u.route.hostname == route.hostname)
                .and_then(|u| u.uptime_day);
            rows.push(RouteRow {
                hostname: match &route.path {
                    Some(path) => format!("{} {path}", route.hostname),
                    None => route.hostname.clone(),
                },
                origin: route.origin,
                health: health(&route.status),
                uptime,
            });
        }
    }
    rows
}

fn now_ms() -> u64 {
    teitunnel_core::domain_shares::now_ms()
}

/// Puts the terminal back however `top` ends.
struct Screen;

impl Screen {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        if let Err(err) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = terminal::disable_raw_mode();
            return Err(err);
        }
        Ok(Self)
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn key(event: &event::KeyEvent) -> Option<Key> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    Some(match event.code {
        KeyCode::Char('c') if event.modifiers.contains(KeyModifiers::CONTROL) => Key::Interrupt,
        KeyCode::Char(c) => Key::Char(c),
        KeyCode::Enter => Key::Enter,
        KeyCode::Esc => Key::Esc,
        KeyCode::Tab => Key::Tab,
        KeyCode::BackTab => Key::BackTab,
        KeyCode::Up => Key::Up,
        KeyCode::Down => Key::Down,
        KeyCode::Backspace => Key::Backspace,
        _ => return None,
    })
}

/// Copies through the terminal (OSC 52), so it works over SSH too.
fn copy(text: &str) {
    use base64::Engine as _;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let mut stdout = io::stdout().lock();
    let _ = write!(stdout, "\x1b]52;c;{encoded}\x07");
    let _ = stdout.flush();
}

/// What a background action reported.
type Report = Result<String, String>;

fn act(source: &Source, records: &Records, command: Command, done: &mpsc::Sender<Report>) {
    let done = done.clone();
    match (source, command) {
        (Source::App { client, .. }, Command::Share(origin)) => {
            let client = Arc::clone(client);
            tokio::spawn(async move {
                let result = client
                    .start_share(&StartShare {
                        origin,
                        stop_after_seconds: None,
                        host_header: teitunnel_control::protocol::HostHeader::Auto,
                    })
                    .await
                    .map(|s| format!("Shared at {}", s.url.unwrap_or_default()))
                    .map_err(|e| app::describe(&e));
                let _ = done.send(result).await;
            });
        }
        (Source::App { client, .. }, Command::Stop(id)) => {
            let client = Arc::clone(client);
            tokio::spawn(async move {
                let result = client
                    .stop_share(&id)
                    .await
                    .map(|()| "Stopped.".to_owned())
                    .map_err(|e| app::describe(&e));
                let _ = done.send(result).await;
            });
        }
        (Source::Local, Command::Share(_)) => {
            let _ = done.try_send(Err(
                "Sharing from here needs the Teitunnel app. Or run `teitunnel share <port>` in another terminal.".into(),
            ));
        }
        (Source::Local, Command::Stop(id)) => {
            let runs = records.runs.clone();
            tokio::spawn(async move {
                let result = if teitunnel_core::cli_shares::stop(&runs, &id).await {
                    Ok("Stopped.".to_owned())
                } else {
                    Err(format!(
                        "Only terminals' shares can be stopped from here. Use `teitunnel shares --stop {id}`."
                    ))
                };
                let _ = done.send(result).await;
            });
        }
        (_, Command::Copy(text)) => copy(&text),
        (_, Command::Quit) => {}
    }
}

/// Runs the dashboard until `q`.
pub(crate) async fn run(data_dir: &Path, wanted: Where) -> Result<ExitCode, String> {
    if !io::IsTerminal::is_terminal(&io::stdout()) {
        return Err("`teitunnel top` needs a terminal. Try `teitunnel status --json`.".into());
    }
    let records = Records::open(data_dir);
    let mut source = match app::connect(data_dir, wanted).await? {
        Some(client) => {
            let events = client
                .subscribe(None)
                .await
                .map_err(|e| app::describe(&e))?;
            Source::App {
                client: Arc::new(client),
                events,
            }
        }
        None => Source::Local,
    };
    let origin = match &source {
        Source::App { client, .. } => Origin::App {
            version: client.hello().app.version.clone(),
        },
        Source::Local => Origin::Local,
    };
    let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    let mut dashboard = Dashboard::new(origin, color);

    // Keys come from a blocking reader thread.
    let (keys_tx, mut keys) = mpsc::channel::<TermEvent>(64);
    let reader = std::thread::spawn(move || {
        loop {
            match event::poll(Duration::from_millis(200)) {
                Ok(true) => match event::read() {
                    Ok(e) => {
                        if keys_tx.blocking_send(e).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
                Ok(false) => {
                    if keys_tx.is_closed() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let screen = Screen::enter().map_err(|e| e.to_string())?;
    let mut terminal =
        Terminal::new(CrosstermBackend::new(io::stdout())).map_err(|e| e.to_string())?;
    let (done_tx, mut done) = mpsc::channel::<Report>(8);
    let mut tick = tokio::time::interval(TICK);
    let mut last = Instant::now();
    let mut routes_at: Option<Instant> = None;
    let result = loop {
        tokio::select! {
            _ = tick.tick() => {
                let due = routes_at.is_none_or(|at| at.elapsed() >= ROUTES_EVERY);
                match snapshot(&mut source, &records, due, now_ms()).await {
                    Ok(snapshot) => {
                        if due {
                            routes_at = Some(Instant::now());
                        }
                        dashboard.update(snapshot, now_ms(), last.elapsed().as_secs_f64());
                        last = Instant::now();
                    }
                    Err(err) => dashboard.message = Some(err),
                }
            }
            Some(event) = keys.recv() => {
                // Other events (resizing) only need the redraw below.
                if let TermEvent::Key(k) = event
                    && let Some(k) = key(&k)
                {
                    match dashboard.key(k) {
                        Some(Command::Quit) => break Ok(ExitCode::SUCCESS),
                        Some(command) => act(&source, &records, command, &done_tx),
                        None => {}
                    }
                }
            }
            Some(report) = done.recv() => {
                dashboard.message = Some(match report {
                    Ok(message) | Err(message) => message,
                });
                // Show the result at once.
                routes_at = None;
            }
        }
        if let Err(err) = terminal.draw(|frame| render::draw(frame, &dashboard)) {
            break Err(err.to_string());
        }
    };
    drop(terminal);
    drop(screen);
    drop(keys);
    let _ = reader.join();
    result
}
