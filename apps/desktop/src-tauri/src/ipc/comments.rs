//! Comments on shares, routes and Snapshots (the logic is `teitunnel_core::comments`).
//! Live shares' comments are kept on this computer; Snapshot comments are read from and
//! answered in the account's D1 database with the account's token.

use tauri::{AppHandle, State};
use tauri_specta::Event;
use teitunnel_core::{
    comments::{Author, Subject, SubjectKind, SubjectView, Thread},
    inspect::{TapScope, TapView, lens::TapId},
    text::msg,
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn comments(state: &AppState) -> Result<&teitunnel_core::comments::Comments, AppError> {
    state
        .inspector
        .comments()
        .ok_or_else(|| AppError::internal(msg::raw("comments need the database")))
}

/// The owner's name on replies: the person part of `person@machine`.
fn owner() -> Author {
    let label = teitunnel_core::engine::ownership::owner_label();
    Author::owner(label.split('@').next().unwrap_or(&label))
}

async fn client_for(
    state: &AppState,
    subject: &Subject,
) -> Result<Option<teitunnel_core::accounts::CloudClient>, AppError> {
    match (&subject.kind, &subject.account_id) {
        (SubjectKind::Snapshot, Some(account)) => Ok(Some(state.accounts.client(account).await?)),
        _ => Ok(None),
    }
}

async fn subject(state: &AppState, key: &str) -> Result<Subject, AppError> {
    comments(state)?
        .subject(key)
        .await?
        .ok_or_else(|| AppError::from(teitunnel_core::comments::CommentsError::NotFound))
}

fn changed(app: &AppHandle, key: &str) {
    let _ = EntityChanged {
        kind: EntityKind::Comments,
        id: Some(key.to_owned()),
    }
    .emit(app);
}

/// Every share, route and Snapshot with comments, newest activity first.
#[tauri::command]
#[specta::specta]
pub async fn comments_subjects(state: State<'_, AppState>) -> Result<Vec<SubjectView>, AppError> {
    Ok(comments(&state)?.subjects().await?)
}

/// A subject's threads (with verified addresses, for the owner); marks them read.
#[tauri::command]
#[specta::specta]
pub async fn comments_threads(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<Vec<Thread>, AppError> {
    let subject = subject(&state, &key).await?;
    let api = client_for(&state, &subject).await?;
    let threads = comments(&state)?.threads(api.as_ref(), &key).await?;
    let before = comments(&state)?
        .subjects()
        .await?
        .into_iter()
        .find(|s| s.subject.key == key)
        .is_some_and(|s| s.unread > 0);
    comments(&state)?.mark_seen(&key).await?;
    if before {
        changed(&app, &key);
    }
    Ok(threads)
}

/// The owner's reply.
#[tauri::command]
#[specta::specta]
pub async fn comments_reply(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    thread: String,
    body: String,
) -> Result<Thread, AppError> {
    let subject = subject(&state, &key).await?;
    let api = client_for(&state, &subject).await?;
    let reply = comments(&state)?
        .reply(api.as_ref(), &key, &thread, &body, &owner())
        .await?;
    changed(&app, &key);
    Ok(reply)
}

/// Resolves or reopens a thread.
#[tauri::command]
#[specta::specta]
pub async fn comments_resolve(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
    thread: String,
    resolved: bool,
) -> Result<Thread, AppError> {
    let subject = subject(&state, &key).await?;
    let api = client_for(&state, &subject).await?;
    let thread = comments(&state)?
        .resolve(api.as_ref(), &key, &thread, resolved, &owner().name)
        .await?;
    changed(&app, &key);
    Ok(thread)
}

/// Removes a subject from the list, with the comments kept on this computer for it
/// (a Snapshot's comments on Cloudflare stay until the Snapshot is deleted).
#[tauri::command]
#[specta::specta]
pub async fn comments_forget(
    app: AppHandle,
    state: State<'_, AppState>,
    key: String,
) -> Result<(), AppError> {
    comments(&state)?.forget(&key).await?;
    changed(&app, &key);
    Ok(())
}

/// Turns comments on a Quick Share or an inspected route on or off. A route with
/// Teitunnel's login trusts the address Cloudflare Access vouches for.
#[tauri::command]
#[specta::specta]
pub async fn comments_set_tap(
    app: AppHandle,
    state: State<'_, AppState>,
    tap: TapId,
    on: bool,
) -> Result<TapView, AppError> {
    let view = state
        .inspector
        .view(&tap)
        .map_err(teitunnel_core::Error::from)?;
    let trust = match &view.scope {
        TapScope::Route {
            account_id,
            hostname,
            ..
        } => state
            .engine
            .local()
            .owned_access_apps(account_id)
            .await
            .unwrap_or_default()
            .iter()
            .any(|(_, domain)| domain.split('/').next() == Some(hostname.as_str())),
        TapScope::QuickShare { .. } => false,
    };
    let view = state
        .inspector
        .set_comments(&tap, on, trust)
        .await
        .map_err(teitunnel_core::Error::from)?;
    let _ = EntityChanged {
        kind: EntityKind::Inspector,
        id: None,
    }
    .emit(&app);
    let _ = EntityChanged {
        kind: EntityKind::Comments,
        id: None,
    }
    .emit(&app);
    Ok(view)
}
