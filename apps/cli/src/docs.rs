//! The command-line reference page (`apps/web/content/docs/reference/cli.mdx`), rendered
//! from the clap definitions so it can't drift from the real CLI.
//!
//! `UPDATE_DOCS=1 cargo test -p teitunnel-cli --bin teitunnel-cli docs` rewrites the page;
//! without `UPDATE_DOCS` the test fails when the page is out of date. The prose around
//! the generated part is in `docs/intro.mdx`, the examples and per-command notes are
//! below, and every example is parsed with the real parser.

use std::fmt::Write as _;

use clap::{Arg, ArgAction, Command, CommandFactory};

use super::Cli;

const PAGE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../web/content/docs/reference/cli.mdx"
);

const REGENERATE: &str = "UPDATE_DOCS=1 cargo test -p teitunnel-cli --bin teitunnel-cli docs";

const FRONTMATTER: &str = r#"---
title: Command line
description: "The teitunnel reference: add routes, share ports and manage Cloudflare Tunnel from the terminal, with every command, option and origin setting."
---
"#;

/// Hand-written prose shown before the commands.
const INTRO: &str = include_str!("docs/intro.mdx");

/// Examples per command path (without `teitunnel`). Each line is a command, optionally
/// followed by `# what it does`; everything from a `>`, `<` or `|` on is left out when
/// it's checked against the parser.
const EXAMPLES: &[(&str, &[&str])] = &[
    (
        "setup",
        &["teitunnel setup  # store an API token in the keychain"],
    ),
    (
        "up",
        &[
            "teitunnel up  # run this machine's tunnels (servers, Docker)",
            "teitunnel up --no-project  # ignore the teitunnel.yml in this folder",
        ],
    ),
    (
        "project init",
        &["teitunnel project init  # a starter teitunnel.yml for this folder"],
    ),
    (
        "project check",
        &["teitunnel project check  # every problem in it, with line and column"],
    ),
    (
        "project diff",
        &["teitunnel project diff  # what applying it would change (nothing changes)"],
    ),
    (
        "project apply",
        &["teitunnel project apply  # apply it; its shares run until Ctrl-C"],
    ),
    (
        "project down",
        &[
            "teitunnel project down --remove-routes  # stop its shares, remove the routes it created",
        ],
    ),
    (
        "local-domain add",
        &["teitunnel local-domain add shop.test 3000  # https://shop.test on this computer"],
    ),
    (
        "local-domain trust",
        &["teitunnel local-domain trust  # browsers accept local domains"],
    ),
    (
        "local-domain ls",
        &["teitunnel local-domain ls  # local domains and whether they're served"],
    ),
    (
        "local-domain status",
        &["teitunnel local-domain status  # CA, trust, ports, .test names, what to fix"],
    ),
    (
        "backup create",
        &["teitunnel backup create  # an encrypted copy of this setup (no tokens)"],
    ),
    (
        "backup restore",
        &["teitunnel backup restore teitunnel-setup.teitunnel-backup"],
    ),
    (
        "serve",
        &[
            "teitunnel serve  # tunnels + web dashboard, API and /mcp",
            "teitunnel serve --set-password",
        ],
    ),
    (
        "mcp",
        &["teitunnel mcp  # the MCP server itself (AI tools start it)"],
    ),
    (
        "mcp install",
        &["teitunnel mcp install cursor  # connect an AI tool"],
    ),
    (
        "mcp status",
        &["teitunnel mcp status  # which AI tools are connected"],
    ),
    (
        "api-key create",
        &["teitunnel api-key create deploy  # a key for the API (shown once)"],
    ),
    (
        "always-on",
        &["teitunnel always-on on  # as a service (a system unit as root)"],
    ),
    ("accounts", &["teitunnel accounts --json"]),
    (
        "routes",
        &[
            "teitunnel routes  # this machine's routes and their status",
            "teitunnel routes --check  # exit 1 unless every route is live",
        ],
    ),
    (
        "status",
        &["teitunnel status  # is the app running, and what does it serve?"],
    ),
    ("top", &["teitunnel top  # a live dashboard (q quits)"]),
    (
        "route add",
        &[
            "teitunnel route add app.teispace.com 3000  # route a hostname to localhost:3000",
            "teitunnel route add api.teispace.com 8000 --path '^/v1/'",
            "teitunnel route add admin.teispace.com 3000 --allow team@teispace.com --allow @teispace.com",
            "teitunnel route add beta.teispace.com 4000 --tunnel staging",
            "teitunnel route add secure.teispace.com https://localhost:8443 --no-tls-verify",
            "teitunnel route add dev.teispace.com 5173 --host-header localhost",
            "teitunnel route add slow.teispace.com 8000 --connect-timeout 60 --keep-alive-timeout 300",
        ],
    ),
    (
        "route balance",
        &["teitunnel route balance app.teispace.com  # load balance across machines (paid add-on)"],
    ),
    ("route remove", &["teitunnel route remove app.teispace.com"]),
    (
        "networks",
        &["teitunnel networks  # the ranges this machine shares"],
    ),
    (
        "network add",
        &["teitunnel network add 192.168.1.0/24  # let WARP users reach a range"],
    ),
    (
        "network remove",
        &["teitunnel network remove 192.168.1.0/24"],
    ),
    ("tunnels", &["teitunnel tunnels  # this machine's tunnels"]),
    (
        "tunnel create",
        &["teitunnel tunnel create staging  # another tunnel for this machine"],
    ),
    ("tunnel delete", &["teitunnel tunnel delete staging"]),
    (
        "share",
        &[
            "teitunnel share 3000  # a temporary public URL (in the app when it runs)",
            "teitunnel share 3000 --here --for 30m  # in this terminal, until Ctrl-C or 30 minutes",
            "teitunnel share 3000 --on demo.teispace.com  # the same, on your own domain",
            "teitunnel share 3000 --on {branch}.dev.teispace.com  # named after the git branch",
            "teitunnel share 3000 --on  # the name used in this folder last time",
            "teitunnel share ./dist  # share a folder of files",
            "teitunnel share ./dist --on docs.teispace.com --spa",
            "teitunnel share 3000 --no-inspect  # skip the inspector (on by default)",
            "teitunnel share 8000 --mcp --on mcp.teispace.com  # an MCP server for remote AI clients",
            "teitunnel share 11434 --ai  # a local AI server behind a bearer token",
        ],
    ),
    (
        "inspect",
        &[
            "teitunnel inspect app.teispace.com  # a route's requests, until Ctrl-C",
            "teitunnel inspect app.teispace.com --off  # restore a route left inspected",
        ],
    ),
    ("traffic ls", &["teitunnel traffic ls --status 5xx"]),
    (
        "traffic get",
        &["teitunnel traffic get 7f3a9c1b --format curl"],
    ),
    (
        "traffic watch",
        &["teitunnel traffic watch --path /webhooks"],
    ),
    (
        "traffic replay",
        &["teitunnel traffic replay 7f3a9c1b --set-header 'X-Debug: 1'"],
    ),
    (
        "traffic export",
        &["teitunnel traffic export --har requests.har"],
    ),
    ("traffic clear", &["teitunnel traffic clear"]),
    (
        "traffic openapi",
        &["teitunnel traffic openapi --out api.yaml  # an OpenAPI 3.1 file from captured requests"],
    ),
    (
        "token",
        &["teitunnel token mcp.teispace.com  # the token a shared MCP or AI server expects"],
    ),
    (
        "reserve",
        &[
            "teitunnel reserve review.dev.teispace.com --until 2026-12-31  # hold a name for yourself",
        ],
    ),
    (
        "reservations",
        &["teitunnel reservations ls  # reserved names and who holds them"],
    ),
    ("release", &["teitunnel release review.dev.teispace.com"]),
    (
        "shares",
        &[
            "teitunnel shares  # every share: the app's, terminals', your domains'",
            "teitunnel shares --stop demo.teispace.com",
            "teitunnel shares --pause demo.teispace.com  # a paused page; the address stays",
            "teitunnel shares --resume demo.teispace.com",
        ],
    ),
    (
        "schedule",
        &[
            "teitunnel schedule demo.teispace.com mon-fri 09:00-18:00  # on during office hours",
            "teitunnel schedule demo.teispace.com --off",
        ],
    ),
    (
        "schedules",
        &["teitunnel schedules  # every schedule, and when each changes next"],
    ),
    (
        "analytics",
        &[
            "teitunnel analytics  # traffic of every route, from Cloudflare's edge",
            "teitunnel analytics app.teispace.com --range week",
        ],
    ),
    (
        "uptime",
        &["teitunnel uptime  # uptime of this machine's routes"],
    ),
    (
        "snapshot publish",
        &[
            "teitunnel snapshot publish ./dist --on preview.teispace.com  # a copy online while you sleep",
            "teitunnel snapshot publish . --build  # build the project, then publish its output",
            "teitunnel snapshot publish ./dist --on preview.teispace.com --comments  # reviewers can comment",
            "teitunnel snapshot publish dist --name web-pr-7 --on pr-7.teispace.com --or-update --yes --json  # CI",
        ],
    ),
    (
        "snapshot ls",
        &["teitunnel snapshot ls  # Snapshots, their addresses and versions"],
    ),
    (
        "snapshot rollback",
        &["teitunnel snapshot rollback preview.teispace.com"],
    ),
    (
        "snapshot rm",
        &["teitunnel snapshot rm pr-7.teispace.com --missing-ok --yes"],
    ),
    (
        "protect",
        &[
            "teitunnel protect app.teispace.com --bots challenge --block-ai  # rules at Cloudflare's edge",
            "teitunnel protect app.teispace.com --rate-limit 30/1m  # Pro plans and up",
            "teitunnel protect app.teispace.com --off  # remove Teitunnel's rules",
        ],
    ),
    (
        "service-token create",
        &[
            "teitunnel service-token create api.teispace.com --name CI  # the secret is printed once",
        ],
    ),
    (
        "service-token ls",
        &["teitunnel service-token ls api.teispace.com"],
    ),
    (
        "service-token revoke",
        &["teitunnel service-token revoke api.teispace.com CI"],
    ),
    (
        "comments ls",
        &[
            "teitunnel comments ls  # shares and Snapshots with review comments",
            "teitunnel comments ls preview.teispace.com --all  # its threads, resolved ones too",
        ],
    ),
    (
        "comments reply",
        &["teitunnel comments reply preview.teispace.com c1a2b3 \"Fixed, thanks\""],
    ),
    (
        "comments resolve",
        &["teitunnel comments resolve preview.teispace.com c1a2b3"],
    ),
    (
        "offline",
        &[
            "teitunnel offline app.teispace.com --title \"Back soon\"  # your page instead of error 1033",
            "teitunnel offline app.teispace.com --off",
        ],
    ),
    (
        "inbox add",
        &[
            "teitunnel inbox add app.teispace.com /webhooks/ --days 7  # keep webhooks while you're away",
        ],
    ),
    (
        "inbox secret",
        &["teitunnel inbox secret app.teispace.com stripe < secret.txt  # keychain; for --verify"],
    ),
    (
        "inbox items",
        &["teitunnel inbox items app.teispace.com /webhooks/  # arrivals and deliveries"],
    ),
    (
        "inbox deliver",
        &["teitunnel inbox deliver  # deliver waiting webhooks now"],
    ),
    (
        "inbox rm",
        &["teitunnel inbox rm app.teispace.com /webhooks/"],
    ),
    (
        "doctor",
        &[
            "teitunnel doctor  # check for problems; exits 1 on an error",
            "teitunnel doctor --fix  # apply the safe fixes",
        ],
    ),
    (
        "cloudflared",
        &["teitunnel cloudflared install  # a verified cloudflared (servers, CI)"],
    ),
    (
        "completions",
        &["teitunnel completions zsh > _teitunnel  # or save it as _teitunnel on your fpath"],
    ),
    ("export", &["teitunnel export terraform > teitunnel.tf"]),
];

