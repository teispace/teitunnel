//! Edge protection and service tokens for the app, the CLI and agents: what a hostname
//! has now (with its zone's quotas), and changes planned and applied through the engine
//! like every other Cloudflare change (ownership, plan → apply, undo).
//!
//! A new or rotated service token's secret exists outside Cloudflare only in the
//! [`IssuedSecrets`] vault, in memory, for a few minutes: the app copies it to the
//! clipboard from Rust (it never crosses IPC), the CLI prints it once.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::{
    Secret,
    domain::Hostname,
    engine::{
        Approval, CloudApi, Connectors, Context, Engine, EngineError, InputError, Intent,
        ObserveError, Outcome, PlanView, Progress,
        edge::{EdgeProtection, IssuedToken, QuotaKind, ZonePlan},
    },
    text::UserText,
};

/// A change to a hostname's protection, as the app, the CLI and agents ask for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProtectionChange {
    /// Enforce these settings at the edge (the default turns everything off).
    Protect {
        /// The hostname.
        hostname: String,
        /// What to enforce.
        protection: EdgeProtection,
    },
    /// Create a service token for machines.
    CreateToken {
        /// The hostname.
        hostname: String,
        /// What it's for, e.g. `CI`.
        label: String,
    },
    /// Revoke (delete) a service token.
    RevokeToken {
        /// The hostname.
        hostname: String,
        /// Token id.
        token_id: String,
    },
    /// Give a service token a new secret.
    RotateToken {
        /// The hostname.
        hostname: String,
        /// Token id.
        token_id: String,
    },
}

fn hostname(input: &str) -> Result<Hostname, InputError> {
    Hostname::parse(input).map_err(|e| InputError {
        field: "hostname",
        message: e.text(),
    })
}

impl ProtectionChange {
    /// The hostname it's about.
    pub fn hostname(&self) -> &str {
        match self {
            Self::Protect { hostname, .. }
            | Self::CreateToken { hostname, .. }
            | Self::RevokeToken { hostname, .. }
            | Self::RotateToken { hostname, .. } => hostname,
        }
    }

    /// Validates the change into an intent.
    ///
    /// # Errors
    /// The first invalid field (`hostname`, `protection`).
    pub fn to_intent(&self) -> Result<Intent, InputError> {
        let host = hostname(self.hostname())?;
        Ok(match self {
            Self::Protect { protection, .. } => Intent::ProtectHostname {
                hostname: host,
                protection: protection.normalized().map_err(|e| InputError {
                    field: "protection",
                    message: e.text(),
                })?,
            },
            Self::CreateToken { label, .. } => Intent::CreateServiceToken {
                hostname: host,
                label: label.clone(),
            },
            Self::RevokeToken { token_id, .. } => Intent::RevokeServiceToken {
                hostname: host,
                token_id: token_id.clone(),
            },
            Self::RotateToken { token_id, .. } => Intent::RotateServiceToken {
                hostname: host,
                token_id: token_id.clone(),
            },
        })
    }
}

/// How much of a quota a zone uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct QuotaView {
    /// Which quota.
    pub quota: QuotaKind,
    /// Rules now (Teitunnel's and others').
    pub used: u32,
    /// What the plan allows.
    pub limit: u32,
}

/// A hostname's edge protection as it is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProtectionView {
    /// The hostname.
    pub hostname: String,
    /// Its zone.
    pub zone: String,
    /// The zone's plan.
    pub plan: ZonePlan,
    /// What Teitunnel enforces for it now.
    pub protection: EdgeProtection,
    /// The zone's quotas.
    pub quotas: Vec<QuotaView>,
    /// Whether the plan's rate limits can match one hostname (Pro and up).
    pub rate_limit_available: bool,
    /// The longest rate limit period the plan allows, in seconds.
    pub longest_period: u32,
    /// Other hostnames sharing its rate limit.
    pub shares_rate_limit_with: Vec<String>,
    /// Whether the credential can read and write Cache Rules (an optional permission
    /// the cache bypass needs).
    pub cache_rules: bool,
}

/// One of Teitunnel's service tokens for a hostname (never its secret).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ServiceTokenView {
    /// Token id.
    pub id: String,
    /// What it's for (the name without Teitunnel's prefix and the hostname).
    pub label: String,
    /// The `CF-Access-Client-Id` value (not a secret).
    pub client_id: String,
    /// When it stops working (RFC 3339).
    pub expires_at: Option<String>,
    /// Deleted in the dashboard: only Teitunnel's note of it is left.
    pub gone: bool,
}

