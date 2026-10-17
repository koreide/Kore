//! API key storage in the OS keychain (macOS Keychain, Windows Credential
//! Manager, Linux Secret Service). Calls are synchronous; [`crate::Core`]
//! runs them on the blocking pool.

use crate::error::{K8sError, Result};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Where API keys live. The default is the OS keychain; tests use [`MemorySecrets`].
pub trait SecretStore: Send + Sync + 'static {
    fn store(&self, provider: &str, key: &str) -> Result<()>;
    fn get(&self, provider: &str) -> Result<Option<String>>;
    fn delete(&self, provider: &str) -> Result<()>;
}

/// The OS keychain.
pub struct KeychainSecrets;

impl SecretStore for KeychainSecrets {
    fn store(&self, provider: &str, key: &str) -> Result<()> {
        store_api_key(provider, key)
    }
    fn get(&self, provider: &str) -> Result<Option<String>> {
        get_api_key(provider)
    }
    fn delete(&self, provider: &str) -> Result<()> {
        delete_api_key(provider)
    }
}

/// In-process key store (never touches the keychain).
#[derive(Default)]
pub struct MemorySecrets(Mutex<HashMap<String, String>>);

impl SecretStore for MemorySecrets {
    fn store(&self, provider: &str, key: &str) -> Result<()> {
        self.0.lock().unwrap().insert(provider.into(), key.into());
        Ok(())
    }
    fn get(&self, provider: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(provider).cloned())
    }
    fn delete(&self, provider: &str) -> Result<()> {
        self.0.lock().unwrap().remove(provider);
        Ok(())
    }
}

/// Cloneable handle stored in [`crate::CoreConfig`].
#[derive(Clone)]
pub struct Secrets(pub Arc<dyn SecretStore>);

impl Secrets {
    pub fn new(s: impl SecretStore) -> Self {
        Secrets(Arc::new(s))
    }
    pub fn memory() -> Self {
        Secrets::new(MemorySecrets::default())
    }
}

impl Default for Secrets {
    fn default() -> Self {
        Secrets::new(KeychainSecrets)
    }
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secrets")
    }
}

const SERVICE: &str = "kore";

fn entry(provider: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, provider).map_err(|e| K8sError::Other(e.to_string()))
}

pub fn store_api_key(provider: &str, key: &str) -> Result<()> {
    entry(provider)?
        .set_password(key)
        .map_err(|e| K8sError::Other(e.to_string()))
}

pub fn get_api_key(provider: &str) -> Result<Option<String>> {
    match entry(provider)?.get_password() {
        Ok(key) => Ok(Some(key)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(K8sError::Other(e.to_string())),
    }
}

pub fn delete_api_key(provider: &str) -> Result<()> {
    match entry(provider)?.delete_password() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(K8sError::Other(e.to_string())),
    }
}
