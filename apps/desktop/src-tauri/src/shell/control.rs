//! The control connection and `teitunnel://` links, hosted by the app: starts and stops
//! the server (`teitunnel-control`), and gives the core's host what only the shell
//! can do (native confirmation dialogs, windows, telling the webview about changes).

use std::{
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};
use tauri_specta::Event as _;
use teitunnel_control::{
    BoxFuture, Decision, Endpoint, Server,
    deeplink::{DeepLink, Handled, LinkHandler},
    protocol::{Event, View},
};
use teitunnel_core::control::{Changed, CoreHost, Prompt, Ui, integrations};

use crate::{
    ipc::{EntityChanged, EntityKind, OpenView, ViewTarget},
    shell,
    state::AppState,
};

/// The running server and the link handler.
#[derive(Debug)]
pub(crate) struct Control {
    /// Answers the control connection and links.
    pub host: Arc<CoreHost>,
    endpoint: Endpoint,
    stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    links: LinkHandler,
}

impl Control {
    pub(crate) fn new(host: Arc<CoreHost>, data_dir: &Path) -> Self {
        Self {
            host,
            endpoint: Endpoint::new(data_dir),
            stop: Mutex::default(),
            links: LinkHandler::default(),
        }
    }

    /// Starts listening, unless it already does.
    pub(crate) fn start(&self) {
        let mut stop = self.stop.lock().unwrap_or_else(PoisonError::into_inner);
        if stop.is_some() {
            return;
        }
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        *stop = Some(tx);
        let endpoint = self.endpoint.clone();
        let host: Arc<dyn teitunnel_control::Host> = self.host.clone();
        tauri::async_runtime::spawn(async move {
            let token = match endpoint.ensure_token() {
                Ok(token) => token,
                Err(err) => {
                    tracing::warn!(%err, "couldn't create the control token");
                    return;
                }
            };
            match endpoint.listen().await {
                Ok(listener) => {
                    tracing::info!("control connection listening");
                    Server::new(host, token)
                        .run(listener, async {
                            let _ = rx.await;
                        })
                        .await;
                    tracing::info!("control connection closed");
                }
                Err(err) => tracing::warn!(%err, "couldn't open the control connection"),
            }
        });
    }

    /// Stops listening and closes every connection.
    pub(crate) fn stop(&self) {
        if let Some(stop) = self
            .stop
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = stop.send(());
        }
    }
}

/// The native side of the host.
struct TauriUi<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> Ui for TauriUi<R> {
    fn confirm(&self, prompt: Prompt) -> BoxFuture<'_, Decision> {
        Box::pin(async move { confirm(&self.app, prompt).await })
    }

    fn open(&self, view: View) {
        open(&self.app, view);
    }

    fn changed(&self, change: Changed) {
        let (kind, id) = match change {
            Changed::Shares => (EntityKind::QuickShares, None),
            Changed::Routes { account_id } => {
                shell_refresh(&self.app);
                (EntityKind::Routes, Some(account_id))
            }
        };
        let _ = EntityChanged { kind, id }.emit(&self.app);
    }
}

fn shell_refresh<R: Runtime>(app: &AppHandle<R>) {
    crate::bootstrap::refresh_tray_routes(app);
}

/// The shell's [`Ui`] for the core's host.
pub(crate) fn ui<R: Runtime>(app: &AppHandle<R>) -> Arc<dyn Ui> {
    Arc::new(TauriUi { app: app.clone() })
}

/// Asks with a native dialog, attached to the main window (brought to the front so
/// the question is seen).
async fn confirm<R: Runtime>(app: &AppHandle<R>, prompt: Prompt) -> Decision {
    shell::windows::focus_main(app);
    let allow = prompt.allow.to_string();
    let always = prompt.always.as_ref().map(ToString::to_string);
    let deny = prompt.deny.to_string();
    let buttons = match &always {
        Some(always) => {
            MessageDialogButtons::YesNoCancelCustom(allow.clone(), always.clone(), deny)
        }
        None => MessageDialogButtons::OkCancelCustom(allow.clone(), deny),
    };
    let mut dialog = app
        .dialog()
        .message(prompt.message.to_string())
        .title(prompt.title.to_string())
        .kind(MessageDialogKind::Warning)
        .buttons(buttons);
    if let Some(window) = app.get_webview_window("main") {
        dialog = dialog.parent(&window);
    }
    let (tx, rx) = tokio::sync::oneshot::channel();
    dialog.show_with_result(move |result| {
        let _ = tx.send(result);
    });
    let Ok(result) = rx.await else {
        return Decision::Deny;
    };
    match result {
        MessageDialogResult::Yes | MessageDialogResult::Ok => Decision::Once,
        MessageDialogResult::No if always.is_some() => Decision::Always,
        MessageDialogResult::Custom(label) if label == allow => Decision::Once,
        MessageDialogResult::Custom(label) if Some(&label) == always.as_ref() => Decision::Always,
        _ => Decision::Deny,
    }
}

/// Shows the main window at `view` (the webview navigates on `OpenView`).
fn open<R: Runtime>(app: &AppHandle<R>, view: View) {
    shell::windows::focus_main(app);
    let target = match view {
        View::Overview => ViewTarget::Overview,
        View::Route { hostname } => ViewTarget::Route { hostname },
        View::Share { id } => ViewTarget::Share { id },
        View::Inspector { share } => ViewTarget::Inspector { share },
        View::Doctor => ViewTarget::Doctor,
    };
    let _ = OpenView { target }.emit(app);
}

/// Sends what the webview is told about to control subscribers too.
pub(crate) fn forward_changes<R: Runtime>(app: &AppHandle<R>, host: Arc<CoreHost>) {
    EntityChanged::listen_any(app, move |event| {
        let event = match event.payload.kind {
            EntityKind::QuickShares => Event::SharesChanged {
                id: event.payload.id,
            },
            EntityKind::Routes => Event::RoutesChanged {
                account_id: event.payload.id,
            },
            _ => return,
        };
        host.publish(event);
    });
}

/// Handles `teitunnel://` links: from the system while the app runs, and the one that
/// launched it.
pub(crate) fn listen_for_links<R: Runtime>(app: &AppHandle<R>) {
    use tauri_plugin_deep_link::DeepLinkExt;
    let handle = app.clone();
    app.deep_link().on_open_url(move |event| {
        for url in event.urls() {
            handle_link(&handle, url.as_str());
        }
    });
    if let Ok(Some(urls)) = app.deep_link().get_current() {
        for url in urls {
            handle_link(app, url.as_str());
        }
    }
}

fn handle_link<R: Runtime>(app: &AppHandle<R>, link: &str) {
    let link = match DeepLink::parse(link) {
        Ok(link) => link,
        Err(err) => {
            tracing::warn!(%err, "ignored a teitunnel:// link");
            return;
        }
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let enabled = integrations::load(&state.store)
            .await
            .is_ok_and(|s| s.deep_links_enabled);
        if !enabled {
            tracing::info!("teitunnel:// links are off; ignored one");
            return;
        }
        let control = &state.control;
        match control.links.handle(control.host.as_ref(), link).await {
            Ok(Handled::Busy) => tracing::info!("a link is waiting for an answer; ignored another"),
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(%err, "a teitunnel:// link failed");
                crate::bootstrap::notify_link_failed(&app, &err.message);
            }
        }
    });
}
