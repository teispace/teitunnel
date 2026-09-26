//! Local HTTPS domains for agents (M12-07): `https://shop.test` for a dev server, with a
//! trusted certificate, on this computer only (nothing at Cloudflare). They're saved in
//! the app's database; the app serves them while it runs, otherwise this server does
//! until its session ends. Trusting Teitunnel's certificate authority asks for the
//! person's password, so agents leave that to them.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_control::{
    ControlClient, Endpoint,
    protocol::{ClientInfo, LocalDomainsInfo},
};
use teitunnel_core::{
    local_domains::{LocalDomainInput, LocalDomains, NameResolution, complete_name},
    text::UserText as _,
};

use crate::{
    backend::BoxFuture,
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolProvider,
        ToolResult, ToolSpec, arguments,
    },
    tools::{Hints, spec},
};

/// The local domain tools.
pub struct LocalDomainTools {
    local: LocalDomains,
    data_dir: PathBuf,
    /// Serving from this process (the app wasn't running).
    serving: tokio::sync::Mutex<bool>,
}

impl std::fmt::Debug for LocalDomainTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalDomainTools").finish_non_exhaustive()
    }
}

fn client_info() -> ClientInfo {
    ClientInfo {
        name: "teitunnel-mcp".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

impl LocalDomainTools {
    /// Tools over `local` (this data folder's local domains).
    pub fn new(local: LocalDomains, data_dir: &Path) -> Self {
        Self {
            local,
            data_dir: data_dir.to_owned(),
            serving: tokio::sync::Mutex::new(false),
        }
    }

    /// Asks the running app to serve what the database says now; `None` without the app.
    async fn app_reload(&self) -> Option<LocalDomainsInfo> {
        let client = ControlClient::connect(&Endpoint::new(&self.data_dir), client_info())
            .await
            .ok()?;
        tokio::time::timeout(Duration::from_secs(20), client.reload_local_domains())
            .await
            .ok()?
            .ok()
    }

    async fn app_list(&self) -> Option<LocalDomainsInfo> {
        let client = ControlClient::connect(&Endpoint::new(&self.data_dir), client_info())
            .await
            .ok()?;
        client.local_domains().await.ok()
    }

    /// Serves from this process (once), when the app can't.
    async fn serve_here(&self) -> Result<(), ToolError> {
        let mut serving = self.serving.lock().await;
        self.local
            .sync()
            .await
            .map_err(|e| ToolError::new(e.text().english()))?;
        if !*serving {
            tokio::spawn(self.local.clone().run());
            *serving = true;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AddArgs {
    /// The name: `shop.test` (every tool, after a one-time resolver entry), `shop.localhost`
    /// (browsers, no setup) or `phone.local` (also phones on the network). Without a suffix,
    /// `.localhost` is added.
    name: String,
    /// The service: a port (`3000`), `host:port` or a URL.
    target: String,
    /// Subdomains (`*.name`) go to the same service.
    #[serde(default)]
    wildcard: bool,
    /// Plain HTTP only (default: HTTPS with a certificate).
    #[serde(default)]
    no_https: bool,
    /// Record its requests in the inspector.
    #[serde(default)]
    inspect: bool,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed.
    #[serde(default)]
    confirmed: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoveArgs {
    /// Its name, e.g. `shop.test`.
    name: String,
    /// Only when this server can't ask the person itself: the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// A local domain.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Domain {
    name: String,
    /// Where to open it.
    url: String,
    /// The local service.
    origin: Option<String>,
    wildcard: bool,
    https: bool,
    /// Teitunnel answers for it now.
    serving: bool,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListOut {
    domains: Vec<Domain>,
    /// `app` (served while the app runs), `thisServer` (until this session ends) or
    /// `nobody`.
    served_by: String,
    /// Browsers trust Teitunnel's local certificates.
    trusted: bool,
    /// What the person may need to do.
    notes: Vec<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeOut {
    /// `added`, `removed`, `needsApproval` or `declined`.
    outcome: String,
    message: String,
    /// The domain added.
    url: Option<String>,
    notes: Vec<String>,
}

impl LocalDomainTools {
    async fn notes(&self, names: &[String]) -> (bool, Vec<String>) {
        let trust = self.local.trust_status().await;
        let status = self.local.status().await;
        let mut notes = Vec::new();
        if !trust.trusted && status.domains.iter().any(|d| d.https) {
            notes.push("Browsers warn about the certificate until the person trusts Teitunnel's local certificate authority: `teitunnel local-domain trust` (it asks for their password), or Settings ▸ Local Domains.".to_owned());
        }
        for domain in status
            .domains
            .iter()
            .filter(|d| names.is_empty() || names.contains(&d.name))
        {
            match domain.resolution {
                NameResolution::NeedsResolver => notes.push(format!(
                    "{} needs a one-time resolver entry before anything can reach it: `teitunnel local-domain trust` or Local Domains in the app shows it (a .localhost name works with no setup).",
                    domain.name
                )),
                NameResolution::Elsewhere => notes.push(format!(
                    "{} resolves to another computer, so it can't be reached here; pick another name.",
                    domain.name
                )),
                NameResolution::Ok | NameResolution::Unchecked => {}
            }
        }
        (trust.trusted, notes)
    }

    async fn list(&self, args: JsonObject) -> ToolResult {
        let _: ListArgs = arguments(args)?;
        let (domains, served_by) = match self.app_list().await {
            Some(info) => (
                info.domains
                    .into_iter()
                    .map(|d| Domain {
                        name: d.name,
                        url: d.url,
                        origin: d.origin,
                        wildcard: d.wildcard,
                        https: d.https,
                        serving: d.serving,
                    })
                    .collect(),
                "app",
            ),
            None => {
                let status = self.local.status().await;
                let here = *self.serving.lock().await && status.running;
                (
                    status
                        .domains
                        .into_iter()
                        .map(|d| Domain {
                            name: d.name,
                            url: d.url,
                            origin: d.origin,
                            wildcard: d.wildcard,
                            https: d.https,
                            serving: d.serving,
                        })
                        .collect(),
                    if here { "thisServer" } else { "nobody" },
                )
            }
        };
        let (trusted, mut notes) = self.notes(&[]).await;
        if served_by == "nobody" {
            notes.push("Nothing serves them right now: they're served while the Teitunnel app runs (or add one here to serve them until this session ends).".to_owned());
        }
        Ok(ToolOutput::new(&ListOut {
            domains,
            served_by: served_by.to_owned(),
            trusted,
            notes,
        }))
    }

    async fn add(&self, args: JsonObject, ctx: &ToolContext) -> ToolResult {
        let args: AddArgs = arguments(args)?;
        let name = complete_name(&args.name);
        let details = format!(
            "Give {} the local address {}://{name} on this computer only (nothing changes at Cloudflare).",
            args.target,
            if args.no_https { "http" } else { "https" }
        );
        if let Some(out) = self
            .approve(ctx, &format!("Add {name}"), &details, args.confirmed)
            .await
        {
            return Ok(out);
        }
        let row = self
            .local
            .save_new(&LocalDomainInput {
                name,
                target: args.target,
                wildcard: args.wildcard,
                https: !args.no_https,
                inspect: args.inspect,
            })
            .await
            .map_err(|e| ToolError::new(e.text().english()))?;
        let name = row.name.to_string();
        let (url, by) = match self.app_reload().await {
            Some(info) => (
                info.domains
                    .iter()
                    .find(|d| d.name == name)
                    .map(|d| d.url.clone()),
                "The Teitunnel app serves it.",
            ),
            None => {
                self.serve_here().await?;
                (
                    self.local
                        .status()
                        .await
                        .domains
                        .into_iter()
                        .find(|d| d.name == name)
                        .map(|d| d.url),
                    "This server serves it until its session ends (the Teitunnel app serves it whenever it runs).",
                )
            }
        };
        let (_, notes) = self.notes(std::slice::from_ref(&name)).await;
        Ok(ToolOutput::new(&ChangeOut {
            outcome: "added".into(),
            message: format!(
                "{} is ready. {by}",
                url.clone().unwrap_or_else(|| name.clone())
            ),
            url,
            notes,
        }))
    }

    async fn remove(&self, args: JsonObject, ctx: &ToolContext) -> ToolResult {
        let args: RemoveArgs = arguments(args)?;
        let name = complete_name(&args.name);
        if let Some(out) = self
            .approve(
                ctx,
                &format!("Remove {name}"),
                &format!("Remove the local domain {name}."),
                args.confirmed,
            )
            .await
        {
            return Ok(out);
        }
        self.local
            .forget(&name)
            .await
            .map_err(|e| ToolError::new(e.text().english()))?;
        if self.app_reload().await.is_none() && *self.serving.lock().await {
            let _ = self.local.sync().await;
        }
        Ok(ToolOutput::new(&ChangeOut {
            outcome: "removed".into(),
            message: format!("Removed {name}."),
            url: None,
            notes: Vec::new(),
        }))
    }

    /// `Some` answer when the person didn't agree (yet).
    async fn approve(
        &self,
        ctx: &ToolContext,
        title: &str,
        details: &str,
        confirmed: bool,
    ) -> Option<ToolOutput> {
        let out = |outcome: &str, message: String| {
            Some(ToolOutput::new(&ChangeOut {
                outcome: outcome.into(),
                message,
                url: None,
                notes: Vec::new(),
            }))
        };
        match ctx
            .approve(&ApprovalRequest {
                title: title.to_owned(),
                details: details.to_owned(),
                confirmed,
            })
            .await
        {
            Approval::Granted { .. } => None,
            Approval::NeedsConfirmation => out(
                "needsApproval",
                format!(
                    "Nothing changed. Show the person this and call again with \"confirmed\": true if they agree:\n{details}"
                ),
            ),
            Approval::Declined(why) => out("declined", format!("{why} Nothing changed.")),
        }
    }
}

impl ToolProvider for LocalDomainTools {
    fn tools(&self) -> Vec<ToolSpec> {
        let change = Hints {
            read_only: false,
            destructive: false,
            idempotent: false,
            open_world: false,
        };
        vec![
            spec::<ListArgs, ListOut>(
                "list_local_domains",
                "List local domains",
                "Local HTTPS addresses on this computer (`https://shop.test` → a local service), whether they're served now and by whom, and whether browsers trust their certificates.",
                ToolClass::Read,
                Hints::READ_LOCAL,
                crate::tools::DEFAULT_TIMEOUT,
            ),
            spec::<AddArgs, ChangeOut>(
                "add_local_domain",
                "Give a dev server a local HTTPS address",
                "Give a local service an HTTPS address on this computer, like https://shop.test, with a certificate from Teitunnel's local authority (nothing changes at Cloudflare; only this computer, and with `.local` names phones on the network, can open it). Useful for apps that need HTTPS, secure cookies, OAuth callbacks or several services on real-looking hostnames. Served by the Teitunnel app while it runs, otherwise by this server until the session ends. If browsers don't trust the certificate yet, the answer says what the person runs.\n\
                 \n\
                 Example: {\"name\": \"shop.test\", \"target\": \"3000\"}",
                ToolClass::Change,
                change,
                Duration::from_secs(60),
            ),
            spec::<RemoveArgs, ChangeOut>(
                "remove_local_domain",
                "Remove a local domain",
                "Remove a local HTTPS address from this computer.\n\nExample: {\"name\": \"shop.test\"}",
                ToolClass::Destructive,
                Hints {
                    destructive: true,
                    ..change
                },
                crate::tools::DEFAULT_TIMEOUT,
            ),
        ]
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: JsonObject,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            match name {
                "list_local_domains" => self.list(arguments).await,
                "add_local_domain" => self.add(arguments, ctx).await,
                "remove_local_domain" => self.remove(arguments, ctx).await,
                other => Err(ToolError::new(format!("Unknown tool {other}."))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use teitunnel_core::{inspect::Inspector, local_domains::LocalDomainsConfig, store::Store};

    use super::*;
    use crate::{
        config::Mode,
        tools::tests::{actor, settings},
    };

    fn tools(dir: &Path) -> LocalDomainTools {
        let store = Store::open_in_memory().unwrap();
        let local = LocalDomains::new(
            store.clone(),
            Inspector::new(Some(store), None, "test"),
            LocalDomainsConfig::detect(dir, None),
        );
        LocalDomainTools::new(local, dir)
    }

    fn args(value: &serde_json::Value) -> JsonObject {
        value.as_object().cloned().unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn asks_before_adding_and_says_when_nothing_serves() {
        let dir = tempfile::tempdir().unwrap();
        let tools = tools(dir.path());
        let full = ToolContext::detached(settings(Mode::Full), actor());
        let listed = tools
            .call("list_local_domains", args(&json!({})), &full)
            .await
            .unwrap()
            .structured;
        assert_eq!(listed["servedBy"], "nobody");
        assert!(listed["domains"].as_array().unwrap().is_empty());

        let ask = ToolContext::detached(settings(Mode::Ask), actor());
        let out = tools
            .call(
                "add_local_domain",
                args(&json!({"name": "shop", "target": "3000"})),
                &ask,
            )
            .await
            .unwrap()
            .structured;
        assert_eq!(out["outcome"], "needsApproval");
        assert!(
            out["message"]
                .as_str()
                .unwrap()
                .contains("https://shop.localhost"),
            "a bare name becomes .localhost: {out}"
        );
        let listed = tools
            .call("list_local_domains", args(&json!({})), &full)
            .await
            .unwrap()
            .structured;
        assert!(
            listed["domains"].as_array().unwrap().is_empty(),
            "nothing saved"
        );

        let missing = tools
            .call(
                "remove_local_domain",
                args(&json!({"name": "nope.test"})),
                &full,
            )
            .await
            .unwrap_err();
        assert!(missing.to_string().contains("nope.test"), "{missing}");
    }
}
