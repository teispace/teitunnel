//! The offline page and webhook inboxes for the app, the CLI and agents: what a route
//! has now (from the local index, no network) and changes planned and applied through
//! the engine like every other Cloudflare change (`engine::front`).
//!
//! A verifying inbox uses the webhook signing secret the inspector already keeps in the
//! keychain for the hostname and provider (`inspect::secrets`); it's read here and sent
//! to Cloudflare as a Worker secret only when verification is turned on or changes, and
//! never returned to the UI or an agent.

use serde::{Deserialize, Serialize};

use crate::{
    domain::Hostname,
    engine::{
        Approval, CloudApi, Connectors, Context, Engine, EngineError, InputError, Intent, Outcome,
        PlanView, Progress,
        front::{FrontConfig, FrontKind, InboxSettings, InboxVerify, OfflinePage},
    },
    secrets::Secrets,
    text::UserText,
};

/// A change to a route's Workers, as the app, the CLI and agents ask for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum FrontChange {
    /// Show this page while the computer is off (`None` removes it).
    Offline {
        /// The hostname.
        hostname: String,
        /// The page.
        page: Option<OfflinePage>,
    },
    /// Keep webhooks to `path` while the computer is off (`None` removes the inbox).
    Inbox {
        /// The hostname.
        hostname: String,
        /// The path, e.g. `/webhooks/`.
        path: String,
        /// Its settings.
        inbox: Option<InboxSettings>,
    },
}

impl FrontChange {
    /// The hostname it's about.
    pub fn hostname(&self) -> &str {
        match self {
            Self::Offline { hostname, .. } | Self::Inbox { hostname, .. } => hostname,
        }
    }
}

/// One of Teitunnel's Workers in front of a route, as the app lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct FrontView {
    /// Account id.
    pub account_id: String,
    /// The hostname.
    pub hostname: String,
    /// Which Worker.
    pub kind: FrontKind,
    /// The inbox path (empty for the offline page).
    pub path: String,
    /// The page, for the offline page.
    pub page: Option<OfflinePage>,
    /// The settings, for an inbox.
    pub inbox: Option<InboxSettings>,
    /// Its Worker.
    pub script: String,
    /// Whether its route is in place.
    pub routed: bool,
}

fn provider_of(verify: InboxVerify) -> lens::webhook::Provider {
    match verify {
        InboxVerify::Github => lens::webhook::Provider::GitHub,
        InboxVerify::Stripe => lens::webhook::Provider::Stripe,
        InboxVerify::Standard => lens::webhook::Provider::StandardWebhooks,
    }
}

fn hostname(input: &str) -> Result<Hostname, InputError> {
    Hostname::parse(input).map_err(|e| InputError {
        field: "hostname",
        message: e.text(),
    })
}

/// Teitunnel's Workers in front of routes in `account` (or every account).
///
/// # Errors
/// Database errors.
pub async fn list(
    engine: &Engine,
    account: Option<&str>,
) -> Result<Vec<FrontView>, crate::store::StoreError> {
    Ok(engine
        .local()
        .fronts(account, None)
        .await?
        .into_iter()
        .map(|(account_id, row)| {
            let (page, inbox) = match &row.config {
                FrontConfig::Offline { page } => (Some(page.clone()), None),
                FrontConfig::Inbox { settings, .. } => (None, Some(settings.clone())),
            };
            FrontView {
                account_id,
                kind: row.config.kind(),
                path: row.config.path().to_owned(),
                hostname: row.hostname,
                page,
                inbox,
                script: row.script,
                routed: row.route_id.is_some(),
            }
        })
        .collect())
}

/// The intent for `change`: a verifying inbox gets the signing secret from the
/// keychain when verification is new or changes (otherwise the Worker keeps its own).
///
/// # Errors
/// Invalid hostname; a verifying inbox without a saved secret (planning refuses it).
pub async fn intent(
    engine: &Engine,
    secrets: Option<&Secrets>,
    account: &str,
    change: &FrontChange,
) -> Result<Intent, EngineError> {
    let host = hostname(change.hostname())?;
    Ok(match change {
        FrontChange::Offline { page, .. } => Intent::SetOfflinePage {
            hostname: host,
            page: page.clone(),
        },
        FrontChange::Inbox { path, inbox, .. } => {
            let current = engine
                .local()
                .fronts(Some(account), Some(host.as_str()))
                .await
                .map_err(crate::engine::ObserveError::from)?
                .into_iter()
                .find_map(|(_, row)| match row.config {
                    FrontConfig::Inbox {
                        path: p, settings, ..
                    } if p == *path => Some(settings),
                    _ => None,
                });
            let wanted = inbox.as_ref().and_then(|i| i.verify);
            let secret = match (wanted, secrets) {
                (Some(verify), Some(secrets))
                    if current.as_ref().and_then(|c| c.verify) != Some(verify) =>
                {
                    crate::inspect::secrets::webhook_secret_text(
                        secrets,
                        &crate::inspect::secrets::host_scope(host.as_str()),
                        provider_of(verify),
                    )
                    .await
                    .ok()
                    .flatten()
                }
                _ => None,
            };
            Intent::SetInbox {
                hostname: host,
                path: path.clone(),
                inbox: inbox.clone(),
                secret,
            }
        }
    })
}

/// Plans a change for review. Nothing is changed.
///
/// # Errors
/// Invalid input, observation or planning errors.
pub async fn preview<C: CloudApi>(
    engine: &Engine,
    api: &C,
    secrets: Option<&Secrets>,
    ctx: Context<'_>,
    change: &FrontChange,
) -> Result<PlanView, EngineError> {
    let intent = intent(engine, secrets, ctx.account, change).await?;
    Ok(engine.preview(api, ctx, &intent).await?.view(ctx.account))
}

