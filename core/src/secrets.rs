use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};

const SERVICE: &str = "elesdee-core";

pub trait SecretStore: Send + Sync {
    fn set_key(&self, account_id: &str, key: &str) -> Result<(), SecretError>;
    fn get_key(&self, account_id: &str) -> Result<Option<String>, SecretError>;
    fn delete_key(&self, account_id: &str) -> Result<(), SecretError>;
}

#[derive(Debug)]
pub struct SecretError(String);

impl SecretError {
    fn new(error: fmt::Arguments<'_>) -> Self {
        Self(error.to_string())
    }
}

impl fmt::Display for SecretError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SecretError {}

pub struct KeyringStore {
    service: String,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self {
            service: SERVICE.to_owned(),
        }
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn set_key(&self, account_id: &str, key: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(&self.service, account_id)
            .map_err(|error| SecretError::new(format_args!("{error}")))?;
        entry
            .set_password(key)
            .map_err(|error| SecretError::new(format_args!("{error}")))
    }

    fn get_key(&self, account_id: &str) -> Result<Option<String>, SecretError> {
        let entry = keyring::Entry::new(&self.service, account_id)
            .map_err(|error| SecretError::new(format_args!("{error}")))?;
        match entry.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(SecretError::new(format_args!("{error}"))),
        }
    }

    fn delete_key(&self, account_id: &str) -> Result<(), SecretError> {
        let entry = keyring::Entry::new(&self.service, account_id)
            .map_err(|error| SecretError::new(format_args!("{error}")))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(SecretError::new(format_args!("{error}"))),
        }
    }
}

#[doc(hidden)]
#[derive(Default)]
pub struct MemorySecretStore {
    keys: Mutex<HashMap<String, String>>,
}

impl SecretStore for MemorySecretStore {
    fn set_key(&self, account_id: &str, key: &str) -> Result<(), SecretError> {
        lock(&self.keys).insert(account_id.to_owned(), key.to_owned());
        Ok(())
    }

    fn get_key(&self, account_id: &str) -> Result<Option<String>, SecretError> {
        Ok(lock(&self.keys).get(account_id).cloned())
    }

    fn delete_key(&self, account_id: &str) -> Result<(), SecretError> {
        lock(&self.keys).remove(account_id);
        Ok(())
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips_and_deletes() {
        let store = MemorySecretStore::default();
        assert_eq!(store.get_key("account").expect("read"), None);
        store.set_key("account", "secret").expect("write");
        assert_eq!(
            store.get_key("account").expect("read"),
            Some("secret".to_owned())
        );
        store.delete_key("account").expect("delete");
        assert_eq!(store.get_key("account").expect("read"), None);
    }
}