/// What Teitunnel enforces for `hostname` now, with its zone's quotas.
///
/// # Errors
/// Invalid hostname, observation errors (a missing permission is
/// `ObserveError::EdgePermission`), or a hostname in no zone of the account.
pub async fn view<C: CloudApi>(
    engine: &Engine,
    api: &C,
    ctx: Context<'_>,
    hostname: &str,
) -> Result<ProtectionView, EngineError> {
    let host = self::hostname(hostname)?;
    let intent = Intent::ProtectHostname {
        hostname: host.clone(),
        protection: EdgeProtection::default(),
    };
    let snapshot = engine.observation(api, ctx, &intent).await?;
    let state = snapshot
        .edge
        .into_iter()
        .next()
        .ok_or_else(|| crate::engine::PlanError::NoZone(host.to_string()))?;
    let protection = state.protection_of(&host);
    let shares_rate_limit_with = state
        .rate_limits()
        .into_iter()
        .find(|(_, _, _, hosts)| hosts.contains(host.as_str()))
        .map(|(_, _, _, hosts)| hosts.into_iter().filter(|h| h != host.as_str()).collect())
        .unwrap_or_default();
    let limits = state.plan.limits();
    Ok(ProtectionView {
        hostname: host.to_string(),
        zone: state.zone.clone(),
        plan: state.plan,
        quotas: QuotaKind::ALL
            .into_iter()
            .filter(|quota| *quota != QuotaKind::Cache || state.cache_readable)
            .map(|quota| QuotaView {
                quota,
                used: state.used(quota),
                limit: quota.limit(state.plan),
            })
            .collect(),
        rate_limit_available: limits.host_rate_limit,
        longest_period: limits.longest_period,
        shares_rate_limit_with,
        cache_rules: state.cache_readable,
        protection,
    })
}

/// Teitunnel's service tokens for `hostname`, with their expiry as Cloudflare has it.
///
/// # Errors
/// Invalid hostname, API errors (`ObserveError::ServiceTokenPermission` without the
/// permission) or database errors.
pub async fn tokens<C: CloudApi>(
    engine: &Engine,
    api: &C,
    account: &str,
    hostname: &str,
) -> Result<Vec<ServiceTokenView>, EngineError> {
    let host = self::hostname(hostname)?;
    let ours = engine
        .local()
        .owned_service_tokens(account)
        .await
        .map_err(ObserveError::from)?;
    let live = match api.service_tokens(account).await {
        Ok(live) => live,
        Err(err) if err.is_auth() => return Err(ObserveError::ServiceTokenPermission.into()),
        Err(err) => return Err(ObserveError::from(err).into()),
    };
    let prefix = format!("{}{} · ", cf_api::TEITUNNEL_PREFIX, host);
    Ok(ours
        .into_iter()
        .filter(|row| row.hostname.eq_ignore_ascii_case(host.as_str()))
        .map(|row| {
            let found = live.iter().find(|t| t.id == row.token_id);
            ServiceTokenView {
                label: row
                    .name
                    .strip_prefix(&prefix)
                    .unwrap_or(&row.name)
                    .to_owned(),
                client_id: row.client_id,
                expires_at: found.and_then(|t| t.expires_at.clone()).or(row.expires_at),
                gone: found.is_none(),
                id: row.token_id,
            }
        })
        .collect())
}

/// Plans a change for review. Nothing is changed.
///
/// # Errors
/// Invalid input, observation or planning errors.
pub async fn preview<C: CloudApi>(
    engine: &Engine,
    api: &C,
    ctx: Context<'_>,
    change: &ProtectionChange,
) -> Result<PlanView, EngineError> {
    let intent = change.to_intent()?;
    Ok(engine.preview(api, ctx, &intent).await?.view(ctx.account))
}

/// Applies a reviewed change. Credentials of tokens it created or rotated are returned
/// once (never stored).
///
/// # Errors
/// Errors before anything changed; failures while applying are in the [`Outcome`].
pub async fn apply<C, K, P>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    change: &ProtectionChange,
    approval: Approval<'_>,
    progress: P,
) -> Result<(Outcome, Vec<IssuedToken>), EngineError>
where
    C: CloudApi,
    K: Connectors,
    P: FnMut(Progress) + Send,
{
    let intent = change.to_intent()?;
    engine
        .apply_issuing(api, connectors, ctx, &intent, approval, progress)
        .await
}

/// A token's credentials as the app shows them: everything but the secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct IssuedTokenView {
    /// Token id (to copy its secret while it's kept).
    pub token_id: String,
    /// Its name.
    pub name: String,
    /// The `CF-Access-Client-Id` value.
    pub client_id: String,
    /// When it stops working (RFC 3339).
    pub expires_at: Option<String>,
}