/// Applies a reviewed change.
///
/// # Errors
/// Errors before anything changed; failures while applying are in the [`Outcome`].
#[allow(clippy::too_many_arguments)]
pub async fn apply<C, K, P>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    secrets: Option<&Secrets>,
    ctx: Context<'_>,
    change: &FrontChange,
    approval: Approval<'_>,
    progress: P,
) -> Result<Outcome, EngineError>
where
    C: CloudApi,
    K: Connectors,
    P: FnMut(Progress) + Send,
{
    let intent = intent(engine, secrets, ctx.account, change).await?;
    engine
        .apply(api, connectors, ctx, &intent, approval, progress)
        .await
}

/// Saves the signing secret a verifying inbox on `hostname` checks `verify`'s webhooks
/// with (in the keychain, shared with the hostname's inspector).
///
/// # Errors
/// The keychain refused.
pub async fn set_inbox_secret(
    secrets: &Secrets,
    hostname: &str,
    verify: InboxVerify,
    secret: crate::Secret<String>,
) -> Result<(), crate::secrets::SecretError> {
    let scope = crate::inspect::secrets::host_scope(hostname);
    crate::inspect::secrets::set_webhook_secret(secrets, &scope, provider_of(verify), secret).await
}

/// Which senders have a signing secret saved for `hostname` (never the secrets).
///
/// # Errors
/// The keychain refused.
pub async fn inbox_secrets(
    secrets: &Secrets,
    hostname: &str,
) -> Result<Vec<InboxVerify>, crate::secrets::SecretError> {
    let scope = crate::inspect::secrets::host_scope(hostname);
    let saved = crate::inspect::secrets::webhook_providers(secrets, &scope).await?;
    Ok([
        InboxVerify::Github,
        InboxVerify::Stripe,
        InboxVerify::Standard,
    ]
    .into_iter()
    .filter(|v| saved.contains(&provider_of(*v)))
    .collect())
}

/// The change that puts a route's Worker back as it is now (for Undo in the app).
///
/// # Errors
/// Database errors.
pub async fn undo_of(
    engine: &Engine,
    account: &str,
    change: &FrontChange,
) -> Result<FrontChange, crate::store::StoreError> {
    let rows = engine
        .local()
        .fronts(Some(account), Some(change.hostname()))
        .await?;
    Ok(match change {
        FrontChange::Offline { hostname, .. } => FrontChange::Offline {
            hostname: hostname.clone(),
            page: rows.into_iter().find_map(|(_, r)| match r.config {
                FrontConfig::Offline { page } => Some(page),
                FrontConfig::Inbox { .. } => None,
            }),
        },
        FrontChange::Inbox { hostname, path, .. } => FrontChange::Inbox {
            hostname: hostname.clone(),
            path: path.clone(),
            inbox: rows.into_iter().find_map(|(_, r)| match r.config {
                FrontConfig::Inbox { path: p, settings } if p == *path => Some(settings),
                _ => None,
            }),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Secret, engine::Local, secrets::MemoryStore, store::Store};

    #[tokio::test]
    async fn a_verifying_inbox_takes_the_saved_secret_once() {
        let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
        let secrets: Secrets = std::sync::Arc::new(MemoryStore::default());
        assert!(
            inbox_secrets(&secrets, "app.xyz.com")
                .await
                .unwrap()
                .is_empty()
        );
        // Saved the way the app and `teitunnel inbox secret` save it (any case).
        set_inbox_secret(
            &secrets,
            "App.xyz.com",
            InboxVerify::Github,
            Secret::new("gh".into()),
        )
        .await
        .unwrap();
        assert_eq!(
            inbox_secrets(&secrets, "app.xyz.com").await.unwrap(),
            [InboxVerify::Github]
        );
        let change = FrontChange::Inbox {
            hostname: "App.XYZ.com".into(),
            path: "/hooks/".into(),
            inbox: Some(InboxSettings {
                verify: Some(InboxVerify::Github),
                ..InboxSettings::default()
            }),
        };
        let Intent::SetInbox { secret, .. } = intent(&engine, Some(&secrets), "acc", &change)
            .await
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(secret.unwrap().expose(), "gh");
        // Once deployed with it, the Worker keeps its secret.
        engine
            .local()
            .save_front(
                "acc",
                "app.xyz.com",
                "z",
                "tt-inbox-1",
                &FrontConfig::Inbox {
                    path: "/hooks/".into(),
                    settings: InboxSettings {
                        verify: Some(InboxVerify::Github),
                        ..InboxSettings::default()
                    },
                },
            )
            .await
            .unwrap();
        let Intent::SetInbox { secret, .. } = intent(&engine, Some(&secrets), "acc", &change)
            .await
            .unwrap()
        else {
            panic!()
        };
        assert!(secret.is_none());
        let listed = list(&engine, Some("acc")).await.unwrap();
        assert_eq!(listed[0].kind, FrontKind::Inbox);
        assert!(!listed[0].routed);
        // Undo turns it back into what's there.
        let undo = undo_of(
            &engine,
            "acc",
            &FrontChange::Inbox {
                hostname: "app.xyz.com".into(),
                path: "/hooks/".into(),
                inbox: None,
            },
        )
        .await
        .unwrap();
        assert!(matches!(undo, FrontChange::Inbox { inbox: Some(_), .. }));
    }
}
