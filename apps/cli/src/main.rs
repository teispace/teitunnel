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

mod analytics;
mod app;
mod backup;
mod browser;
mod comments;
mod complete;
mod context;
#[cfg(test)]
mod docs;
mod doctor;
mod expose;
mod exposure;
mod fronts;
mod inspect;
mod local;
mod mcp;
mod probe;
mod project;
mod protect;
mod serve;
mod share;
mod sharing;
mod snapshot;
mod top;
mod traffic;
mod up;

use std::{
    io::{self, BufRead, IsTerminal, Write},
    process::ExitCode,
    time::Duration,
};

use clap::{Parser, Subcommand, ValueEnum};
use teitunnel_core::{
    domain::{Hostname, OriginOptions},
    engine::{AccessRule, Approval, Change, Outcome, Plan, RouteInput, SignIn, StepState, Warning},
    export::{ExportFormat, render},
};

use crate::context::App;

#[derive(Debug, Parser)]
#[command(
    name = "teitunnel",
    // Help and errors say `teitunnel` whatever the file is called (inside the macOS and
    // Windows packages it's teitunnel-cli).
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
    ///
    /// In a folder with a teitunnel.yml, its plan is shown and applied first, and its
    /// shares run as long as this does.
    Up(up::UpArgs),
    /// Work with the project file (teitunnel.yml): check, diff, apply, init, down.
    #[command(subcommand)]
    Project(project::ProjectCommand),
    /// Local HTTPS domains on this computer: https://shop.test with a trusted certificate.
    #[command(subcommand, name = "local-domain", visible_alias = "local")]
    LocalDomain(local::LocalCommand),
    /// The Teitunnel browser extension: let it talk to the app from your browsers.
    #[command(subcommand)]
    Browser(browser::BrowserCommand),
    /// Move to another computer: an encrypted backup of Teitunnel's setup (never a
    /// token or password), and restoring it.
    #[command(subcommand)]
    Backup(backup::BackupCommand),
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
        /// Don't serve the MCP endpoint (`/mcp`, for AI agents with an API key).
        #[arg(long)]
        no_mcp: bool,
        /// The MCP endpoint's mode: read-only, ask (default) or full.
        #[arg(long, value_parser = mcp::parse_mode)]
        mcp_mode: Option<teitunnel_mcp::Mode>,
        /// A browser origin allowed to call `/mcp` (repeatable). Agents send none.
        #[arg(long, value_name = "ORIGIN")]
        mcp_allow_origin: Vec<String>,
    },
    /// Run Teitunnel's MCP server for AI agents (Claude Code, Cursor, VS Code, Codex…)
    /// over stdio, or connect a client to it (`teitunnel mcp install cursor`).
    ///
    /// Agents share local services, manage routes through reviewed plans, diagnose
    /// problems and inspect traffic. Modes: `read-only`, `ask` (default: every change
    /// needs your approval) and `full`. Secrets never reach the agent.
    Mcp {
        #[command(subcommand)]
        command: Option<mcp::McpCommand>,
        /// read-only, ask (default) or full.
        #[arg(long, value_parser = mcp::parse_mode)]
        mode: Option<teitunnel_mcp::Mode>,
        /// Show credentials in captured traffic and logs to the agent (off by default).
        #[arg(long)]
        allow_secrets: bool,
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
        /// Ask the running app (its connectors' state is the live one); fails if it
        /// isn't running. By default the app is used when it runs.
        #[arg(long, conflicts_with = "check")]
        app: bool,
    },
    /// Whether the Teitunnel app is running, and what it serves.
    Status {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// A live dashboard of shares, routes and traffic (q quits, ? for keys).
    Top {
        /// Only with the running app.
        #[arg(long, conflicts_with = "here")]
        app: bool,
        /// Without the app, from this machine's records.
        #[arg(long)]
        here: bool,
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
    /// Share a local service, or a folder of files, at a temporary public URL until you
    /// press Ctrl-C.
    Share {
        /// What to share: a port (`3000`), `host:port`, a URL, or a folder (`./dist`).
        origin: String,
        /// Stop by itself after this long, e.g. `30m`, `2h`, `90s`.
        #[arg(long = "for", value_name = "DURATION", value_parser = share::parse_duration)]
        stop_after: Option<Duration>,
        /// Don't print a QR code.
        #[arg(long)]
        no_qr: bool,
        /// Share at this hostname on one of your domains instead of a random
        /// trycloudflare.com address (removed again when the command ends). `{project}`,
        /// `{branch}` and `{user}` are filled in from this folder, e.g.
        /// `--on {branch}.dev.teispace.com`; `--on` alone uses the name last used here.
        #[arg(long, value_name = "HOSTNAME", num_args = 0..=1, default_missing_value = "")]
        on: Option<String>,
        /// With a folder: list the files of folders that have no index.html (the
        /// default when the folder itself has none).
        #[arg(long, conflicts_with = "no_listing")]
        listing: bool,
        /// With a folder: never list files, even when it has no index.html.
        #[arg(long)]
        no_listing: bool,
        /// With a folder: a single-page app (unknown paths get /index.html).
        #[arg(long)]
        spa: bool,
        /// With --on: on only during these hours, paused otherwise, e.g.
        /// `"mon-fri 09:00-18:00"`.
        #[arg(long, value_name = "DAYS HH:MM-HH:MM", requires = "on")]
        schedule: Option<String>,
        /// With --schedule: its time zone, e.g. `Europe/Berlin` (default: this computer's).
        #[arg(long, value_name = "ZONE", requires = "schedule")]
        tz: Option<String>,
        /// With --on: the account, when several are connected.
        #[arg(long, short, requires = "on")]
        account: Option<String>,
        /// With --on: require a login (an email address, `@domain`, or `github:ORG[/TEAM]`
        /// for the members of a GitHub organization or team); repeatable.
        #[arg(long, value_name = "EMAIL|@DOMAIN|github:ORG[/TEAM]", requires = "on")]
        allow: Vec<String>,
        /// With --allow: how people log in, `github` or `google` (the account's login method
        /// of that kind, set up in Cloudflare Zero Trust), or `any` (the default).
        #[arg(long, value_name = "METHOD", value_parser = parse_sign_in, requires = "allow")]
        sign_in: Option<SignIn>,
        /// Send this Host header to the service, e.g. `localhost:5173` for a dev server
        /// that only answers its own address. By default Vite, webpack and Angular dev
        /// servers get their own address.
        #[arg(long, value_name = "HOST", conflicts_with = "no_host_header")]
        host_header: Option<String>,
        /// Pass the visitor's Host header through unchanged, even to a dev server.
        #[arg(long)]
        no_host_header: bool,
        /// Share through the running app (it keeps the share after this command ends);
        /// fails if the app isn't running. This is the default when the app runs.
        #[arg(long, conflicts_with_all = ["here", "on"])]
        app: bool,
        /// Share from this terminal, for as long as the command runs, even when the app
        /// is running.
        #[arg(long)]
        here: bool,
        /// Print `{"url": …, "hostname": …}` on stdout once it's live (for scripts and CI).
        #[arg(long)]
        json: bool,
        /// Don't share when the exposure check finds a leak (a .env file, the git
        /// folder, debug pages…); by default it only warns.
        #[arg(long)]
        strict: bool,
        /// Don't send requests through Teitunnel's inspector (it records them for
        /// `teitunnel traffic`, masking credentials).
        #[arg(long)]
        no_inspect: bool,
        /// Don't print a line for each request.
        #[arg(long, short)]
        quiet: bool,
        /// Stop after this long without a request, e.g. `30m`.
        #[arg(long, value_name = "DURATION", value_parser = share::parse_duration, conflicts_with = "no_inspect")]
        idle: Option<Duration>,
        /// Say so when a request hits this path, e.g. `/webhooks/*` (repeatable).
        #[arg(long, value_name = "PATH", conflicts_with = "no_inspect")]
        watch: Vec<String>,
        /// Let visitors pin comments on its pages (read and answer them with
        /// `teitunnel comments` or in the app).
        #[arg(long, conflicts_with = "no_inspect")]
        comments: bool,
        /// Share a local MCP server for remote AI clients: checks it answers MCP, keeps
        /// streams alive and requires a bearer token (needs --on: Quick Tunnels don't
        /// carry event streams). Prints configurations for Claude Code, Cursor and VS
        /// Code.
        #[arg(long, requires = "on", conflicts_with_all = ["no_inspect", "ai"])]
        mcp: bool,
        /// With --mcp: the server's endpoint path (default: /mcp, then /sse and /).
        #[arg(long, value_name = "PATH", requires = "mcp")]
        mcp_path: Option<String>,
        /// Share a local AI server (Ollama, LM Studio, vLLM) behind a bearer token, for
        /// OpenAI-compatible clients.
        #[arg(long, conflicts_with = "no_inspect")]
        ai: bool,
        /// With --mcp or --ai on your domain: make a new token instead of the saved one.
        #[arg(long)]
        new_token: bool,
    },
    /// Inspect one of this machine's routes while this command runs: its requests go
    /// through Teitunnel's inspector (shown here and in `teitunnel traffic`), and it's
    /// pointed back at its own service when the command ends. The change is shown first.
    Inspect {
        /// The route's hostname.
        hostname: String,
        /// The route's path rule, if it has one.
        #[arg(long)]
        path: Option<String>,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Point a route left inspected back at its own service.
        #[arg(long)]
        off: bool,
        /// Don't ask before changing the route.
        #[arg(long, short)]
        yes: bool,
    },
    /// Requests captured by the inspector: list, show, follow, replay, clear, export.
    #[command(subcommand)]
    Traffic(traffic::TrafficCommand),
    /// Show the bearer token a shared service expects (`share --mcp` or `--ai` on your
    /// domain), or make a new one.
    Token {
        /// The hostname.
        hostname: String,
        /// Make a new token (clients with the old one stop working).
        #[arg(long)]
        new: bool,
    },
    /// Reserve a hostname so teammates sharing the account see it's taken (a placeholder
    /// DNS record with your name, until a date or until released). Reserving it again
    /// changes the end date; a route you add there keeps the reservation.
    Reserve {
        /// The hostname, e.g. `review.dev.teispace.com`.
        hostname: String,
        /// When it ends: `2026-12-31` (end of that day, UTC) or `2026-12-31T18:00Z`.
        #[arg(long, value_name = "DATE")]
        until: Option<String>,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// List the account's reserved hostnames and who holds them (`reservations ls`).
    Reservations {
        /// `ls` (the default).
        #[arg(value_enum, default_value = "ls")]
        action: ReservationsAction,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Give up a hostname's reservation (a route there stays).
    Release {
        /// The hostname.
        hostname: String,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// List shares (the app's, terminals' and on your domains), or stop, pause or resume
    /// one.
    Shares {
        /// Stop a share: its URL, its hostname on your domain, or its id.
        #[arg(long, value_name = "URL|HOSTNAME", conflicts_with_all = ["pause", "resume"])]
        stop: Option<String>,
        /// Pause a share (on your domain, a route, or one of the app's Quick Shares by its
        /// URL or id): the address stays, and visitors see a paused page until it's resumed.
        #[arg(long, value_name = "URL|HOSTNAME", conflicts_with = "resume")]
        pause: Option<String>,
        /// Serve a paused share or route again, at the same address.
        #[arg(long, value_name = "URL|HOSTNAME")]
        resume: Option<String>,
        /// With --pause or --resume on a route: the account, when several are connected.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
        /// Ask the running app; fails if it isn't running. By default the app is used
        /// when it runs.
        #[arg(long)]
        app: bool,
    },
    /// Run a share on your domain (or a route) only during set hours: visitors see a
    /// paused page the rest of the time. Without hours, shows its schedule.
    Schedule {
        /// The hostname.
        hostname: String,
        /// Days and hours, e.g. `mon-fri 09:00-18:00`, `weekends 10:00-16:00` or
        /// `daily 22:00-02:00` (past midnight).
        #[arg(value_name = "DAYS HH:MM-HH:MM", num_args = 0..)]
        spec: Vec<String>,
        /// The time zone, e.g. `Europe/Berlin` (default: this computer's).
        #[arg(long, value_name = "ZONE")]
        tz: Option<String>,
        /// Remove the schedule (the share stays as it is now).
        #[arg(long, conflicts_with = "spec")]
        off: bool,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
    },
    /// List schedules of shares and routes.
    Schedules {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Traffic from Cloudflare's edge: one route in detail, or every route of this
    /// machine side by side. Needs the token's Zone ▸ Analytics ▸ Read permission.
    Analytics {
        /// A route's hostname (default: every route of this machine).
        hostname: Option<String>,
        /// With a hostname: only requests under this path, e.g. `/api`.
        #[arg(long)]
        path: Option<String>,
        /// `hour`, `day`, `week` or `month`.
        #[arg(long, default_value = "day", value_parser = parse_range)]
        range: teitunnel_core::analytics::AnalyticsRange,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Uptime of this machine's routes (checked every minute through Cloudflare while the
    /// app, `up` or `serve` runs).
    Uptime {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Publish static copies of a site to your Cloudflare account (online while this
    /// computer sleeps), and list, update, roll back or delete them.
    #[command(subcommand)]
    Snapshot(snapshot::SnapshotCommand),
    /// Protect a hostname at Cloudflare's edge: challenge or block bots and AI
    /// crawlers, rate limit visitors, set or remove headers. Without options, shows what
    /// it has now.
    Protect(protect::ProtectArgs),
    /// Service tokens for machines (CI, scripts, servers) to pass a hostname's login.
    #[command(subcommand)]
    ServiceToken(protect::ServiceTokenCommand),
    /// Comments reviewers pinned to your shares and Snapshots: list, reply, resolve.
    #[command(subcommand)]
    Comments(comments::CommentsCommand),
    /// Show your own page instead of Cloudflare's error 1033 while this computer is off
    /// (a Worker on your account; without options, shows what the route has).
    Offline(fronts::OfflineArgs),
    /// Keep webhooks while this computer is off and deliver them in order when it's back.
    #[command(subcommand)]
    Inbox(fronts::InboxCommand),
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
    /// The cloudflared Teitunnel uses: `status`, or `install` (the latest release from
    /// Cloudflare, verified, into Teitunnel's data folder; for servers and CI).
    Cloudflared {
        /// `status` or `install`.
        #[arg(value_enum, default_value = "status")]
        action: CloudflaredAction,
    },
    /// Print a shell completion script, e.g. `teitunnel completions zsh`. It completes
    /// commands and flags, and your hostnames, tunnels, domains, shares and accounts from
    /// this machine's records (no network).
    Completions {
        /// The shell.
        #[arg(value_enum)]
        shell: complete::Shell,
        /// Commands and flags only, in a script that never runs `teitunnel`.
        #[arg(long = "static")]
        static_script: bool,
    },
    /// Candidates for the completion scripts (`completions`); not for people.
    #[command(name = "__complete", hide = true)]
    Complete {
        /// The shell asking.
        shell: String,
        /// Which word is being completed (0 is `teitunnel`).
        index: usize,
        /// The command line's words.
        #[arg(raw = true)]
        words: Vec<String>,
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
    /// Route a hostname to a service on this machine, e.g. `app.teispace.com 3000`.
    Add {
        /// Public hostname on one of the account's domains.
        hostname: String,
        /// Where traffic goes: a port (`3000`), `host:port`, or a URL.
        origin: String,
        /// Only requests whose path matches this regex, e.g. `^/api`.
        #[arg(long)]
        path: Option<String>,
        /// Require a login: an email address, `@domain` for anyone at that domain, or
        /// `github:ORG[/TEAM]` for the members of a GitHub organization or team. Repeat for
        /// more people. Needs Cloudflare Zero Trust (free).
        #[arg(long, value_name = "EMAIL|@DOMAIN|github:ORG[/TEAM]")]
        allow: Vec<String>,
        /// With --allow: how people log in, `github` or `google` (the account's login method
        /// of that kind, set up in Cloudflare Zero Trust), or `any` (the default).
        #[arg(long, value_name = "METHOD", value_parser = parse_sign_in, requires = "allow")]
        sign_in: Option<SignIn>,
        /// With --allow: a path that skips the login, e.g. `/webhooks`, so webhook
        /// senders get through; repeatable.
        #[arg(long, value_name = "PATH", requires = "allow")]
        skip_login: Vec<String>,
        #[command(flatten)]
        origin_options: Box<OriginArgs>,
        /// Don't add the route when the exposure check finds a leak in the service;
        /// by default it only warns.
        #[arg(long)]
        strict: bool,
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
    /// Take a hostname someone else holds (their reservation, or another machine's
    /// route).
    #[arg(long)]
    take_over: bool,
    /// One of this machine's tunnels, by name. Default: the tunnel carrying the route,
    /// or the default tunnel for a new one.
    #[arg(long)]
    tunnel: Option<String>,
}

fn parse_range(value: &str) -> Result<teitunnel_core::analytics::AnalyticsRange, String> {
    teitunnel_core::analytics::AnalyticsRange::parse(value)
        .ok_or_else(|| format!("`{value}` isn't a range; use hour, day, week or month."))
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CloudflaredAction {
    Status,
    Install,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ReservationsAction {
    Ls,
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
    // A browser starting this command for the Teitunnel extension (native messaging):
    // no arguments to parse, and standard output carries only its messages.
    let args: Vec<String> = std::env::args().collect();
    if teitunnel_core::browser_host::started_by_browser(&args) {
        return browser::host().await;
    }
    let cli = Cli::parse();
    // On the heap: the future for every command together is large.
    match Box::pin(run(cli.command)).await {
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
            on,
            account,
            ai: true,
            new_token,
            no_inspect,
            quiet,
            idle,
            watch,
            ..
        } => {
            let options = share::ShareOptions {
                inspect: !no_inspect,
                idle,
                watch,
                log: !quiet,
                bearer: None,
                oauth: None,
                comments: false,
            };
            return expose::ai(
                &origin,
                on.as_deref(),
                account.as_deref(),
                new_token,
                stop_after,
                !no_qr,
                options,
            )
            .await;
        }
        Command::Share {
            origin,
            stop_after,
            no_qr,
            on: None,
            host_header,
            no_host_header,
            app,
            here,
            json,
            strict,
            no_inspect,
            quiet,
            idle,
            watch,
            listing,
            no_listing,
            spa,
            comments,
            ..
        } => {
            let host_header = share::host_header_choice(host_header, no_host_header);
            let folder = folder_arg(&origin, listing_choice(listing, no_listing), spa)?;
            // The app inspects by its own settings; options about the inspector keep
            // the share in this terminal.
            let local_only = no_inspect || idle.is_some() || !watch.is_empty() || comments;
            let wanted = app::Where::from_flags(app, here || local_only);
            let dir = context::data_dir()?;
            if let Some(client) = app::connect(&dir, wanted).await? {
                if folder.is_none() {
                    exposure::check(&origin, share::store(&dir).ok().as_ref(), strict).await?;
                }
                let request = app::ShareRequest {
                    origin: &origin,
                    folder: folder.as_ref(),
                    stop_after,
                    host_header: &host_header,
                };
                return app::share(&client, &request, !no_qr, json).await;
            }
            let options = share::ShareOptions {
                inspect: !no_inspect,
                idle,
                watch,
                log: !quiet,
                bearer: None,
                oauth: None,
                comments,
            };
            return share::run(
                &origin,
                folder,
                stop_after,
                !no_qr,
                json,
                &host_header,
                &options,
                strict,
            )
            .await;
        }
        Command::Shares {
            stop,
            pause,
            resume,
            account,
            json,
            app,
        } => {
            let wanted = app::Where::from_flags(app, false);
            let pausing = pause.clone().map(|id| (id, true));
            let pausing = pausing.or_else(|| resume.clone().map(|id| (id, false)));
            if let Some(client) = app::connect(&context::data_dir()?, wanted).await? {
                if let Some((id, paused)) = pausing {
                    return app::pause(&client, &id, account.as_deref(), paused).await;
                }
                return app::shares(&client, stop.as_deref(), json).await;
            }
            let app = App::open().await?;
            if let Some((id, paused)) = pausing {
                return sharing::pause_here(&app, &id, account.as_deref(), paused).await;
            }
            return shares(&app, stop.as_deref(), json).await;
        }
        Command::Routes {
            account,
            json,
            check: false,
            app,
        } => {
            let wanted = app::Where::from_flags(app, false);
            if let Some(client) = app::connect(&context::data_dir()?, wanted).await? {
                let list = client
                    .routes(account.as_deref())
                    .await
                    .map_err(|e| app::describe(&e))?;
                return app::print_routes(&list, json);
            }
            let app = App::open().await?;
            return routes(&app, account.as_deref(), json).await;
        }
        Command::Status { json } => return app::status(&context::data_dir()?, json).await,
        Command::Top { app, here } => {
            return top::run(&context::data_dir()?, app::Where::from_flags(app, here)).await;
        }
        Command::Complete {
            index, mut words, ..
        } => {
            // Some shells drop the empty word being completed.
            if words.len() <= index {
                words.resize(index + 1, String::new());
            }
            let names = context::data_dir()
                .map(|dir| teitunnel_core::completion::candidates(&dir))
                .unwrap_or_default();
            let command = <Cli as clap::CommandFactory>::command();
            for candidate in complete::complete(&command, &words, index, &names) {
                out!("{}", candidate.line())?;
            }
            return Ok(ExitCode::SUCCESS);
        }
        Command::Traffic(command) => {
            let dir = context::data_dir()?;
            let path = dir.join("teitunnel.db");
            if !path.exists() {
                return Err("No captured requests: Teitunnel keeps them in the app's database, which doesn't exist on this machine yet.".into());
            }
            let store = teitunnel_core::store::Store::open(&path).map_err(|e| e.to_string())?;
            return traffic::run(&traffic::History::new(store), command).await;
        }
        Command::Setup => return setup().await,
        Command::Cloudflared { action } => return cloudflared_command(action).await,
        Command::Project(command) => return project::run(command).await,
        Command::LocalDomain(command) => return local::run(command).await,
        Command::Browser(command) => return browser::run(command).await,
        Command::Mcp {
            command: Some(command),
            ..
        } => return mcp::setup(command),
        Command::Mcp {
            command: None,
            mode,
            allow_secrets,
        } => return mcp::serve(mode, allow_secrets).await,
        Command::Completions {
            shell,
            static_script,
        } => {
            let script = complete::script(
                shell,
                static_script,
                &mut <Cli as clap::CommandFactory>::command(),
            );
            write!(io::stdout().lock(), "{script}").map_err(|e| e.to_string())?;
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
            mcp: true,
            mcp_path,
            new_token,
            quiet,
            idle,
            watch,
            ..
        } => {
            let (hostname, _) = on_hostname(&app, &hostname).await?;
            let options = share::ShareOptions {
                inspect: true,
                idle,
                watch,
                log: !quiet,
                bearer: None,
                oauth: None,
                comments: false,
            };
            expose::mcp(
                &app,
                &origin,
                mcp_path.as_deref(),
                &hostname,
                account.as_deref(),
                new_token,
                stop_after,
                options,
            )
            .await
        }
        Command::Share {
            origin,
            stop_after,
            on: Some(hostname),
            account,
            allow,
            sign_in,
            host_header,
            no_host_header,
            json,
            strict,
            no_inspect,
            quiet,
            idle,
            watch,
            listing,
            no_listing,
            spa,
            schedule,
            tz,
            comments,
            ..
        } => {
            let folder = folder_arg(&origin, listing_choice(listing, no_listing), spa)?;
            if folder.is_some() && no_inspect {
                return Err(
                    "A folder is served by Teitunnel's inspector; leave out --no-inspect.".into(),
                );
            }
            let schedule = schedule
                .map(|spec| teitunnel_core::schedule::Schedule::parse(&spec, tz.as_deref()))
                .transpose()
                .map_err(|e| e.to_string())?;
            let (hostname, remember) = on_hostname(&app, &hostname).await?;
            let options = share::ShareOptions {
                inspect: !no_inspect,
                idle,
                watch,
                log: !quiet,
                bearer: None,
                oauth: None,
                comments,
            };
            share::run_on_domain(
                &app,
                &hostname,
                &origin,
                share::DomainShareOptions {
                    account,
                    allow: access_rule(&allow, &[], sign_in),
                    stop_after,
                    json,
                    strict,
                    folder,
                    schedule,
                    remember: Some(remember),
                },
                &share::host_header_choice(host_header, no_host_header),
                &options,
                |_| Ok(()),
            )
            .await
        }
        Command::Inspect {
            hostname,
            path,
            account,
            off,
            yes,
        } => {
            inspect::run(
                &app,
                &hostname,
                path.as_deref(),
                account.as_deref(),
                off,
                yes,
            )
            .await
        }
        Command::Token { hostname, new } => expose::show_token(&app, &hostname, new).await,
        Command::Reserve {
            hostname,
            until,
            apply,
        } => change_routes(&app, Change::ReserveHostname { hostname, until }, &apply).await,
        Command::Release { hostname, apply } => {
            change_routes(&app, Change::ReleaseHostname { hostname }, &apply).await
        }
        Command::Reservations {
            action: ReservationsAction::Ls,
            account,
            json,
        } => reservations(&app, account.as_deref(), json).await,
        Command::Share { .. }
        | Command::Traffic(_)
        | Command::Cloudflared { .. }
        | Command::Completions { .. }
        | Command::Complete { .. }
        | Command::Status { .. }
        | Command::Top { .. }
        | Command::Shares { .. }
        | Command::Routes { check: false, .. }
        | Command::Setup
        | Command::Project(_)
        | Command::LocalDomain(_)
        | Command::Browser(_)
        | Command::Mcp { .. } => {
            unreachable!("handled above")
        }
        Command::Up(args) => up::up(&app, &args).await,
        Command::Schedule {
            hostname,
            spec,
            tz,
            off,
            account,
        } => {
            sharing::set_schedule(
                &app,
                &hostname,
                &spec,
                tz.as_deref(),
                off,
                account.as_deref(),
            )
            .await
        }
        Command::Schedules { json } => sharing::list_schedules(&app, json).await,
        Command::Backup(command) => backup::run(&app, command).await,
        Command::Serve {
            set_password: true, ..
        } => set_web_password(&app).await,
        Command::Serve {
            listen,
            allow_remote,
            secure_cookies,
            no_mcp,
            mcp_mode,
            mcp_allow_origin,
            ..
        } => {
            serve::run(
                app,
                serve::Options {
                    listen,
                    allow_remote,
                    secure_cookies,
                    mcp: (!no_mcp).then_some(serve::McpOptions {
                        mode: mcp_mode,
                        allowed_origins: mcp_allow_origin,
                    }),
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
        Command::Analytics {
            hostname,
            path,
            range,
            account,
            json,
        } => {
            analytics::analytics(
                &app,
                hostname.as_deref(),
                path.as_deref(),
                range,
                account.as_deref(),
                json,
            )
            .await
        }
        Command::Uptime { json } => analytics::uptime(&app, json).await,
        Command::Snapshot(command) => snapshot::run(&app, command).await,
        Command::Protect(args) => protect::protect(&app, args).await,
        Command::ServiceToken(command) => protect::service_token(&app, command).await,
        Command::Comments(command) => comments::run(&app, command).await,
        Command::Offline(args) => fronts::offline(&app, args).await,
        Command::Inbox(command) => fronts::inbox(&app, command).await,
        Command::Accounts { json } => accounts(&app, json).await,
        Command::Routes {
            account,
            json,
            check: true,
            ..
        } => check_routes(&app, account.as_deref(), json).await,
        Command::Route(RouteCommand::Add {
            hostname,
            origin,
            path,
            allow,
            sign_in,
            skip_login,
            origin_options,
            strict,
            apply,
        }) => {
            exposure::check(&origin, Some(app.store()), strict).await?;
            let change = Change::AddRoute {
                route: RouteInput {
                    hostname,
                    path,
                    origin,
                    access: access_rule(&allow, &skip_login, sign_in),
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

/// `--listing` / `--no-listing`; neither: lists only a folder without an index.html.
fn listing_choice(listing: bool, no_listing: bool) -> Option<bool> {
    (listing || no_listing).then_some(listing)
}

/// A folder to share, when `origin` names one (`./dist`, `/srv/site`).
fn folder_arg(
    origin: &str,
    listing: Option<bool>,
    spa: bool,
) -> Result<Option<teitunnel_core::folder_share::FolderShare>, String> {
    use teitunnel_core::folder_share::{FolderShare, looks_like_folder};
    if !looks_like_folder(origin) {
        if listing.is_some() || spa {
            return Err(format!(
                "--listing, --no-listing and --spa are for folders, and {origin} isn't one."
            ));
        }
        return Ok(None);
    }
    FolderShare::resolve(origin, listing, spa)
        .map(Some)
        .map_err(|e| e.to_string())
}

/// The hostname of `share --on`: `{project}`, `{branch}` and `{user}` filled in from the
/// current folder, or the name last used here when none is given. Also returns what to
/// remember for the folder.
async fn on_hostname(
    app: &App,
    typed: &str,
) -> Result<(String, (std::path::PathBuf, String)), String> {
    use teitunnel_core::share_names;
    let dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let template = if typed.trim().is_empty() {
        share_names::remembered(app.store(), &dir)
            .await
            .map_err(|e| e.to_string())?
            .ok_or_else(|| {
                "No name was used for a share from this folder yet; give one: --on demo.teispace.com (or --on {branch}.dev.teispace.com).".to_owned()
            })?
    } else {
        typed.trim().to_owned()
    };
    let hostname = share_names::expand(&template, &dir).map_err(|e| e.to_string())?;
    if hostname != template.to_ascii_lowercase() {
        share::status(&format!("{template} is {hostname} here."));
    }
    Ok((hostname, (dir, template)))
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
            "No shares running. Start one with `teitunnel share 3000` (add `--on demo.teispace.com` for your own domain)."
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
        let paused = if share.paused { ", paused" } else { "" };
        out!(
            "https://{}\t{}\tstarted by {by}{paused}{ends}",
            share.hostname,
            share.source.as_deref().unwrap_or(&share.origin)
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

/// `cloudflared status|install`: the binary Teitunnel runs, found like the app finds it.
async fn cloudflared_command(action: CloudflaredAction) -> Result<ExitCode, String> {
    let binary = context::binary(&context::data_dir()?);
    let found = match action {
        CloudflaredAction::Status => binary.current().await,
        CloudflaredAction::Install => binary.install_latest(|_| {}).await,
    }
    .map_err(|e| match e {
        cloudflared::Error::NotFound => {
            "cloudflared isn't installed. Run `teitunnel cloudflared install`.".to_owned()
        }
        other => other.to_string(),
    })?;
    let version = found
        .version
        .map_or_else(|| "unknown version".to_owned(), |v| v.to_string());
    out!("{}\t{version}", found.path.display())?;
    Ok(ExitCode::SUCCESS)
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

/// `--allow` values as a login rule (see [`AccessRule::from_allow`]); `skip` are the
/// `--skip-login` paths.
fn access_rule(allow: &[String], skip: &[String], sign_in: Option<SignIn>) -> Option<AccessRule> {
    AccessRule::from_allow(allow, skip).map(|rule| AccessRule {
        sign_in: sign_in.unwrap_or(rule.sign_in),
        ..rule
    })
}

/// `--sign-in`: `github`, `google` or `any`.
pub(crate) fn parse_sign_in(input: &str) -> Result<SignIn, String> {
    SignIn::parse(input).ok_or_else(|| format!("`{input}` isn't github, google or any"))
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
        Warning::HeldBy {
            hostname,
            owner,
            until,
            kind,
        } => format!(
            "{} Pass --take-over to take it.",
            teitunnel_core::reservations::describe(&teitunnel_core::engine::Hold {
                hostname: hostname.clone(),
                owner: owner.clone(),
                until: *until,
                kind: *kind,
            })
            .english()
        ),
        Warning::EdgeQuota {
            quota,
            zone,
            used,
            limit,
        } => format!(
            "{zone} will use {used} of the {limit} {} its plan allows.",
            protect::quota_name(*quota)
        ),
        Warning::MachineOnly { domain } => format!(
            "{domain} has no login yet: the new one lets in only service tokens, so people can't open it in a browser."
        ),
        Warning::WorkerRequests { pattern } => format!(
            "Every request to {pattern} runs a Worker, counted against your account's 100,000 free Worker requests a day; past that the site keeps working without it."
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

/// What a plan needs confirming: taking a name someone else holds (`--take-over`), and
/// anything else (`--replace`: records Teitunnel didn't create, public ranges).
fn confirmations(plan: &Plan) -> (bool, bool) {
    let held = plan
        .warnings
        .iter()
        .any(|w| matches!(w, Warning::HeldBy { .. }));
    let other = plan.warnings.iter().any(|w| {
        matches!(
            w,
            Warning::ReplacesForeignRecord { .. }
                | Warning::DeletesForeignRecord { .. }
                | Warning::PublicNetwork { .. }
        )
    });
    (held, other || (plan.requires_confirmation && !held))
}

/// `reservations ls`: the account's reserved hostnames and who holds them.
async fn reservations(app: &App, account: Option<&str>, json: bool) -> Result<ExitCode, String> {
    use teitunnel_core::engine::ownership::format_until;
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let listed = teitunnel_core::reservations::list(&app.engine, &api, &account.id)
        .await
        .map_err(|e| e.to_string())?;
    if json {
        out!(
            "{}",
            serde_json::to_string(&listed).map_err(|e| e.to_string())?
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if listed.cached {
        let _ = writeln!(
            io::stderr().lock(),
            "Cloudflare couldn't be reached; these are the reservations seen last."
        );
    }
    if listed.items.is_empty() {
        out!(
            "No reserved hostnames in {}. Reserve one with `teitunnel reserve <hostname>`.",
            account.name
        )?;
    }
    for r in &listed.items {
        let owner = if r.mine {
            "you".to_owned()
        } else {
            r.owner
                .clone()
                .unwrap_or_else(|| "another Teitunnel".to_owned())
        };
        let until = match (r.ended, r.until) {
            (true, Some(at)) => format!("ended {}", format_until(at)),
            (_, Some(at)) => format!("until {}", format_until(at)),
            (_, None) => "no end date".to_owned(),
        };
        let routed = if r.routed { "\troutes it" } else { "" };
        out!("{}\t{owner}\t{until}{routed}", r.hostname)?;
    }
    Ok(ExitCode::SUCCESS)
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
    let (held, other) = confirmations(&plan);
    if held && !apply.take_over {
        return Ok(share::held(
            "Someone else holds this hostname (see above). Pass --take-over to take it.",
        ));
    }
    if plan.requires_confirmation && other && !apply.replace {
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
        confirmed: apply.replace || apply.take_over,
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
            teitunnel_core::engine::VERIFY_PATIENCE,
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
            share::explain(&result, share::Via::Route, &mut |line| {
                let _ = out!("{line}");
            });
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
    fn parses_share_host_header_flags() {
        use teitunnel_core::quick_share::HostHeaderChoice;
        let parse = |args: &[&str]| {
            let cli = Cli::try_parse_from(["teitunnel", "share", "5173"].iter().chain(args))?;
            let Command::Share {
                host_header,
                no_host_header,
                ..
            } = cli.command
            else {
                panic!("not a share");
            };
            Ok::<_, clap::Error>(share::host_header_choice(host_header, no_host_header))
        };
        assert_eq!(parse(&[]).unwrap(), HostHeaderChoice::Auto);
        assert_eq!(parse(&["--no-host-header"]).unwrap(), HostHeaderChoice::Off);
        assert_eq!(
            parse(&["--host-header", "localhost:5173"]).unwrap(),
            HostHeaderChoice::Set {
                value: "localhost:5173".into()
            }
        );
        assert!(parse(&["--host-header", "a", "--no-host-header"]).is_err());
    }

    #[test]
    fn comments_need_the_inspector_and_folders_choose_their_listing() {
        let share = |args: &[&str]| {
            Cli::try_parse_from(["teitunnel", "share", "5173"].iter().chain(args))
                .map(|c| c.command)
        };
        let Ok(Command::Share { comments, .. }) = share(&["--comments"]) else {
            panic!("--comments parses");
        };
        assert!(comments);
        assert!(
            share(&["--comments", "--no-inspect"]).is_err(),
            "the inspector shows them"
        );
        assert!(share(&["--listing", "--no-listing"]).is_err());
        assert_eq!(listing_choice(false, false), None, "automatic");
        assert_eq!(listing_choice(true, false), Some(true));
        assert_eq!(listing_choice(false, true), Some(false));
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
            "--skip-login",
            "/webhooks",
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
            skip_login,
            origin_options,
            apply,
            ..
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
            access_rule(&allow, &skip_login, None),
            Some(AccessRule {
                emails: vec!["me@xyz.com".into()],
                email_domains: vec!["@team.io".into()],
                bypass: vec!["/webhooks".into()],
                ..AccessRule::default()
            })
        );
        assert_eq!(access_rule(&[], &[], None), None);
        let github = Cli::try_parse_from([
            "teitunnel",
            "route",
            "add",
            "app.example.com",
            "3000",
            "--allow",
            "github:teispace/devs",
            "--sign-in",
            "github",
        ])
        .unwrap_or_else(|e| unreachable!("{e}"));
        let Command::Route(RouteCommand::Add { allow, sign_in, .. }) = github.command else {
            unreachable!()
        };
        let rule = access_rule(&allow, &[], sign_in).unwrap();
        assert_eq!(
            (rule.github.as_slice(), rule.sign_in),
            (["teispace/devs".to_owned()].as_slice(), SignIn::Github)
        );
        assert!(parse_sign_in("okta").is_err());
        assert!(
            Cli::try_parse_from([
                "teitunnel",
                "route",
                "add",
                "app.example.com",
                "3000",
                "--sign-in",
                "google"
            ])
            .is_err(),
            "--sign-in needs --allow"
        );
        // A path can only skip a login the route has.
        assert!(
            Cli::try_parse_from([
                "teitunnel",
                "route",
                "add",
                "a.example.com",
                "3000",
                "--skip-login",
                "/webhooks"
            ])
            .is_err()
        );
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
