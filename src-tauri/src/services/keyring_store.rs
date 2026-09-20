use keyring::Entry;
use crate::error::AppError;

const SERVICE_NAME: &str = "com.teispace.teitunnel";
const TOKEN_KEY: &str = "cloudflare_api_token";

pub struct KeyringStore;

impl KeyringStore {
    /// Securely saves the Cloudflare API token into the OS keychain
    pub fn save_token(token: &str) -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, TOKEN_KEY)
            .map_err(|e| AppError::KeyringError(e.to_string()))?;
        entry
            .set_password(token)
            .map_err(|e| AppError::KeyringError(format!("Failed to store token in keychain: {}", e)))?;
        Ok(())
    }

    /// Securely retrieves the Cloudflare API token from the OS keychain
    pub fn get_token() -> Result<Option<String>, AppError> {
        let entry = Entry::new(SERVICE_NAME, TOKEN_KEY)
            .map_err(|e| AppError::KeyringError(e.to_string()))?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(AppError::KeyringError(format!("Failed to read token from keychain: {}", e))),
        }
    }

    /// Deletes the Cloudflare API token from the OS keychain
    pub fn delete_token() -> Result<(), AppError> {
        let entry = Entry::new(SERVICE_NAME, TOKEN_KEY)
            .map_err(|e| AppError::KeyringError(e.to_string()))?;
        match entry.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(AppError::KeyringError(format!("Failed to delete token: {}", e))),
        }
    }
}
