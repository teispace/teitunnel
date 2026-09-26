//! Asking the person in the app, from the process that shares the MCP server (the CLI or
//! `teitunnel mcp`), over the control connection (`oauth.approve`).

use std::{future::Future, path::Path, pin::Pin, sync::Arc};

use teitunnel_control::{
    ClientError, ControlClient, Endpoint,
    protocol::{ClientInfo, OAuthApproval, code},
};

use super::{Approver, ConsentRequest};

/// Asks in the app while it runs; otherwise asks `fallback` (e.g. the terminal), or
/// refuses.
pub struct AskApp {
    endpoint: Endpoint,
    fallback: Option<Arc<dyn Approver>>,
}

impl std::fmt::Debug for AskApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AskApp").finish_non_exhaustive()
    }
}

impl AskApp {
    /// Asks the app of the data folder `data_dir`.
    pub fn new(data_dir: &Path, fallback: Option<Arc<dyn Approver>>) -> Arc<Self> {
        Arc::new(Self {
            endpoint: Endpoint::new(data_dir),
            fallback,
        })
    }
}

fn client_info() -> ClientInfo {
    ClientInfo {
        name: "teitunnel-mcp-auth".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

impl Approver for AskApp {
    fn approve(&self, request: ConsentRequest) -> Pin<Box<dyn Future<Output = bool> + Send>> {
        let endpoint = self.endpoint.clone();
        let fallback = self.fallback.clone();
        Box::pin(async move {
            let question = OAuthApproval {
                host: request.host.clone(),
                client_name: request.client_name.clone(),
                published_by: request.published_by.clone(),
                redirect_host: request.redirect_host.clone(),
                redirect_loopback: request.redirect_loopback,
                code: request.code.clone(),
            };
            let asked = match ControlClient::connect(&endpoint, client_info()).await {
                Ok(client) => client.approve_oauth(&question).await,
                Err(err) => Err(err),
            };
            match asked {
                Ok(approved) => approved,
                // Not answered in time: that's a no.
                Err(ClientError::Rpc(error)) if error.code == code::TIMEOUT => false,
                // The app can't ask (not running, connection off, an older app).
                Err(_) => match fallback {
                    Some(fallback) => fallback.approve(request).await,
                    None => {
                        tracing::warn!(
                            "{} wants to connect to {}, but Teitunnel isn't running to ask",
                            request.client_name,
                            request.host
                        );
                        false
                    }
                },
            }
        })
    }
}
