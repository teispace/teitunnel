//! `teitunnel`: Teitunnel's routes from the terminal. It uses the app's accounts,
//! keychain and database (or, on a server, an API token from the environment), and makes
//! every change through the same plan → apply engine, showing the plan before applying
//! it. Connectors run in the app, as Always-on services, or in `teitunnel up`
//! (servers and containers); `share` runs its own for the command's lifetime.

/// Writes a line to stdout; a write error (e.g. a closed pipe) ends the command.
macro_rules! out {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        writeln!(std::io::stdout().lock(), $($arg)*).map_err(|e| e.to_string())
    }};
}

mod context;
mod doctor;
mod probe;
mod serve;
mod share;
mod snapshot;
mod up;

use std::{
    io::{self, BufRead, IsTerminal, Write},
    process::ExitCode,
    time::Duration,
};

use clap::{Parser, Subcommand, ValueEnum};
use teitunnel_core::{
    domain::{Hostname, OriginOptions},
    engine::{AccessRule, Approval, Change, Outcome, Plan, RouteInput, StepState, Warning},
    export::{ExportFormat, render},
};

use crate::context::App;

/// How long a fresh route may take to start answering.
const VERIFY_PATIENCE: Duration = Duration::from_secs(30);

#[derive(Debug, Parser)]
#[command(
    name = "teitunnel",
    // Help and errors say `teitunnel` whatever the file is called (inside the macOS and
    // Windows packages it's teitunnel-cli, D-091).
    bin_name = "teitunnel",
    version,
    about = "Manage Teitunnel routes from the terminal.",
    long_about = "Manage Teitunnel routes from the terminal. Uses the accounts connected in the Teitunnel app; every change is shown before it's applied."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Connect a Cloudflare account with an API token (stored in the OS keychain).
    ///
    /// On a server or in a container without a keychain, don't store it: set
    /// `CLOUDFLARE_API_TOKEN` (or `CLOUDFLARE_API_TOKEN_FILE`) for each command instead.
    /// The token needs Cloudflare Tunnel, DNS and Zone permissions; create one from the
    /// template at https://dash.cloudflare.com/profile/api-tokens.
    Setup,
    /// Run this machine's tunnels in the foreground until stopped (servers, containers).
    Up,
    /// Run this machine's tunnels plus a web dashboard and JSON API (servers).
    ///
    /// Listens on 127.0.0.1:8765 unless told otherwise. Sign in with the password set by
    /// `--set-password` (or TEITUNNEL_WEB_PASSWORD); automation uses API keys.
    Serve {
        /// Where to listen.
        #[arg(long, default_value = "127.0.0.1:8765")]
        listen: std::net::SocketAddr,
        /// Allow listening on a non-loopback address (put TLS in front of it).
        #[arg(long)]
        allow_remote: bool,
        /// Mark the session cookie Secure (when served over HTTPS).
        #[arg(long)]
        secure_cookies: bool,
        /// Set the dashboard password (read from the terminal) and exit.
        #[arg(long)]
        set_password: bool,
    },
    /// Create, list or revoke API keys for the server API.
    #[command(subcommand)]
    ApiKey(ApiKeyCommand),
    /// Keep this machine's tunnels running as an OS service, even after a restart.
    AlwaysOn {
        /// `on`, `off` or `status`.
        #[arg(value_enum)]
        action: AlwaysOnAction,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Only this tunnel (by name); default: all of the account's.
        #[arg(long)]
        tunnel: Option<String>,
    },
    /// List connected Cloudflare accounts.
    Accounts {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// List this machine's routes and their status.
    Routes {
        /// Account name or id (needed when several are connected).
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
        /// Exit with 1 unless every route is live (a health check for containers and
        /// monitoring). Checks every account when none is named.
        #[arg(long)]
        check: bool,
    },
    /// Add, or remove, a route.
    #[command(subcommand)]
    Route(RouteCommand),
    /// List the private networks this machine shares with WARP clients.
    Networks {
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Share, or stop sharing, a private network with WARP clients.
    #[command(subcommand)]
    Network(NetworkCommand),
    /// List this machine's tunnels (routes go on the default one unless `--tunnel` says).
    Tunnels {
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Create, or delete, one of this machine's tunnels.
    #[command(subcommand)]
    Tunnel(TunnelCommand),
    /// Share a local service at a temporary public URL until you press Ctrl-C.
    Share {
        /// What to share: a port (`3000`), `host:port`, or a URL.
        origin: String,
        /// Stop by itself after this long, e.g. `30m`, `2h`, `90s`.
        #[arg(long = "for", value_name = "DURATION", value_parser = share::parse_duration)]
        stop_after: Option<Duration>,
        /// Don't print a QR code.
        #[arg(long)]
        no_qr: bool,
        /// Share at this hostname on one of your domains instead of a random
        /// trycloudflare.com address (removed again when the command ends).
        #[arg(long, value_name = "HOSTNAME")]
        on: Option<String>,
        /// With --on: the account, when several are connected.
        #[arg(long, short, requires = "on")]
        account: Option<String>,
        /// With --on: require a login (an email address, or `@domain`); repeatable.
        #[arg(long, value_name = "EMAIL|@DOMAIN", requires = "on")]
        allow: Vec<String>,
    },
    /// List shares on your domains (from the app or any terminal), or stop one.
    Shares {
        /// Stop the share at this hostname (its route and DNS record are removed).
        #[arg(long, value_name = "HOSTNAME")]
        stop: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Publish static copies of a site to your Cloudflare account (online while this
    /// computer sleeps), and list, update, roll back or delete them.
    #[command(subcommand)]
    Snapshot(snapshot::SnapshotCommand),
    /// Check for problems, like the app's Doctor. Exits with 1 when there's an error.
    Doctor {
        /// Apply the safe fixes (nothing Teitunnel didn't create is touched).
        #[arg(long)]
        fix: bool,
        /// With --fix, apply without asking.
        #[arg(long, short, requires = "fix")]
        yes: bool,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print a shell completion script, e.g. `teitunnel completions zsh`.
    Completions {
        /// The shell.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Print this machine's tunnel and routes as config.yml, Docker Compose or Terraform.
    Export {
        /// What to export as.
        #[arg(value_enum)]
        format: Format,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// One of this machine's tunnels, by name (default: the default tunnel).
        #[arg(long)]
        tunnel: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum ApiKeyCommand {
    /// Create a key (shown once).
    Create {
        /// What it's for, e.g. `deploy`.
        name: String,
    },
    /// List keys (names only).
    List,
    /// Revoke a key by its id (from `list`).
    Revoke {
        /// The key's id.
        id: i64,
    },
}

#[derive(Debug, Subcommand)]
enum TunnelCommand {
    /// Create another tunnel for this machine, e.g. `staging`.
    Create {
        /// Its name (unique in the account).
        name: String,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// Run an existing tunnel of the account on this machine too (nothing changes in
    /// Cloudflare). If another machine runs it, requests are split between them.
    Adopt {
        /// The tunnel's name or id.
        name: String,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
    },
    /// Delete one of this machine's tunnels, with its routes and the DNS records
    /// Teitunnel created for them.
    Delete {
        /// The tunnel's name.
        name: String,
        #[command(flatten)]
        apply: ApplyArgs,
    },
}

/// Origin settings (`originRequest`) for a route; cloudflared's defaults when left out.
#[derive(Debug, Default, clap::Args)]
#[command(next_help_heading = "Origin settings")]
struct OriginArgs {
    /// Host header sent to the service, e.g. for a dev server that checks it.
    #[arg(long, value_name = "HOST")]
    host_header: Option<String>,
    /// Accept any certificate from an HTTPS service (self-signed ones).
    #[arg(long)]
    no_tls_verify: bool,
    /// Hostname expected on the service's TLS certificate.
    #[arg(long, value_name = "NAME")]
    origin_server_name: Option<String>,
    /// Use the request's hostname as the TLS server name.
    #[arg(long)]
    match_sni_to_host: bool,
    /// Certificate authority file for the service's certificate.
    #[arg(long, value_name = "PATH")]
    ca_pool: Option<String>,
    /// Speak HTTP/2 to an HTTPS service.
    #[arg(long)]
    http2_origin: bool,
    /// Don't use chunked transfer encoding (some WSGI servers need this).
    #[arg(long)]
    disable_chunked_encoding: bool,
    /// Seconds to wait for a connection to the service.
    #[arg(long, value_name = "SECONDS")]
    connect_timeout: Option<u32>,
    /// Seconds to wait for the TLS handshake.
    #[arg(long, value_name = "SECONDS")]
    tls_timeout: Option<u32>,
    /// Seconds between TCP keepalive packets.
    #[arg(long, value_name = "SECONDS")]
    tcp_keep_alive: Option<u32>,
    /// Seconds before an idle keepalive connection closes.
    #[arg(long, value_name = "SECONDS")]
    keep_alive_timeout: Option<u32>,
    /// Idle keepalive connections kept open.
    #[arg(long, value_name = "COUNT")]
    keep_alive_connections: Option<u32>,
    /// Don't fall back between IPv4 and IPv6.
    #[arg(long)]
    no_happy_eyeballs: bool,
    /// `socks` to use the service as a SOCKS5 proxy (TCP routes).
    #[arg(long, value_name = "TYPE")]
    proxy_type: Option<String>,
}

impl OriginArgs {
    fn options(self) -> Option<Box<OriginOptions>> {
        let options = OriginOptions {
            http_host_header: self.host_header,
            origin_server_name: self.origin_server_name,
            match_sni_to_host: self.match_sni_to_host,
            no_tls_verify: self.no_tls_verify,
            ca_pool: self.ca_pool,
            http2_origin: self.http2_origin,
            disable_chunked_encoding: self.disable_chunked_encoding,
            connect_timeout: self.connect_timeout,
            tls_timeout: self.tls_timeout,
            tcp_keep_alive: self.tcp_keep_alive,
            keep_alive_timeout: self.keep_alive_timeout,
            keep_alive_connections: self.keep_alive_connections,
            no_happy_eyeballs: self.no_happy_eyeballs,
            proxy_type: self.proxy_type,
        };
        (!options.is_default()).then(|| Box::new(options))
    }
}

#[derive(Debug, Subcommand)]
enum RouteCommand {
    /// Route a hostname to a service on this machine, e.g. `app.example.com 3000`.
    Add {
        /// Public hostname on one of the account's domains.
        hostname: String,
        /// Where traffic goes: a port (`3000`), `host:port`, or a URL.
        origin: String,
        /// Only requests whose path matches this regex, e.g. `^/api`.
        #[arg(long)]
        path: Option<String>,
        /// Require a login: an email address, or `@domain` for anyone at that domain.
        /// Repeat for more people. Needs Cloudflare Zero Trust (free).
        #[arg(long, value_name = "EMAIL|@DOMAIN")]
        allow: Vec<String>,
        #[command(flatten)]
        origin_options: OriginArgs,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// Load balance a route across every machine that routes its hostname (Cloudflare
    /// Load Balancing, a paid add-on): add the same route on each machine first.
    Balance {
        /// Hostname.
        hostname: String,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// Stop load balancing a route (its DNS record serves it again).
    Unbalance {
        /// Hostname.
        hostname: String,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// Remove a route (and its DNS record, if Teitunnel created it).
    Remove {
        /// Hostname.
        hostname: String,
        /// The route's path rule, if it has one.
        #[arg(long)]
        path: Option<String>,
        #[command(flatten)]
        apply: ApplyArgs,
    },
}

#[derive(Debug, Subcommand)]
enum NetworkCommand {
    /// Let WARP clients reach a range through this machine, e.g. `192.168.1.0/24`.
    Add {
        /// An IP address or CIDR range.
        network: String,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// Stop routing a range through this machine.
    Remove {
        /// The range.
        network: String,
        #[command(flatten)]
        apply: ApplyArgs,
    },
}

#[derive(Debug, clap::Args)]
struct ApplyArgs {
    /// Account name or id.
    #[arg(long, short)]
    account: Option<String>,
    /// Apply without asking.
    #[arg(long, short)]
    yes: bool,
    /// Also allow what needs a confirmation: replacing or deleting DNS records Teitunnel
    /// didn't create, or routing a public range.
    #[arg(long)]
    replace: bool,
    /// One of this machine's tunnels, by name. Default: the tunnel carrying the route,
    /// or the default tunnel for a new one.
    #[arg(long)]
    tunnel: Option<String>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum AlwaysOnAction {
    On,
    Off,
    Status,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    ConfigYaml,
    DockerCompose,
    Terraform,
}

impl From<Format> for ExportFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::ConfigYaml => Self::ConfigYaml,
            Format::DockerCompose => Self::DockerCompose,
            Format::Terraform => Self::Terraform,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command).await {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "teitunnel: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(command: Command) -> Result<ExitCode, String> {
    // These don't need the app to be set up.
    match command {
        Command::Share {
            origin,
            stop_after,
            no_qr,
            on: None,
            ..
        } => return share::run(&origin, stop_after, !no_qr).await,
        Command::Setup => return setup().await,
        Command::Completions { shell } => {
            clap_complete::generate(
                shell,
                &mut <Cli as clap::CommandFactory>::command(),
                "teitunnel",
                &mut io::stdout(),
            );
            return Ok(ExitCode::SUCCESS);
        }
        _ => {}
    }
    let app = App::open().await?;
    match command {
        Command::Share {
            origin,
            stop_after,
            on: Some(hostname),
            account,
            allow,
            ..
        } => {
            share::run_on_domain(
                &app,
                &hostname,
                &origin,
                account.as_deref(),
                access_rule(&allow),
                stop_after,
            )
            .await
        }
        Command::Share { .. } | Command::Completions { .. } | Command::Setup => {
            unreachable!("handled above")
        }
        Command::Up => up::up(&app).await,
        Command::Serve {
            set_password: true, ..
        } => set_web_password(&app).await,
        Command::Serve {
            listen,
            allow_remote,
            secure_cookies,
            ..
        } => {
            serve::run(
                app,
                serve::Options {
                    listen,
                    allow_remote,
                    secure_cookies,
                },
            )
            .await
        }
        Command::ApiKey(command) => api_keys(&app, command).await,
        Command::AlwaysOn {
            action,
            account,
            tunnel,
        } => {
            let action = match action {
                AlwaysOnAction::On => up::AlwaysOn::On,
                AlwaysOnAction::Off => up::AlwaysOn::Off,
                AlwaysOnAction::Status => up::AlwaysOn::Status,
            };
            up::always_on(&app, action, account.as_deref(), tunnel.as_deref()).await
        }
        Command::Doctor { fix, yes, json } => doctor::run(&app, json, fix, yes).await,
        Command::Snapshot(command) => snapshot::run(&app, command).await,
        Command::Accounts { json } => accounts(&app, json).await,
        Command::Routes {
            account,
            json,
            check: true,
        } => check_routes(&app, account.as_deref(), json).await,
        Command::Routes { account, json, .. } => routes(&app, account.as_deref(), json).await,
        Command::Route(RouteCommand::Add {
            hostname,
            origin,
            path,
            allow,
            origin_options,
            apply,
        }) => {
            let change = Change::AddRoute {
                route: RouteInput {
                    hostname,
                    path,
                    origin,
                    access: access_rule(&allow),
                    options: origin_options.options(),
                },
            };
            change_routes(&app, change, &apply).await
        }
        Command::Route(RouteCommand::Balance { hostname, apply }) => {
            change_routes(&app, Change::BalanceRoute { hostname }, &apply).await
        }
        Command::Route(RouteCommand::Unbalance { hostname, apply }) => {
            change_routes(&app, Change::UnbalanceRoute { hostname }, &apply).await
        }
        Command::Route(RouteCommand::Remove {
            hostname,
            path,
            apply,
        }) => change_routes(&app, Change::RemoveRoute { hostname, path }, &apply).await,
        Command::Networks { account, json } => networks(&app, account.as_deref(), json).await,
        Command::Network(NetworkCommand::Add { network, apply }) => {
            change_routes(&app, Change::AddNetwork { network }, &apply).await
        }
        Command::Network(NetworkCommand::Remove { network, apply }) => {
            change_routes(&app, Change::RemoveNetwork { network }, &apply).await
        }
        Command::Tunnels { account, json } => tunnels(&app, account.as_deref(), json).await,
        Command::Shares { stop, json } => shares(&app, stop.as_deref(), json).await,
        Command::Tunnel(TunnelCommand::Create { name, apply }) => {
            change_routes(&app, Change::CreateTunnel { name }, &apply).await
        }
        Command::Tunnel(TunnelCommand::Adopt { name, account }) => {
            adopt(&app, &name, account.as_deref()).await
        }
        Command::Tunnel(TunnelCommand::Delete { name, mut apply }) => {
            apply.tunnel = Some(name);
            change_routes(&app, Change::RemoveTunnel, &apply).await
        }
        Command::Export {
            format,
            account,
            tunnel,
        } => export(&app, format, account.as_deref(), tunnel.as_deref()).await,
    }
}

async fn shares(app: &App, stop: Option<&str>, json: bool) -> Result<ExitCode, String> {
    use teitunnel_core::domain_shares::{self, APP_OWNER};
    let list = app
        .engine
        .local()
        .shares(None)
        .await
        .map_err(|e| e.to_string())?;
    if let Some(hostname) = stop {
        let share = list
            .iter()
            .find(|s| s.hostname.eq_ignore_ascii_case(hostname.trim()))
            .ok_or_else(|| format!("No share at {hostname}. See `teitunnel shares`."))?;
        let account = app.account(Some(&share.account_id)).await?;
        let api = app
            .accounts
            .client(&account.id)
            .await
            .map_err(|e| e.to_string())?;
        let connectors = app.connectors(&account).await;
        domain_shares::stop(
            &app.engine,
            &api,
            &connectors,
            app.context(&account),
            &share.hostname,
        )
        .await
        .map_err(|e| e.english())?;
        out!("Stopped sharing https://{}.", share.hostname)?;
        return Ok(ExitCode::SUCCESS);
    }
    let terminals = teitunnel_core::cli_shares::list(&context::data_dir()?.join("run-cli"));
    if json {
        out!(
            "{}",
            serde_json::json!({ "domains": list, "terminals": terminals })
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if list.is_empty() && terminals.is_empty() {
        out!(
            "No shares running. Start one with `teitunnel share 3000` (add `--on demo.example.com` for your own domain)."
        )?;
    }
    let now = domain_shares::now_ms();
    for share in &list {
        let by = if share.owner == APP_OWNER {
            "the app"
        } else {
            "a terminal"
        };
        let ends = share.expires_at.map_or_else(String::new, |at| {
            format!(", ends in {} min", at.saturating_sub(now) / 60_000)
        });
        out!(
            "https://{}\t{}\tstarted by {by}{ends}",
            share.hostname,
            share.origin
        )?;
    }
    for share in &terminals {
        out!("{}\t{}\tstarted in a terminal", share.url, share.origin)?;
    }
    Ok(ExitCode::SUCCESS)
}

async fn adopt(app: &App, name: &str, account: Option<&str>) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let tunnels = api.tunnels(&account.id).await.map_err(|e| e.to_string())?;
    let tunnel = tunnels
        .iter()
        .find(|t| t.name.eq_ignore_ascii_case(name) || t.id == name)
        .ok_or_else(|| format!("The account has no tunnel named “{name}”."))?;
    if !tunnel.connections.is_empty() {
        let _ = writeln!(
            io::stderr().lock(),
            "Note: another machine runs “{}” now. Cloudflare will split its requests between both machines.",
            tunnel.name
        );
    }
    app.engine
        .adopt(&api, &account.id, &tunnel.id)
        .await
        .map_err(|e| e.to_string())?;
    out!(
        "“{}” is now one of this machine's tunnels. Run it with `teitunnel up` or the app.",
        tunnel.name
    )?;
    Ok(ExitCode::SUCCESS)
}

async fn tunnels(app: &App, account: Option<&str>, json: bool) -> Result<ExitCode, String> {
    use teitunnel_core::engine::Connectors as _;
    let account = app.account(account).await?;
    let connectors = app.connectors(&account).await;
    let tunnels = app
        .engine
        .local()
        .tunnels(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let running = |id: &str| {
        !matches!(
            connectors.state(id),
            None | Some(teitunnel_core::runtime::ConnectorState::Stopped)
        )
    };
    if json {
        let list: Vec<_> = tunnels
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.tunnel_id,
                    "name": t.name,
                    "default": t.is_default,
                    "alwaysOn": t.always_on,
                    "running": running(&t.tunnel_id),
                })
            })
            .collect();
        out!("{}", serde_json::Value::Array(list))?;
    } else if tunnels.is_empty() {
        out!(
            "This machine has no tunnel in {} yet. Adding a route creates one.",
            account.name
        )?;
    } else {
        for t in &tunnels {
            let mut notes = Vec::new();
            if t.is_default {
                notes.push("default");
            }
            notes.push(if running(&t.tunnel_id) {
                "running"
            } else {
                "stopped"
            });
            if t.always_on {
                notes.push("always on");
            }
            out!("{}\t{}\t{}", t.name, t.tunnel_id, notes.join(", "))?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The tunnel a change is on: `--tunnel` (by name or id), else the one carrying the
/// route being changed, else the default one (`None`).
async fn tunnel_for(
    app: &App,
    account: &teitunnel_core::accounts::Account,
    named: Option<&str>,
    change: &Change,
) -> Result<Option<String>, String> {
    let local = app.engine.local();
    if let Some(name) = named {
        let tunnels = local
            .tunnels(&account.id)
            .await
            .map_err(|e| e.to_string())?;
        return tunnels
            .into_iter()
            .find(|t| t.name.eq_ignore_ascii_case(name) || t.tunnel_id == name)
            .map(|t| Some(t.tunnel_id))
            .ok_or_else(|| {
                format!("This machine has no tunnel named “{name}”. See `teitunnel tunnels`.")
            });
    }
    match change {
        Change::RemoveRoute { hostname, .. }
        | Change::UpdateRoute { hostname, .. }
        | Change::BalanceRoute { hostname }
        | Change::UnbalanceRoute { hostname } => local
            .tunnel_routing(&account.id, hostname)
            .await
            .map_err(|e| e.to_string()),
        _ => Ok(None),
    }
}

/// Connects the accounts an API token reaches. With a token in the environment it only
/// checks it (nothing is stored); otherwise it reads one from the terminal and stores it
/// in the OS keychain.
async fn setup() -> Result<ExitCode, String> {
    if std::env::var_os("CLOUDFLARE_API_TOKEN").is_some()
        || std::env::var_os("CLOUDFLARE_API_TOKEN_FILE").is_some()
        || std::env::var_os("TEITUNNEL_API_TOKEN").is_some()
        || std::env::var_os("TEITUNNEL_API_TOKEN_FILE").is_some()
    {
        let app = App::open().await?;
        let accounts = app.accounts.list().await.map_err(|e| e.to_string())?;
        out!("The token from the environment works. It isn't stored; set it for each command.")?;
        for account in &accounts {
            out!("  {}\t{}", account.name, account.id)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    out!(
        "Create a token at https://dash.cloudflare.com/profile/api-tokens with Cloudflare Tunnel · Edit, DNS · Edit and Zone · Read, then paste it here."
    )?;
    write!(io::stdout().lock(), "API token: ").map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut token = String::new();
    io::stdin()
        .lock()
        .read_line(&mut token)
        .map_err(|e| e.to_string())?;
    let token = token.trim().to_owned();
    if token.is_empty() {
        return Err("No token given.".into());
    }
    let added = context::connect(teitunnel_core::Secret::new(token)).await?;
    out!("Connected:")?;
    for account in &added {
        out!("  {}\t{}", account.name, account.id)?;
    }
    Ok(ExitCode::SUCCESS)
}

async fn set_web_password(app: &App) -> Result<ExitCode, String> {
    write!(
        io::stdout().lock(),
        "New dashboard password (12+ characters): "
    )
    .map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut password = String::new();
    io::stdin()
        .lock()
        .read_line(&mut password)
        .map_err(|e| e.to_string())?;
    teitunnel_core::web_auth::set_password(app.store(), password.trim_end_matches(['\r', '\n']))
        .await
        .map_err(|e| e.to_string())?;
    out!("Password set. Start the dashboard with `teitunnel serve`.")?;
    Ok(ExitCode::SUCCESS)
}

async fn api_keys(app: &App, command: ApiKeyCommand) -> Result<ExitCode, String> {
    use teitunnel_core::web_auth;
    match command {
        ApiKeyCommand::Create { name } => {
            let key = web_auth::create_api_key(app.store(), &name)
                .await
                .map_err(|e| e.to_string())?;
            out!("{key}")?;
            let _ = writeln!(
                io::stderr().lock(),
                "Keep it safe: it isn't stored and can't be shown again. Use it as `Authorization: Bearer <key>`."
            );
        }
        ApiKeyCommand::List => {
            for key in web_auth::api_keys(app.store())
                .await
                .map_err(|e| e.to_string())?
            {
                out!("{}\t{}", key.id, key.name)?;
            }
        }
        ApiKeyCommand::Revoke { id } => {
            if !web_auth::revoke_api_key(app.store(), id)
                .await
                .map_err(|e| e.to_string())?
            {
                return Err(format!("No API key with id {id}."));
            }
            out!("Revoked.")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn accounts(app: &App, json: bool) -> Result<ExitCode, String> {
    let accounts = app.accounts.list().await.map_err(|e| e.to_string())?;
    if json {
        let list: Vec<_> = accounts
            .iter()
            .map(|a| serde_json::json!({ "id": a.id, "name": a.name }))
            .collect();
        out!("{}", serde_json::Value::Array(list))?;
    } else if accounts.is_empty() {
        out!("No Cloudflare account is connected. Connect one in Teitunnel.")?;
    } else {
        for account in &accounts {
            out!("{}\t{}", account.name, account.id)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn routes(app: &App, account: Option<&str>, json: bool) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let connectors = app.connectors(&account).await;
    let overview = app
        .engine
        .overview(&api, &connectors, app.context(&account))
        .await
        .map_err(|e| e.to_string())?;
    let statuses = overview.statuses();
    if json {
        let list: Vec<_> = overview
            .routes
            .iter()
            .zip(&statuses)
            .map(|(route, (_, status))| {
                serde_json::json!({
                    "hostname": route.hostname,
                    "path": route.path,
                    "origin": route.origin,
                    "access": route.access,
                    "client": route.client,
                    "tunnelId": route.tunnel_id,
                    "status": status,
                })
            })
            .collect();
        out!("{}", serde_json::Value::Array(list))?;
        return Ok(ExitCode::SUCCESS);
    }
    if overview.routes.is_empty() {
        out!("No routes on this machine in {}.", account.name)?;
    }
    // With several tunnels, say which one carries each route.
    let tunnel_name = |id: Option<&str>| {
        (overview.tunnels.len() > 1)
            .then(|| overview.tunnels.iter().find(|t| Some(t.id.as_str()) == id))
            .flatten()
            .map(|t| format!("\ttunnel: {}", t.name))
            .unwrap_or_default()
    };
    for (route, (_, status)) in overview.routes.iter().zip(&statuses) {
        let path = route
            .path
            .as_deref()
            .map(|p| format!(" {p}"))
            .unwrap_or_default();
        let login = route
            .access
            .as_ref()
            .map(|rule| format!("\tlogin: {}", rule.people()))
            .unwrap_or_default();
        out!(
            "{}{path}\t{}\t{}{login}{}",
            route.hostname,
            route.origin,
            status.text().english(),
            tunnel_name(route.tunnel_id.as_deref())
        )?;
        if let Some(client) = &route.client {
            out!("    connect: {}", client.command)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `routes --check`: 0 when every route of the named account (or of every account) is
/// live, 1 otherwise; prints the ones that aren't.
async fn check_routes(app: &App, account: Option<&str>, json: bool) -> Result<ExitCode, String> {
    let accounts = match account {
        Some(_) => vec![app.account(account).await?],
        None => app.accounts.list().await.map_err(|e| e.to_string())?,
    };
    let mut down = Vec::new();
    for account in &accounts {
        let api = app
            .accounts
            .client(&account.id)
            .await
            .map_err(|e| e.to_string())?;
        let connectors = app.connectors(account).await;
        let overview = app
            .engine
            .overview(&api, &connectors, app.context(account))
            .await
            .map_err(|e| e.to_string())?;
        down.extend(
            overview
                .statuses()
                .into_iter()
                .filter(|(_, health)| !health.is_live()),
        );
    }
    if json {
        let list: Vec<_> = down
            .iter()
            .map(|(hostname, health)| serde_json::json!({ "hostname": hostname, "status": health }))
            .collect();
        out!("{}", serde_json::Value::Array(list))?;
    } else {
        for (hostname, health) in &down {
            out!("{hostname}\t{}", health.text().english())?;
        }
    }
    Ok(if down.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

async fn networks(app: &App, account: Option<&str>, json: bool) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let connectors = app.connectors(&account).await;
    let overview = app
        .engine
        .overview(&api, &connectors, app.context(&account))
        .await
        .map_err(|e| e.to_string())?;
    let networks = overview.networks.ok_or_else(|| {
        "This account's credential can't read private networks. Give the API token the Cloudflare Tunnel permission.".to_owned()
    })?;
    if json {
        out!(
            "{}",
            serde_json::to_string(&networks).map_err(|e| e.to_string())?
        )?;
    } else if networks.is_empty() {
        out!(
            "This machine doesn't share any private network in {}.",
            account.name
        )?;
    } else {
        for network in &networks {
            let note = if network.private {
                ""
            } else {
                "\tpublic range"
            };
            out!("{}{note}", network.network)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `--allow` values as a login rule: `me@xyz.com` is a person, `@xyz.com` (or `xyz.com`)
/// everyone at a domain. The engine validates them.
fn access_rule(allow: &[String]) -> Option<AccessRule> {
    if allow.is_empty() {
        return None;
    }
    let (emails, domains): (Vec<&String>, Vec<&String>) = allow
        .iter()
        .partition(|a| a.trim().find('@').is_some_and(|at| at > 0));
    Some(AccessRule {
        emails: emails.into_iter().cloned().collect(),
        email_domains: domains.into_iter().cloned().collect(),
    })
}

fn warning_text(warning: &Warning) -> String {
    match warning {
        Warning::ReplacesForeignRecord {
            hostname,
            kind,
            content,
        } => format!(
            "{hostname} already has {} {kind} record ({content}) that Teitunnel didn't create. It will be replaced.",
            if kind.starts_with(['A', 'E', 'I', 'O', 'U']) {
                "an"
            } else {
                "a"
            }
        ),
        Warning::DeletesForeignRecord {
            hostname,
            kind,
            content,
        } => format!(
            "The {kind} record for {hostname} ({content}) wasn't created by Teitunnel. It will be deleted."
        ),
        Warning::KeepsForeignRecord { hostname } => format!(
            "The DNS record for {hostname} wasn't created by Teitunnel, so it's left in place."
        ),
        Warning::SingleEndpoint { hostname } => format!(
            "Only this machine serves {hostname} so far: add the same route on another machine for the load balancer to fail over to."
        ),
        Warning::TunnelEmpty => {
            "No routes will be left. The tunnel stays, so adding a route later is quick.".into()
        }
        Warning::RemoteOrigin { origin } => {
            format!("{origin} isn't on this machine. It must be reachable from here.")
        }
        Warning::PublicNetwork { network } => format!(
            "{network} isn't a private range. WARP clients would reach those addresses through this machine instead of the internet."
        ),
        Warning::OverlapsNetwork {
            network,
            other,
            tunnel,
        } => format!(
            "{network} overlaps {other}, which goes through tunnel “{tunnel}”. For addresses in both, the narrower range wins."
        ),
    }
}

fn print_plan(plan: &Plan, account_id: &str) -> Result<(), String> {
    let view = plan.view(account_id);
    for warning in &view.warnings {
        out!("! {}", warning_text(warning))?;
    }
    for (index, step) in view.steps.iter().enumerate() {
        out!("{:>2}. {}", index + 1, step.description)?;
    }
    Ok(())
}

/// Asks `question` (y/N). Without a terminal to ask on, `--yes` is needed.
pub(crate) fn confirm(question: &str) -> Result<bool, String> {
    if !io::stdin().is_terminal() {
        return Err("Not a terminal, so nothing to ask. Pass --yes to apply.".into());
    }
    write!(io::stdout().lock(), "{question} [y/N] ").map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

async fn change_routes(app: &App, change: Change, apply: &ApplyArgs) -> Result<ExitCode, String> {
    let account = app.account(apply.account.as_deref()).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let tunnel = tunnel_for(app, &account, apply.tunnel.as_deref(), &change).await?;
    let ctx = teitunnel_core::engine::Context {
        tunnel: tunnel.as_deref(),
        ..app.context(&account)
    };
    let intent = app
        .engine
        .intent_for(&api, ctx, &change)
        .await
        .map_err(|e| e.to_string())?;
    let plan = app
        .engine
        .preview(&api, ctx, &intent)
        .await
        .map_err(|e| e.to_string())?;
    if plan.is_empty() {
        out!("Nothing to change.")?;
        return Ok(ExitCode::SUCCESS);
    }
    print_plan(&plan, &account.id)?;
    if plan.requires_confirmation && !apply.replace {
        return Err("This needs a confirmation (see above). Pass --replace to allow it.".into());
    }
    if !apply.yes && !confirm("Apply?")? {
        out!("Nothing changed.")?;
        return Ok(ExitCode::SUCCESS);
    }

    let connectors = app.connectors(&account).await;
    let steps = plan.view(&account.id).steps;
    let approval = Approval {
        fingerprint: &plan.fingerprint,
        confirmed: apply.replace,
    };
    let outcome = app
        .engine
        .apply(&api, &connectors, ctx, &intent, approval, |progress| {
            let Some(step) = steps.get(usize::try_from(progress.step).unwrap_or(usize::MAX)) else {
                return;
            };
            let mark = match progress.state {
                StepState::Done => "done",
                StepState::Failed { .. } => "failed",
                StepState::Undone => "undone",
                StepState::UndoFailed { .. } => "couldn't undo",
                _ => return,
            };
            let _ = writeln!(io::stdout().lock(), "    {mark}: {}", step.description);
        })
        .await
        .map_err(|e| e.to_string())?;

    match outcome {
        Outcome::Applied {
            verify,
            connector_error,
            ..
        } => {
            if let Some(error) = connector_error {
                out!("Note: {error}")?;
            }
            let mut ok = true;
            for hostname in verify {
                ok &= check(app, &api, &account, &hostname).await?;
            }
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Outcome::RolledBack { error, .. } => {
            out!("Failed: {error}. Everything was undone.")?;
            Ok(ExitCode::FAILURE)
        }
        Outcome::PartiallyApplied {
            error, leftovers, ..
        } => {
            out!("Failed: {error}. These couldn't be undone:")?;
            for leftover in leftovers {
                out!("  - {leftover}")?;
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Checks a hostname end to end through Cloudflare's edge; `true` if it works.
async fn check(
    app: &App,
    api: &cf_api::Client,
    account: &teitunnel_core::accounts::Account,
    hostname: &str,
) -> Result<bool, String> {
    let host = Hostname::parse(hostname).map_err(|e| e.to_string())?;
    out!("Checking https://{hostname}…")?;
    let result = app
        .engine
        .verify(
            api,
            app.context(account),
            &host,
            context::edge(),
            VERIFY_PATIENCE,
        )
        .await
        .map_err(|e| e.to_string())?;
    match (&result.failure, &result.message) {
        (None, _) => {
            out!("https://{hostname} works.")?;
            Ok(true)
        }
        (Some(_), Some(message)) => {
            out!("https://{hostname} doesn't work yet: {message}")?;
            Ok(false)
        }
        (Some(failure), None) => {
            out!("https://{hostname} doesn't work yet: {failure:?}")?;
            Ok(false)
        }
    }
}

async fn export(
    app: &App,
    format: Format,
    account: Option<&str>,
    tunnel: Option<&str>,
) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let tunnel = tunnel_for(app, &account, tunnel, &Change::RemoveTunnel).await?;
    let ctx = teitunnel_core::engine::Context {
        tunnel: tunnel.as_deref(),
        ..app.context(&account)
    };
    let input = app
        .engine
        .export_input(&api, ctx, None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("This machine has no routes in {} yet.", account.name))?;
    let file = render(&input, format.into());
    write!(io::stdout().lock(), "{}", file.contents).map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_a_share_on_a_domain() {
        let cli = Cli::try_parse_from([
            "teitunnel",
            "share",
            "3000",
            "--on",
            "demo.example.com",
            "--for",
            "2h",
            "--allow",
            "@team.io",
        ])
        .unwrap();
        let Command::Share {
            on,
            stop_after,
            allow,
            ..
        } = cli.command
        else {
            panic!("not a share");
        };
        assert_eq!(on.as_deref(), Some("demo.example.com"));
        assert_eq!(stop_after, Some(Duration::from_secs(7200)));
        assert_eq!(allow, ["@team.io"]);
        // Account and logins only make sense on your own domain.
        assert!(Cli::try_parse_from(["teitunnel", "share", "3000", "--allow", "@x.io"]).is_err());
    }

    #[test]
    fn parses_tunnel_commands() {
        let cli =
            Cli::try_parse_from(["teitunnel", "tunnel", "create", "staging", "--yes"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Tunnel(TunnelCommand::Create { ref name, ref apply }) if name == "staging" && apply.yes
        ));
        let cli = Cli::try_parse_from([
            "teitunnel",
            "route",
            "add",
            "beta.example.com",
            "4000",
            "--tunnel",
            "staging",
        ])
        .unwrap();
        let Command::Route(RouteCommand::Add { apply, .. }) = cli.command else {
            panic!("not a route add");
        };
        assert_eq!(apply.tunnel.as_deref(), Some("staging"));
    }

    #[test]
    fn parses_a_route_add() {
        let cli = Cli::try_parse_from([
            "teitunnel",
            "route",
            "add",
            "app.example.com",
            "3000",
            "--path",
            "^/api",
            "--allow",
            "me@xyz.com",
            "--allow",
            "@team.io",
            "--no-tls-verify",
            "--host-header",
            "app.local",
            "--connect-timeout",
            "15",
            "--yes",
        ])
        .unwrap_or_else(|e| unreachable!("{e}"));
        let Command::Route(RouteCommand::Add {
            hostname,
            origin,
            path,
            allow,
            origin_options,
            apply,
        }) = cli.command
        else {
            unreachable!()
        };
        assert_eq!(
            (hostname.as_str(), origin.as_str(), path.as_deref()),
            ("app.example.com", "3000", Some("^/api"))
        );
        assert!(apply.yes && !apply.replace);
        assert_eq!(
            access_rule(&allow),
            Some(AccessRule {
                emails: vec!["me@xyz.com".into()],
                email_domains: vec!["@team.io".into()],
            })
        );
        assert_eq!(access_rule(&[]), None);
        let options = origin_options.options().unwrap();
        assert!(options.no_tls_verify);
        assert_eq!(options.http_host_header.as_deref(), Some("app.local"));
        assert_eq!(options.connect_timeout, Some(15));
        assert_eq!(
            OriginArgs::default().options(),
            None,
            "defaults send nothing"
        );
    }

    #[test]
    fn parses_a_network_add() {
        let cli =
            Cli::try_parse_from(["teitunnel", "network", "add", "192.168.1.0/24", "--replace"])
                .unwrap_or_else(|e| unreachable!("{e}"));
        let Command::Network(NetworkCommand::Add { network, apply }) = cli.command else {
            unreachable!()
        };
        assert_eq!(network, "192.168.1.0/24");
        assert!(apply.replace && !apply.yes);
    }

    #[test]
    fn describes_every_warning() {
        let text = warning_text(&Warning::ReplacesForeignRecord {
            hostname: "a.xyz.com".into(),
            kind: "A".into(),
            content: "192.0.2.1".into(),
        });
        assert!(text.starts_with("a.xyz.com already has an A record"));
    }
}