/// Extra prose after a command's reference (MDX).
const NOTES: &[(&str, &str)] = &[
    (
        "setup",
        "See [Accounts and permissions](/docs/guides/accounts/) and \
         [Permissions and scopes](/docs/reference/permissions/) for what the token needs, and \
         [Servers and containers](/docs/guides/servers/) for giving it in the environment.",
    ),
    (
        "up",
        "See [Servers and containers](/docs/guides/servers/) and \
         [Project files](/docs/guides/project-file/).",
    ),
    (
        "project",
        "See [Project files](/docs/guides/project-file/).",
    ),
    (
        "local-domain",
        "See [Local domains](/docs/guides/local-domains/).",
    ),
    (
        "backup",
        "See [Move to a new computer](/docs/guides/move-computer/).",
    ),
    (
        "serve",
        "See [Servers and containers](/docs/guides/servers/).",
    ),
    ("mcp", "See [AI agents](/docs/guides/ai-agents/)."),
    ("always-on", "See [Run modes](/docs/concepts/run-modes/)."),
    (
        "route add",
        "`--allow` puts a login in front of the route ([Require a login](/docs/guides/require-login/)); \
         `--tunnel` picks one of this machine's tunnels ([Several tunnels](/docs/guides/several-tunnels/)). \
         After adding a route, the command checks it through Cloudflare and exits with status 1 \
         if it doesn't work yet.\n\n\
         The origin settings are cloudflared's `originRequest` settings, the same as the app's \
         (leave them out for cloudflared's defaults):\n\n\
         | Flag | Setting |\n\
         |---|---|\n\
         | `--host-header` | `httpHostHeader` |\n\
         | `--no-tls-verify` | `noTLSVerify` |\n\
         | `--origin-server-name` | `originServerName` |\n\
         | `--match-sni-to-host` | `matchSNItoHost` |\n\
         | `--ca-pool` | `caPool` |\n\
         | `--http2-origin` | `http2Origin` |\n\
         | `--disable-chunked-encoding` | `disableChunkedEncoding` |\n\
         | `--connect-timeout` | `connectTimeout` |\n\
         | `--tls-timeout` | `tlsTimeout` |\n\
         | `--tcp-keep-alive` | `tcpKeepAlive` |\n\
         | `--keep-alive-timeout` | `keepAliveTimeout` |\n\
         | `--keep-alive-connections` | `keepAliveConnections` |\n\
         | `--no-happy-eyeballs` | `noHappyEyeballs` |\n\
         | `--proxy-type` | `proxyType` |",
    ),
    (
        "route balance",
        "See [Load balancing](/docs/guides/load-balancing/).",
    ),
    (
        "network",
        "See [Private networks](/docs/guides/private-networks/).",
    ),
    (
        "tunnel",
        "See [Several tunnels](/docs/guides/several-tunnels/).",
    ),
    (
        "share",
        "See [Sharing](/docs/guides/sharing/), [Dev servers](/docs/tutorials/dev-servers/) \
         for `--host-header`, the [inspector](/docs/guides/inspector/) and the \
         [exposure check](/docs/guides/exposure-check/).",
    ),
    ("inspect", "See [Inspector](/docs/guides/inspector/)."),
    (
        "traffic",
        "See [Inspector](/docs/guides/inspector/) and [OpenAPI from traffic](/docs/guides/openapi/).",
    ),
    (
        "reserve",
        "See [Teams sharing one account](/docs/guides/ci-previews/#teams-sharing-one-account).",
    ),
    ("analytics", "See [Analytics](/docs/guides/analytics/)."),
    ("snapshot", "See [Snapshots](/docs/guides/snapshots/)."),
    (
        "protect",
        "See [Edge protection](/docs/guides/protection/).",
    ),
    (
        "service-token",
        "See [Edge protection](/docs/guides/protection/).",
    ),
    ("comments", "See [Comments](/docs/guides/comments/)."),
    ("offline", "See [Offline page](/docs/guides/offline-page/)."),
    ("inbox", "See [Webhook inbox](/docs/guides/webhook-inbox/)."),
    (
        "doctor",
        "It runs the app's checks and hides the issues you ignored there. See \
         [Doctor](/docs/reference/doctor/).",
    ),
    ("export", "See [Export](/docs/guides/export/)."),
    (
        "top",
        "`top` shows your shares (address, service, age, requests per second), routes (a status \
         dot, where they go, uptime over 24 hours), traffic, and the requests of a share the app \
         inspects. It reads from the app when it runs; otherwise from this machine's records \
         (then you can see and stop terminals' shares, but not start new ones). It follows the \
         terminal's size and uses no colour when `NO_COLOR` is set.\n\n\
         | Key | Does |\n\
         |---|---|\n\
         | `q`, `Esc` | quit (`Esc` first clears a filter) |\n\
         | `Tab` | next pane |\n\
         | `↑` `↓`, `j` `k` | select |\n\
         | `s` | share a port (through the app) |\n\
         | `x` `x` | stop the selected share (press twice) |\n\
         | `c` | copy the selected URL (through the terminal, so it works over SSH too) |\n\
         | `/` | filter everything by text |\n\
         | `?` | the keys |",
    ),
    (
        "completions",
        "To load them in every new shell:\n\n\
         ```sh\n\
         source <(teitunnel completions zsh)                  # zsh, in ~/.zshrc\n\
         eval \"$(teitunnel completions bash)\"                 # bash, in ~/.bashrc\n\
         teitunnel completions fish > ~/.config/fish/completions/teitunnel.fish\n\
         teitunnel completions powershell | Out-String | Invoke-Expression  # in $PROFILE\n\
         ```\n\n\
         The script asks `teitunnel` for suggestions on every Tab, from this machine's records \
         without going online. `--static` prints a script with only commands and flags that \
         never runs `teitunnel`; Elvish gets that one.",
    ),
];

