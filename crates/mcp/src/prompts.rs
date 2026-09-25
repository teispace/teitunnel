//! Prompts: ready-made tasks a person can pick in their client ("put my dev server
//! online"), each a short recipe of tool calls with the person's details filled in.

use rmcp::{
    ErrorData as McpError,
    model::{GetPromptResult, JsonObject, Prompt, PromptArgument, PromptMessage, Role},
};

use crate::config::Mode;

/// The prompts.
pub(crate) fn list() -> Vec<Prompt> {
    let arg = |name: &str, description: &str, required: bool| {
        PromptArgument::new(name)
            .with_description(description)
            .with_required(required)
    };
    vec![
        Prompt::new(
            "put_online",
            Some("Put my dev server online: a public link, or a hostname on my domain with a login."),
            Some(vec![
                arg("port", "The port or URL of the service (found automatically if omitted).", false),
                arg("hostname", "A hostname on your domain, e.g. demo.teispace.com (omit for a random trycloudflare.com link).", false),
                arg("allow", "Who may open it: emails or @domains, comma-separated (needs a hostname).", false),
                arg("permanent", "\"yes\" for a permanent route instead of a share that ends with this session.", false),
            ]),
        )
        .with_title("Put my dev server online"),
        Prompt::new(
            "debug_webhook",
            Some("Debug a failing webhook: receive it, inspect it, fix the handler, replay until it passes."),
            Some(vec![
                arg("port", "The port of the app receiving the webhook.", false),
                arg("path", "The webhook path, e.g. /webhooks/stripe.", false),
                arg("provider", "Who sends it, e.g. Stripe, GitHub, Slack.", false),
            ]),
        )
        .with_title("Debug a failing webhook"),
        Prompt::new(
            "route_down",
            Some("Find out why a route doesn't work, and fix it."),
            Some(vec![arg("hostname", "The hostname that doesn't work, e.g. app.teispace.com.", true)]),
        )
        .with_title("Why is my route down?"),
        Prompt::new(
            "move_to_server",
            Some("Move this machine's routes to a server (Docker Compose, config.yml or Terraform)."),
            Some(vec![
                arg("format", "dockerCompose (default), configYaml or terraform.", false),
                arg("tunnel", "Which of this machine's tunnels (default: the default one).", false),
            ]),
        )
        .with_title("Move my routes to a server"),
    ]
}

fn value(arguments: Option<&JsonObject>, name: &str) -> Option<String> {
    arguments?
        .get(name)?
        .as_str()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToOwned::to_owned)
}

fn approval_note(mode: Mode) -> &'static str {
    match mode {
        Mode::ReadOnly => {
            "This Teitunnel server is read-only: explain what you'd do and give the person the plan, but don't try to change anything."
        }
        Mode::Ask => {
            "Every change needs the person's approval: if a tool answers needsApproval, show them what it says and call it again with confirmed: true only after they agree."
        }
        Mode::Full => {
            "Changes apply without asking, so explain each one to the person before making it."
        }
    }
}

