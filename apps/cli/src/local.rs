//! `teitunnel local-domain add|ls|rm|serve|trust|untrust|status`: local HTTPS domains
//! (`https://shop.test`) on this computer (M12-07). The registry is the app's database;
//! when the app runs it serves them (it's asked to reload), otherwise `add` and `serve`
//! serve them from this terminal until Ctrl-C. Nothing here touches Cloudflare.

use std::{path::Path, process::ExitCode, sync::Arc, time::Duration};

use clap::Subcommand;
use teitunnel_control::{
    ControlClient,
    protocol::{LocalDomainInfo, LocalDomainsInfo},
};
use teitunnel_core::{
    inspect::Inspector,
    local_domains::{
        LocalDomainInput, LocalDomains, LocalDomainsConfig, LocalDomainsStatus, NameResolution,
        TrustOptions, TrustState, TrustView,
    },
    secrets::{KeychainStore, Secrets},
    store::Store,
    text::UserText as _,
};

use crate::{app, context, share::status as note};

/// `teitunnel local-domain …`.
#[derive(Debug, Subcommand)]
pub(crate) enum LocalCommand {
    /// Give a local service an HTTPS address like https://shop.test.
    ///
    /// Names end in .localhost (works in browsers with no setup), .test (every tool,
    /// after a one-time resolver entry) or .local (also phones on your network). Served
    /// by the Teitunnel app when it runs, otherwise from this terminal until Ctrl-C.
    Add {
        /// The name, e.g. shop.test (".localhost" is added when there's no suffix).
        name: String,
        /// The service: a port (3000), host:port, or a URL (https://localhost:5173).
        target: String,
        /// Subdomains (*.name) go to the same service.
        #[arg(long)]
        wildcard: bool,
        /// Plain HTTP only (no certificate).
        #[arg(long)]
        no_https: bool,
        /// Record its requests (see `teitunnel traffic`).
        #[arg(long)]
        inspect: bool,
        /// Only save it; don't serve it from this terminal when the app isn't running.
        #[arg(long, conflicts_with = "here")]
        no_serve: bool,
        /// Serve it from this terminal even if the app runs.
        #[arg(long)]
        here: bool,
    },
    /// List local domains and whether they're served.
    #[command(visible_alias = "list")]
    Ls {
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
    /// Remove a local domain.
    #[command(visible_alias = "remove")]
    Rm {
        /// Its name.
        name: String,
    },
    /// Serve every local domain from this terminal until Ctrl-C (or hand them to the
    /// running app).
    Serve {
        /// Serve from this terminal even if the app runs.
        #[arg(long)]
        here: bool,
    },
    /// Trust Teitunnel's local certificate authority, so browsers accept local domains.
    ///
    /// macOS asks for your password; Windows asks you to confirm. On Linux the system
    /// store needs an administrator: the commands to run are printed.
    Trust {
        /// Also add it to Chrome's and Firefox's own certificate databases (NSS).
        #[arg(long)]
        browsers: bool,
        /// Turn on "trust the system's certificates" in Firefox profiles.
        #[arg(long)]
        firefox: bool,
    },
    /// Stop trusting the local certificate authority.
    Untrust {
        /// Also delete it (a new one is made the next time it's needed).
        #[arg(long)]
        forget: bool,
    },
    /// Certificates, trust, ports, .test names and each domain, with what to fix.
    Status {
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
}

/// The registry and a service over it in this process (not serving until asked).
fn open(dir: &Path) -> Result<LocalDomains, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let store = Store::open(&dir.join("teitunnel.db")).map_err(|e| e.to_string())?;
    let secrets: Secrets = Arc::new(KeychainStore);
    let inspector = Inspector::new(
        Some(store.clone()),
        Some(Arc::clone(&secrets)),
        &teitunnel_core::runtime::this_process(),
    );
    Ok(LocalDomains::new(
        store,
        inspector,
        LocalDomainsConfig::detect(dir, Some(secrets)),
    ))
}

fn info(status: &LocalDomainsStatus) -> LocalDomainsInfo {
    LocalDomainsInfo {
        running: status.running,
        https_port: status.https_port,
        http_port: status.http_port,
        error: status
            .error
            .as_ref()
            .map(teitunnel_core::text::Text::english),
        domains: status
            .domains
            .iter()
            .map(|d| LocalDomainInfo {
                name: d.name.clone(),
                url: d.url.clone(),
                origin: d.origin.clone(),
                wildcard: d.wildcard,
                https: d.https,
                inspect: d.inspect,
                serving: d.serving,
            })
            .collect(),
    }
}

fn print_list(list: &LocalDomainsInfo, json: bool) -> Result<(), String> {
    if json {
        out!(
            "{}",
            serde_json::to_string_pretty(list).map_err(|e| e.to_string())?
        )?;
        return Ok(());
    }
    if list.domains.is_empty() {
        out!("No local domains. Add one with `teitunnel local-domain add shop.test 3000`.")?;
        return Ok(());
    }
    for domain in &list.domains {
        let mut flags = Vec::new();
        if domain.wildcard {
            flags.push("subdomains");
        }
        if domain.inspect {
            flags.push("inspected");
        }
        flags.push(if domain.serving {
            "serving"
        } else {
            "not served"
        });
        out!(
            "{}\t-> {}\t{}",
            domain.url,
            domain.origin.as_deref().unwrap_or("?"),
            flags.join(", ")
        )?;
    }
    if let Some(error) = &list.error {
        out!("\n{error}")?;
    }
    Ok(())
}

/// Asks the running app to serve what's saved; `None` when it doesn't run (or `here`).
async fn via_app(dir: &Path, here: bool) -> Result<Option<LocalDomainsInfo>, String> {
    let Some(client): Option<ControlClient> =
        app::connect(dir, app::Where::from_flags(false, here)).await?
    else {
        return Ok(None);
    };
    match client.reload_local_domains().await {
        Ok(list) => Ok(Some(list)),
        // An app from before local domains: serve from here.
        Err(err) if err.code() == Some(teitunnel_control::protocol::code::METHOD_NOT_FOUND) => {
            Ok(None)
        }
        Err(err) => Err(app::describe(&err)),
    }
}

/// The Doctor's local-domain issues seen from here (trust, `.test` names, certificates);
/// none when there are no local domains or the database can't be opened.
pub(crate) async fn issues(dir: &Path) -> Vec<teitunnel_core::doctor::Issue> {
    if !dir.join("teitunnel.db").exists() {
        return Vec::new();
    }
    match open(dir) {
        Ok(local) => local.doctor().await,
        Err(_) => Vec::new(),
    }
}

/// After a project saved local domains: the running app serves them at once; otherwise
/// says how to serve them.
pub(crate) async fn announce_saved(
    dir: &Path,
    saved: &[teitunnel_core::project::LocalDomainAction],
) -> Result<(), String> {
    let names: Vec<&str> = saved.iter().map(|d| d.name.as_str()).collect();
    if via_app(dir, false).await?.is_some() {
        out!(
            "Local domains served by the Teitunnel app: {}",
            names.join(", ")
        )?;
    } else {
        out!(
            "Saved local domains {}. They're served while the Teitunnel app runs, or with `teitunnel local-domain serve`.",
            names.join(", ")
        )?;
    }
    Ok(())
}

/// Serves from this process until Ctrl-C.
async fn serve_here(local: &LocalDomains) -> Result<ExitCode, String> {
    let _ = local.sync().await;
    let status = local.status().await;
    if !status.running {
        let why = status
            .error
            .map_or_else(|| "nothing to serve".to_owned(), |e| e.english());
        return Err(format!("Local domains couldn't start: {why}"));
    }
    print_list(&info(&status), false)?;
    if let Some(port) = status.https_port {
        note(&format!("HTTPS on port {port}"));
    }
    if let Some(port) = status.http_port {
        note(&format!("Plain HTTP on port {port}"));
    }
    for issue in local.doctor().await {
        note(&format!("! {}", issue.title.english()));
    }
    note("Serving local domains until Ctrl-C.");
    let upkeep = tokio::spawn(local.clone().run());
    let _ = tokio::signal::ctrl_c().await;
    local.stop().await;
    let _ = tokio::time::timeout(Duration::from_secs(2), upkeep).await;
    Ok(ExitCode::SUCCESS)
}

fn state_word(state: &TrustState) -> String {
    match state {
        TrustState::Trusted => "trusted".into(),
        TrustState::NotTrusted => "added, not trusted".into(),
        TrustState::Absent => "not added".into(),
        TrustState::FollowsSystem => "uses the system's trust".into(),
        TrustState::Disabled => "doesn't use the system's trust".into(),
        TrustState::Unsupported => "no supported store".into(),
        TrustState::ToolMissing { tool, package } => match package {
            Some(package) => format!("needs {tool} (install {package})"),
            None => format!("needs {tool}"),
        },
        TrustState::Error { message } => format!("couldn't check: {message}"),
    }
}

fn print_trust(view: &TrustView) -> Result<(), String> {
    if let Some(ca) = &view.ca {
        out!(
            "Certificate authority: {} (SHA-256 {})",
            ca.common_name,
            ca.sha256
        )?;
    }
    for store in &view.stores {
        let path = store
            .path
            .as_deref()
            .map(|p| format!(" {p}"))
            .unwrap_or_default();
        out!("  {:?}{path}: {}", store.kind, state_word(&store.state))?;
    }
    if !view.steps.is_empty() {
        out!("\nThe system store needs an administrator. Run:")?;
        for step in &view.steps {
            out!("{}", step.command)?;
        }
    }
    out!(
        "{}",
        if view.trusted {
            "Browsers trust your local domains."
        } else {
            "Not trusted by the system yet."
        }
    )?;
    Ok(())
}

pub(crate) async fn run(command: LocalCommand) -> Result<ExitCode, String> {
    let dir = context::data_dir()?;
    let local = open(&dir)?;
    match command {
        LocalCommand::Add {
            name,
            target,
            wildcard,
            no_https,
            inspect,
            no_serve,
            here,
        } => {
            let input = LocalDomainInput {
                name: teitunnel_core::local_domains::complete_name(&name),
                target,
                wildcard,
                https: !no_https,
                inspect,
            };
            let row = local
                .save_new(&input)
                .await
                .map_err(|e| e.text().english())?;
            if let Some(list) = via_app(&dir, here).await? {
                let url = list
                    .domains
                    .iter()
                    .find(|d| d.name == row.name.as_str())
                    .map(|d| d.url.clone())
                    .unwrap_or_default();
                out!("{url}")?;
                note("Served by the Teitunnel app.");
                return Ok(ExitCode::SUCCESS);
            }
            if no_serve {
                note(&format!(
                    "Saved {}. It's served while the Teitunnel app runs, or with `teitunnel local-domain serve`.",
                    row.name
                ));
                return Ok(ExitCode::SUCCESS);
            }
            serve_here(&local).await
        }
        LocalCommand::Ls { json } => {
            let client = app::connect(&dir, app::Where::Auto).await?;
            let from_app = match &client {
                Some(client) => client.local_domains().await.ok(),
                None => None,
            };
            let list = match from_app {
                Some(list) => list,
                None => info(&local.status().await),
            };
            print_list(&list, json)?;
            Ok(ExitCode::SUCCESS)
        }
        LocalCommand::Rm { name } => {
            let name = teitunnel_core::local_domains::complete_name(&name);
            local.forget(&name).await.map_err(|e| e.text().english())?;
            let _ = via_app(&dir, false).await?;
            note(&format!("Removed {name}."));
            Ok(ExitCode::SUCCESS)
        }
        LocalCommand::Serve { here } => {
            if let Some(list) = via_app(&dir, here).await? {
                print_list(&list, false)?;
                note("Served by the Teitunnel app.");
                return Ok(ExitCode::SUCCESS);
            }
            serve_here(&local).await
        }
        LocalCommand::Trust { browsers, firefox } => {
            let view = local
                .trust(TrustOptions {
                    browsers,
                    firefox_system_roots: firefox,
                })
                .await
                .map_err(|e| e.text().english())?;
            print_trust(&view)?;
            Ok(if view.trusted {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        LocalCommand::Untrust { forget } => {
            let view = local
                .untrust(forget)
                .await
                .map_err(|e| e.text().english())?;
            if forget {
                out!("Removed the local certificate authority.")?;
            }
            print_trust(&view)?;
            Ok(ExitCode::SUCCESS)
        }
        LocalCommand::Status { json } => {
            let status = local.status().await;
            let trust = local.trust_status().await;
            let issues = teitunnel_core::local_domains::diagnose(&local.facts().await);
            if json {
                let value = serde_json::json!({
                    "status": status,
                    "trust": trust,
                    "issues": issues.iter().map(|i| serde_json::json!({
                        "check": i.check,
                        "title": i.title.english(),
                        "detail": i.detail.english(),
                    })).collect::<Vec<_>>(),
                });
                out!(
                    "{}",
                    serde_json::to_string_pretty(&value).map_err(|e| e.to_string())?
                )?;
                return Ok(ExitCode::SUCCESS);
            }
            print_list(&info(&status), false)?;
            for domain in &status.domains {
                match domain.resolution {
                    NameResolution::NeedsResolver => {
                        out!("{} doesn't resolve to this computer yet.", domain.name)?;
                    }
                    NameResolution::Elsewhere => {
                        out!("{} resolves to another computer.", domain.name)?;
                    }
                    NameResolution::Ok | NameResolution::Unchecked => {}
                }
            }
            if status.resolver.needed && !status.resolver.configured {
                out!("\nOne-time setup for .test names (needs an administrator):")?;
                for step in &status.resolver.setup {
                    out!("{}", step.command)?;
                }
            }
            out!("")?;
            print_trust(&trust)?;
            for issue in &issues {
                out!("! {}: {}", issue.title.english(), issue.detail.english())?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}