#[test]
fn the_reference_page_is_up_to_date() {
    let page = render();
    if std::env::var_os("UPDATE_DOCS").is_some() {
        std::fs::write(PAGE, &page).unwrap();
        return;
    }
    // Windows checkouts may turn line endings into CRLF.
    let current = std::fs::read_to_string(PAGE)
        .unwrap_or_default()
        .replace("\r\n", "\n");
    assert!(
        current == page,
        "CLI reference is out of date: run `{REGENERATE}`"
    );
}

#[test]
fn every_example_parses_as_its_command() {
    for (path, examples) in EXAMPLES {
        for example in *examples {
            let words = words(example);
            assert_eq!(
                words.first().map(String::as_str),
                Some("teitunnel"),
                "{example}"
            );
            let matches = Cli::command()
                .try_get_matches_from(&words)
                .unwrap_or_else(|e| panic!("`{example}` doesn't parse:\n{e}"));
            let mut parsed = Vec::new();
            let mut at = &matches;
            while let Some((name, sub)) = at.subcommand() {
                parsed.push(name);
                at = sub;
            }
            assert_eq!(
                parsed.join(" "),
                *path,
                "`{example}` is filed under the wrong command"
            );
        }
    }
}

#[test]
fn examples_and_notes_name_real_commands() {
    let mut root = Cli::command();
    root.build();
    let mut paths = Vec::new();
    collect_paths(&root, "", &mut paths);
    let named = EXAMPLES
        .iter()
        .map(|(p, _)| *p)
        .chain(NOTES.iter().map(|(p, _)| *p));
    for path in named {
        assert!(
            paths.iter().any(|p| p == path),
            "no visible command `{path}`"
        );
    }
    // One entry per command, so a later one can't silently shadow another.
    for list in [
        EXAMPLES.iter().map(|(p, _)| *p).collect::<Vec<_>>(),
        NOTES.iter().map(|(p, _)| *p).collect(),
    ] {
        let mut sorted = list.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), list.len(), "a command is listed twice");
    }
}