/// A prompt, filled in.
pub(crate) fn get(
    name: &str,
    arguments: Option<&JsonObject>,
    mode: Mode,
) -> Result<GetPromptResult, McpError> {
    let text = match name {
        "put_online" => {
            let port = value(arguments, "port");
            let hostname = value(arguments, "hostname");
            let allow = value(arguments, "allow");
            let permanent =
                value(arguments, "permanent").is_some_and(|p| p.eq_ignore_ascii_case("yes"));
            let find = port.as_deref().map_or_else(
                || "1. Call list_local_services and pick the dev server (ask the person if several look likely).".to_owned(),
                |p| format!("1. The service is {p}."),
            );
            let share = match (&hostname, permanent) {
                (Some(host), true) => format!(
                    "2. Call plan_change with an addRoute change for {host}{}, show the person the plan, then apply_plan.\n3. verify_route {host} and give the person the URL.",
                    allow.as_deref().map(|a| format!(" with allow = [{a}]")).unwrap_or_default()
                ),
                (Some(host), false) => format!(
                    "2. Call share_port with the service and hostname {host}{}.\n3. Give the person the URL; mention it ends when this session does (or use plan_change addRoute for a permanent route).",
                    allow.as_deref().map(|a| format!(" and allow = [{a}]")).unwrap_or_default()
                ),
                (None, _) => "2. Call share_port with the service (a public trycloudflare.com link).\n3. Give the person the URL and say anyone with it can open it.".to_owned(),
            };
            format!(
                "Put my dev server online with Teitunnel.\n\n{find}\n{share}\nIf the page says the host isn't allowed (Vite, webpack, Next.js dev servers check the Host header), set the route's httpHostHeader to localhost (plan_change updateRoute options) or add the hostname to the dev server's allowed hosts.\n\n{}",
                approval_note(mode)
            )
        }
        "debug_webhook" => {
            let port = value(arguments, "port")
                .unwrap_or_else(|| "the app's port (list_local_services finds it)".into());
            let path = value(arguments, "path").unwrap_or_else(|| "the webhook path".into());
            let provider = value(arguments, "provider").unwrap_or_else(|| "the sender".into());
            format!(
                "Help me debug a failing webhook from {provider}.\n\n\
                 1. Check list_shares for an existing share of {port}; otherwise share it with share_port (a hostname on my domain keeps the URL stable).\n\
                 2. Tell me the URL to configure at {provider}, ending in {path}, and ask me to trigger a delivery (or resend one).\n\
                 3. Call wait_for_request with pathContains {path} and method POST (timeoutSeconds 300). Keep its sinceMs to wait again without missing a delivery.\n\
                 4. Read the request and my app's response in the result (or traffic_get). Find why the handler fails (status, error body, missing or wrong signature, content type), then fix the code.\n\
                 5. traffic_replay the same request until it answers 2xx. Signature headers stay masked; a replay reuses the original ones, so a handler that checks timestamps may reject old deliveries: say so if that happens.\n\n{}",
                approval_note(mode)
            )
        }
        "route_down" => {
            let hostname = value(arguments, "hostname")
                .ok_or_else(|| McpError::invalid_params("route_down needs a hostname.", None))?;
            format!(
                "https://{hostname} doesn't work. Find out why and fix it.\n\n\
                 1. verify_route {hostname}: it says at which stage it breaks (DNS, edge, tunnel, origin).\n\
                 2. doctor: look for issues about {hostname} or its tunnel.\n\
                 3. If the connector is the problem (noConnector, 1033), check connector_status; if the service is (originUnreachable, 502), check list_local_services and logs_tail with hostname {hostname}.\n\
                 4. Fix it with fix_issue, or a plan (plan_change), or tell me what to do on my side (start the dev server, open Teitunnel).\n\
                 5. verify_route again and tell me the result.\n\n{}",
                approval_note(mode)
            )
        }
        "move_to_server" => {
            let format = value(arguments, "format").unwrap_or_else(|| "dockerCompose".into());
            let tunnel = value(arguments, "tunnel")
                .map(|t| format!(" for tunnel {t}"))
                .unwrap_or_default();
            format!(
                "Move this machine's routes to a server.\n\n\
                 1. list_routes to show me what will move.\n\
                 2. export_config with format {format}{tunnel}.\n\
                 3. Explain how to run it on the server: for Docker Compose, save it as compose.yaml and set TUNNEL_TOKEN (I get the token from the Cloudflare dashboard or `cloudflared tunnel token`; never ask me to paste it to you); for config.yml, install cloudflared and run `cloudflared tunnel run`; for Terraform, `terraform init && terraform plan` should show no changes.\n\
                 4. Once the server runs the tunnel, both machines serve it; I can stop it here in Teitunnel.\n\n{}",
                approval_note(mode)
            )
        }
        other => {
            return Err(McpError::invalid_params(
                format!("No prompt {other}."),
                None,
            ));
        }
    };
    let description = list()
        .into_iter()
        .find(|p| p.name == name)
        .and_then(|p| p.description)
        .unwrap_or_default();
    Ok(
        GetPromptResult::new(vec![PromptMessage::new_text(Role::User, text)])
            .with_description(description),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_in_arguments() {
        let mut args = JsonObject::new();
        args.insert("hostname".into(), "demo.xyz.com".into());
        args.insert("allow".into(), "@xyz.com".into());
        let prompt = get("put_online", Some(&args), Mode::Ask).unwrap();
        let text = serde_json::to_string(&prompt.messages).unwrap();
        assert!(text.contains("share_port"));
        assert!(text.contains("demo.xyz.com"));
        assert!(text.contains("needsApproval"));
        assert!(
            get("route_down", None, Mode::Ask).is_err(),
            "hostname is required"
        );
        assert!(get("nope", None, Mode::Ask).is_err());
        for prompt in list() {
            let mut args = JsonObject::new();
            args.insert("hostname".into(), "a.xyz.com".into());
            assert!(
                get(&prompt.name, Some(&args), Mode::ReadOnly).is_ok(),
                "{}",
                prompt.name
            );
        }
    }
}
