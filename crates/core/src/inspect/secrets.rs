//! Secrets the inspector uses, kept only in the OS keychain: webhook signing secrets
//! (per share or route and provider, to verify and re-sign captured webhooks) and bearer
//! tokens protecting an exposed service (per hostname, so an AI client's configuration
//! keeps working the next time it's shared).

use lens::webhook::Provider;

use crate::{
    Secret,
    secrets::{SecretError, Secrets, spawn_blocking},
};

fn provider_key(provider: Provider) -> &'static str {
    match provider {
        Provider::Stripe => "stripe",
        Provider::GitHub => "github",
        Provider::Slack => "slack",
        Provider::Shopify => "shopify",
        Provider::StandardWebhooks => "standard-webhooks",
        Provider::Twilio => "twilio",
        Provider::Linear => "linear",
        Provider::Discord => "discord",
    }
}

fn webhook_key(scope: &str, provider: Provider) -> String {
    format!("lens:webhook:{scope}:{}", provider_key(provider))
}

fn bearer_key(hostname: &str) -> String {
    format!("lens:bearer:{}", hostname.trim().to_ascii_lowercase())
}

/// Where a hostname's webhook secrets are kept (a route's taps and its webhook inbox
/// share them).
pub fn host_scope(hostname: &str) -> String {
    format!("host:{}", hostname.trim().to_ascii_lowercase())
}

/// Saves the signing secret for `provider` on `scope` ([`super::TapScope::secret_scope`]).
///
/// # Errors
/// The keychain refused.
pub async fn set_webhook_secret(
    secrets: &Secrets,
    scope: &str,
    provider: Provider,
    secret: Secret<String>,
) -> Result<(), SecretError> {
    let (secrets, key) = (secrets.clone(), webhook_key(scope, provider));
    spawn_blocking(move || secrets.set(&key, &secret)).await
}

/// Removes a saved signing secret.
///
/// # Errors
/// The keychain refused.
pub async fn remove_webhook_secret(
    secrets: &Secrets,
    scope: &str,
    provider: Provider,
) -> Result<(), SecretError> {
    let (secrets, key) = (secrets.clone(), webhook_key(scope, provider));
    spawn_blocking(move || secrets.delete(&key)).await
}

/// The saved signing secret, if any.
///
/// # Errors
/// The keychain refused.
pub async fn webhook_secret(
    secrets: &Secrets,
    scope: &str,
    provider: Provider,
) -> Result<Option<lens::webhook::WebhookSecret>, SecretError> {
    let (secrets, key) = (secrets.clone(), webhook_key(scope, provider));
    let secret = spawn_blocking(move || secrets.get(&key)).await?;
    Ok(secret.map(|s| lens::webhook::WebhookSecret::new(s.expose().clone())))
}

/// The saved signing secret as text (for a webhook inbox's Worker secret; never
/// returned to the UI or an agent).
///
/// # Errors
/// The keychain refused.
pub(crate) async fn webhook_secret_text(
    secrets: &Secrets,
    scope: &str,
    provider: Provider,
) -> Result<Option<Secret<String>>, SecretError> {
    let (secrets, key) = (secrets.clone(), webhook_key(scope, provider));
    spawn_blocking(move || secrets.get(&key)).await
}

/// Providers with a saved secret on `scope`.
///
/// # Errors
/// The keychain refused.
pub async fn webhook_providers(
    secrets: &Secrets,
    scope: &str,
) -> Result<Vec<Provider>, SecretError> {
    let mut found = Vec::new();
    for provider in Provider::ALL {
        if webhook_secret(secrets, scope, provider).await?.is_some() {
            found.push(provider);
        }
    }
    Ok(found)
}

/// A new random token: 32 bytes from the OS's generator, base64url, prefixed `tt_`.
///
/// # Errors
/// The OS couldn't provide randomness.
pub fn generate_token() -> Result<Secret<String>, getrandom::Error> {
    use base64::Engine as _;
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(Secret::new(format!(
        "tt_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    )))
}

/// The bearer token saved for `hostname`, if any.
///
/// # Errors
/// The keychain refused.
pub async fn bearer_token(
    secrets: &Secrets,
    hostname: &str,
) -> Result<Option<Secret<String>>, SecretError> {
    let (secrets, key) = (secrets.clone(), bearer_key(hostname));
    spawn_blocking(move || secrets.get(&key)).await
}

/// Saves the bearer token for `hostname`.
///
/// # Errors
/// The keychain refused.
pub async fn set_bearer_token(
    secrets: &Secrets,
    hostname: &str,
    token: Secret<String>,
) -> Result<(), SecretError> {
    let (secrets, key) = (secrets.clone(), bearer_key(hostname));
    spawn_blocking(move || secrets.set(&key, &token)).await
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::secrets::MemoryStore;

    #[tokio::test]
    async fn webhook_secrets_live_in_the_keychain_by_scope_and_provider() {
        let store = Arc::new(MemoryStore::default());
        let secrets: Secrets = store.clone();
        set_webhook_secret(
            &secrets,
            "host:app.xyz.com",
            Provider::Stripe,
            Secret::new("whsec_abc".into()),
        )
        .await
        .unwrap();
        assert_eq!(store.keys(), ["lens:webhook:host:app.xyz.com:stripe"]);
        assert_eq!(
            webhook_providers(&secrets, "host:app.xyz.com")
                .await
                .unwrap(),
            [Provider::Stripe]
        );
        assert!(
            webhook_providers(&secrets, "host:other.xyz.com")
                .await
                .unwrap()
                .is_empty()
        );
        let secret = webhook_secret(&secrets, "host:app.xyz.com", Provider::Stripe)
            .await
            .unwrap()
            .unwrap();
        assert!(!format!("{secret:?}").contains("whsec_abc"));
        remove_webhook_secret(&secrets, "host:app.xyz.com", Provider::Stripe)
            .await
            .unwrap();
        assert!(store.keys().is_empty());
    }

    #[tokio::test]
    async fn bearer_tokens_are_random_and_kept_per_hostname() {
        let a = generate_token().unwrap();
        let b = generate_token().unwrap();
        assert_ne!(a.expose(), b.expose());
        assert!(a.expose().starts_with("tt_") && a.expose().len() > 40);
        assert_eq!(format!("{a:?}"), "Secret([redacted])");
        let secrets: Secrets = Arc::new(MemoryStore::default());
        set_bearer_token(&secrets, "MCP.xyz.com", a.clone())
            .await
            .unwrap();
        assert_eq!(
            bearer_token(&secrets, "mcp.xyz.com").await.unwrap(),
            Some(a)
        );
    }
}