#[test]
fn escapes_mdx_outside_code() {
    assert_eq!(text("a <b> {c} `<d> {e}`"), r"a \<b\> \{c\} `<d> {e}`");
    assert_eq!(cell("`a|b` c|d"), r"`a\|b` c\|d");
}

fn collect_paths(command: &Command, prefix: &str, out: &mut Vec<String>) {
    for sub in visible_subcommands(command) {
        let path = format!("{prefix}{}", sub.get_name());
        collect_paths(sub, &format!("{path} "), out);
        out.push(path);
    }
}

/// Splits a shell line into words, like `sh` would for these simple lines: quotes group,
/// a `#` word starts a comment, and a `>`, `<` or `|` word ends the command.
fn words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    for c in line.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    let end = words
        .iter()
        .position(|w| matches!(w.as_str(), "#" | ">" | "<" | "|"))
        .unwrap_or(words.len());
    words.truncate(end);
    words
}

fn visible_subcommands(command: &Command) -> impl Iterator<Item = &Command> {
    command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set() && sub.get_name() != "help")
}

fn render() -> String {
    let mut root = Cli::command();
    root.build();
    let mut page = String::new();
    page.push_str(FRONTMATTER);
    let _ = write!(
        page,
        "\n{{/* Generated from the CLI's definitions (apps/cli). Edit the help text there, \
         and the prose in apps/cli/src/docs/intro.mdx or apps/cli/src/docs.rs, then run: \
         {REGENERATE} */}}\n\n"
    );
    page.push_str(INTRO.trim());
    page.push_str("\n\n## Commands\n\n| Command | What it does |\n|---|---|\n");
    for sub in visible_subcommands(&root) {
        let _ = writeln!(
            page,
            "| [`{name}`](#{name}) | {about} |",
            name = sub.get_name(),
            about = cell(&about(sub)),
        );
    }
    page.push_str(
        "\nEvery command takes `-h` (a summary) and `--help` (the full help), and \
         `teitunnel --version` prints the version.\n",
    );
    for sub in visible_subcommands(&root) {
        render_command(&mut page, sub, sub.get_name(), 2);
    }
    // No runs of blank lines, and one trailing newline.
    while page.contains("\n\n\n") {
        page = page.replace("\n\n\n", "\n\n");
    }
    let trimmed = page.trim_end().len();
    page.truncate(trimmed);
    page.push('\n');
    page
}