impl From<&IssuedToken> for IssuedTokenView {
    fn from(token: &IssuedToken) -> Self {
        Self {
            token_id: token.token_id.clone(),
            name: token.name.clone(),
            client_id: token.client_id.clone(),
            expires_at: token.expires_at.clone(),
        }
    }
}

/// What to copy of a kept secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum SecretCopy {
    /// The `CF-Access-Client-Secret` value alone.
    Secret,
    /// Both headers, as lines to paste into a request or a CI secret.
    Headers,
}

/// How long a new secret stays available to copy.
pub const SECRET_KEPT_FOR: Duration = Duration::from_secs(10 * 60);

/// New and rotated secrets, kept in memory for [`SECRET_KEPT_FOR`] so the person can
/// copy them; never written anywhere.
#[derive(Debug, Default)]
pub struct IssuedSecrets {
    kept: Mutex<HashMap<String, (Instant, IssuedToken)>>,
}

impl IssuedSecrets {
    /// Keeps `tokens`, returning what the app may show of them.
    pub fn keep(&self, tokens: Vec<IssuedToken>) -> Vec<IssuedTokenView> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|_, (at, _)| at.elapsed() < SECRET_KEPT_FOR);
        tokens
            .into_iter()
            .map(|token| {
                let view = IssuedTokenView::from(&token);
                kept.insert(token.token_id.clone(), (Instant::now(), token));
                view
            })
            .collect()
    }

    /// The text to copy for a kept token (`None` once it expired or was forgotten).
    pub fn copy_text(&self, token_id: &str, what: SecretCopy) -> Option<Secret<String>> {
        let mut kept = self.kept.lock().unwrap_or_else(PoisonError::into_inner);
        kept.retain(|_, (at, _)| at.elapsed() < SECRET_KEPT_FOR);
        let (_, token) = kept.get(token_id)?;
        Some(Secret::new(match what {
            SecretCopy::Secret => token.client_secret.expose().clone(),
            SecretCopy::Headers => headers(token),
        }))
    }

    /// Drops a kept secret (the sheet was closed).
    pub fn forget(&self, token_id: &str) {
        self.kept
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(token_id);
    }
}

/// Both headers a machine sends, one per line.
pub fn headers(token: &IssuedToken) -> String {
    format!(
        "CF-Access-Client-Id: {}\nCF-Access-Client-Secret: {}",
        token.client_id,
        token.client_secret.expose()
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::edge::{BotMode, EdgeHeaderOp, HeaderRule};

    fn issued(id: &str) -> IssuedToken {
        IssuedToken {
            token_id: id.into(),
            name: "Teitunnel · api.xyz.com · CI".into(),
            client_id: format!("{id}.access"),
            client_secret: Secret::new("s3cret".into()),
            expires_at: None,
        }
    }

    #[test]
    fn keeps_secrets_only_in_memory_and_briefly() {
        let vault = IssuedSecrets::default();
        let views = vault.keep(vec![issued("t1")]);
        assert_eq!(views[0].client_id, "t1.access");
        assert!(!serde_json::to_string(&views).unwrap().contains("s3cret"));
        assert_eq!(
            vault.copy_text("t1", SecretCopy::Secret).unwrap().expose(),
            "s3cret"
        );
        assert_eq!(
            vault.copy_text("t1", SecretCopy::Headers).unwrap().expose(),
            "CF-Access-Client-Id: t1.access\nCF-Access-Client-Secret: s3cret"
        );
        vault.forget("t1");
        assert!(vault.copy_text("t1", SecretCopy::Secret).is_none());
    }

    #[test]
    fn changes_are_validated_into_intents() {
        let bad = ProtectionChange::Protect {
            hostname: "app.xyz.com".into(),
            protection: EdgeProtection {
                request_headers: vec![HeaderRule {
                    name: "CF-Ray".into(),
                    op: EdgeHeaderOp::Set,
                    value: Some("x".into()),
                }],
                ..EdgeProtection::default()
            },
        };
        assert_eq!(bad.to_intent().unwrap_err().field, "protection");
        let good = ProtectionChange::Protect {
            hostname: "App.XYZ.com".into(),
            protection: EdgeProtection {
                bots: BotMode::Block,
                ..EdgeProtection::default()
            },
        };
        assert!(matches!(
            good.to_intent().unwrap(),
            Intent::ProtectHostname { hostname, .. } if hostname.as_str() == "app.xyz.com"
        ));
        let nameless = ProtectionChange::CreateToken {
            hostname: "not a host".into(),
            label: "CI".into(),
        };
        assert_eq!(nameless.to_intent().unwrap_err().field, "hostname");
    }
}