fn render_command(page: &mut String, command: &Command, path: &str, level: usize) {
    let _ = write!(page, "\n{} {path}\n\n", "#".repeat(level.min(4)));
    let description = command
        .get_long_about()
        .or_else(|| command.get_about())
        .map(ToString::to_string)
        .unwrap_or_default();
    for paragraph in description.split("\n\n") {
        let _ = write!(
            page,
            "{}\n\n",
            text(&sentence(&paragraph.replace('\n', " ")))
        );
    }
    let aliases: Vec<_> = command.get_visible_aliases().collect();
    if !aliases.is_empty() {
        let list: Vec<_> = aliases.iter().map(|a| format!("`{a}`")).collect();
        let _ = write!(page, "Also: {}.\n\n", list.join(", "));
    }
    let usage = command.clone().render_usage().to_string();
    let usage: Vec<_> = usage
        .lines()
        .map(|line| line.trim().trim_start_matches("Usage:").trim())
        .filter(|line| !line.is_empty())
        .collect();
    let _ = write!(page, "```sh\n{}\n```\n\n", usage.join("\n"));

    render_arguments(page, command);

    let subs: Vec<_> = visible_subcommands(command).collect();
    if !subs.is_empty() {
        page.push_str("| Command | What it does |\n|---|---|\n");
        for sub in &subs {
            let sub_path = format!("{path} {}", sub.get_name());
            let _ = writeln!(
                page,
                "| [`{sub_path}`](#{anchor}) | {about} |",
                anchor = sub_path.replace(' ', "-"),
                about = cell(&about(sub)),
            );
        }
        page.push('\n');
    }

    if let Some((_, examples)) = EXAMPLES.iter().find(|(p, _)| *p == path) {
        page.push_str("Examples:\n\n```sh\n");
        let width = examples
            .iter()
            .filter_map(|e| e.split_once("  # ").map(|(c, _)| c.chars().count()))
            .max()
            .unwrap_or(0);
        for example in *examples {
            match example.split_once("  # ") {
                Some((command, comment)) => {
                    let pad = width - command.chars().count();
                    let _ = writeln!(page, "{command}{}  # {comment}", " ".repeat(pad));
                }
                None => {
                    let _ = writeln!(page, "{example}");
                }
            }
        }
        page.push_str("```\n\n");
    }
    if let Some((_, note)) = NOTES.iter().find(|(p, _)| *p == path) {
        let _ = write!(page, "{note}\n\n");
    }

    for sub in subs {
        render_command(page, sub, &format!("{path} {}", sub.get_name()), level + 1);
    }
}

fn about(command: &Command) -> String {
    let about = command
        .get_about()
        .map(ToString::to_string)
        .unwrap_or_default();
    sentence(&about.replace('\n', " "))
}

fn render_arguments(page: &mut String, command: &Command) {
    let args: Vec<&Arg> = command
        .get_arguments()
        .filter(|arg| {
            !arg.is_hide_set()
                && !matches!(
                    arg.get_action(),
                    ArgAction::Help
                        | ArgAction::HelpShort
                        | ArgAction::HelpLong
                        | ArgAction::Version
                )
        })
        .collect();
    let positionals: Vec<_> = args.iter().filter(|a| a.is_positional()).collect();
    if !positionals.is_empty() {
        page.push_str("| Argument | Description |\n|---|---|\n");
        for arg in positionals {
            let _ = writeln!(
                page,
                "| {} | {} |",
                cell(&format!("`{}`", positional(arg))),
                cell(&describe(arg))
            );
        }
        page.push('\n');
    }
    // Options grouped by help heading, headings in order of first appearance.
    let mut headings: Vec<Option<&str>> = Vec::new();
    for arg in args.iter().filter(|a| !a.is_positional()) {
        if !headings.contains(&arg.get_help_heading()) {
            headings.push(arg.get_help_heading());
        }
    }
    for heading in headings {
        if let Some(heading) = heading {
            let _ = write!(page, "**{}**\n\n", text(heading));
        }
        page.push_str("| Option | Description |\n|---|---|\n");
        for arg in args
            .iter()
            .filter(|a| !a.is_positional() && a.get_help_heading() == heading)
        {
            let _ = writeln!(
                page,
                "| {} | {} |",
                cell(&format!("`{}`", option(arg))),
                cell(&describe(arg))
            );
        }
        page.push('\n');
    }
}

fn value_names(arg: &Arg) -> Vec<String> {
    match arg.get_value_names() {
        Some(names) => names.iter().map(ToString::to_string).collect(),
        None => vec![arg.get_id().as_str().to_uppercase()],
    }
}

fn positional(arg: &Arg) -> String {
    let name = value_names(arg).join(" ");
    let many = arg
        .get_num_args()
        .is_some_and(|range| range.max_values() > 1);
    let dots = if many { "..." } else { "" };
    if arg.is_required_set() {
        format!("<{name}>{dots}")
    } else {
        format!("[{name}]{dots}")
    }
}

fn option(arg: &Arg) -> String {
    let mut flags = Vec::new();
    if let Some(short) = arg.get_short() {
        flags.push(format!("-{short}"));
    }
    if let Some(long) = arg.get_long() {
        flags.push(format!("--{long}"));
    }
    for alias in arg.get_visible_short_aliases().unwrap_or_default() {
        flags.push(format!("-{alias}"));
    }
    for alias in arg.get_visible_aliases().unwrap_or_default() {
        flags.push(format!("--{alias}"));
    }
    let mut out = flags.join(", ");
    if arg.get_action().takes_values() {
        let name = value_names(arg).join(" ");
        let optional = arg
            .get_num_args()
            .is_some_and(|range| range.min_values() == 0);
        if optional {
            let _ = write!(out, " [{name}]");
        } else {
            let _ = write!(out, " <{name}>");
        }
    }
    out
}

fn describe(arg: &Arg) -> String {
    let help = arg
        .get_long_help()
        .or_else(|| arg.get_help())
        .map(ToString::to_string)
        .unwrap_or_default()
        .replace("\n\n", " ")
        .replace('\n', " ");
    let mut out = sentence(&help);
    if !arg.get_action().takes_values() {
        return out;
    }
    // Values the help doesn't already name.
    let values: Vec<_> = arg
        .get_possible_values()
        .into_iter()
        .filter(|v| !v.is_hide_set())
        .collect();
    let named = values.iter().all(|v| help.contains(v.get_name()));
    if !values.is_empty() && !(named && values.iter().all(|v| v.get_help().is_none())) {
        let list: Vec<_> = values
            .iter()
            .map(|v| match v.get_help() {
                Some(value_help) => format!("`{}`: {value_help}", v.get_name()),
                None => format!("`{}`", v.get_name()),
            })
            .collect();
        let helped = values.iter().any(|v| v.get_help().is_some());
        let _ = write!(
            out,
            " Values: {}.",
            list.join(if helped { "; " } else { ", " })
        );
    }
    let defaults: Vec<_> = arg
        .get_default_values()
        .iter()
        .map(|v| format!("`{}`", v.to_string_lossy()))
        .collect();
    if !defaults.is_empty() && !help.to_lowercase().contains("default") {
        let _ = write!(out, " Default: {}.", defaults.join(", "));
    }
    if !arg.is_positional()
        && matches!(arg.get_action(), ArgAction::Append)
        && !help.contains("epeat")
    {
        out.push_str(" Repeatable.");
    }
    out.trim().to_owned()
}

/// Clap drops a one-line help's final period; the page puts it back.
fn sentence(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.ends_with(['.', '?', '!', ':']) {
        value.to_owned()
    } else {
        format!("{value}.")
    }
}

/// Text for a table cell: also escapes the pipes (in code spans too, as GFM tables need).
fn cell(value: &str) -> String {
    text(value).replace('|', r"\|")
}

/// Escapes what MDX would read as JSX or expressions, outside code spans.
fn text(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut in_code = false;
    for c in value.chars() {
        match c {
            '`' => {
                in_code = !in_code;
                out.push(c);
            }
            '<' | '>' | '{' | '}' if !in_code => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
