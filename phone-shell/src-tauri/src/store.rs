//! Phone identity and computer storage.
//!
//! The device identity lives in the platform keychain. The computer list holds
//! no secrets, so it lives as JSON in app-private storage. Android backup is
//! disabled for the app, so a restored phone does not clone the identity.

use std::fs;
use std::path::{Path, PathBuf};

use elesdee_link::{ComputerRecord, PhoneStore, StoreError};

const SERVICE: &str = "elesdee-phone";
const IDENTITY_ACCOUNT: &str = "identity";
const COMPUTERS_FILE: &str = "computers.json";

/// A [`PhoneStore`] backed by the platform keychain and a JSON file.
pub struct ShellStore {
    computers: PathBuf,
}

impl ShellStore {
    /// Creates the store for one app data directory.
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            computers: data_dir.join(COMPUTERS_FILE),
        }
    }

    /// Installs the platform keychain as the default credential store.
    ///
    /// # Errors
    ///
    /// Returns a message when the platform store cannot be created.
    pub fn install_keychain() -> Result<(), String> {
        install_keychain()
    }
}

impl PhoneStore for ShellStore {
    fn load_identity(&self) -> Result<Option<[u8; 32]>, StoreError> {
        let entry = keyring_core::Entry::new(SERVICE, IDENTITY_ACCOUNT)
            .map_err(|error| StoreError::new(error.to_string()))?;
        match entry.get_secret() {
            Ok(bytes) => {
                let key: [u8; 32] = bytes
                    .as_slice()
                    .try_into()
                    .map_err(|_| StoreError::new("The stored phone key has the wrong length."))?;
                Ok(Some(key))
            }
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(error) => Err(StoreError::new(error.to_string())),
        }
    }

    fn save_identity(&self, secret: &[u8; 32]) -> Result<(), StoreError> {
        let entry = keyring_core::Entry::new(SERVICE, IDENTITY_ACCOUNT)
            .map_err(|error| StoreError::new(error.to_string()))?;
        entry
            .set_secret(secret)
            .map_err(|error| StoreError::new(error.to_string()))
    }

    fn load_computers(&self) -> Result<Vec<ComputerRecord>, StoreError> {
        match fs::read(&self.computers) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|error| {
                StoreError::new(format!("The computer list is not valid: {error}"))
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(error) => Err(StoreError::new(format!(
                "The computer list could not be read: {error}"
            ))),
        }
    }

    fn save_computers(&self, computers: &[ComputerRecord]) -> Result<(), StoreError> {
        let json = serde_json::to_vec_pretty(computers)
            .map_err(|error| StoreError::new(format!("The computer list is not valid: {error}")))?;
        if let Some(parent) = self.computers.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                StoreError::new(format!("The data folder could not be created: {error}"))
            })?;
        }
        let temporary = self.computers.with_extension("json.tmp");
        write_private(&temporary, &json)?;
        fs::rename(&temporary, &self.computers).map_err(|error| {
            StoreError::new(format!("The computer list could not be saved: {error}"))
        })
    }
}

#[cfg(unix)]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| {
            StoreError::new(format!("The computer list could not be written: {error}"))
        })?;
    file.write_all(bytes).map_err(|error| {
        StoreError::new(format!("The computer list could not be written: {error}"))
    })
}

#[cfg(not(unix))]
fn write_private(path: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    fs::write(path, bytes).map_err(|error| {
        StoreError::new(format!("The computer list could not be written: {error}"))
    })
}

#[cfg(target_os = "macos")]
fn install_keychain() -> Result<(), String> {
    let store = apple_native_keyring_store::keychain::Store::new()
        .map_err(|error| format!("The system keychain is not available: {error}"))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "ios")]
fn install_keychain() -> Result<(), String> {
    let store = apple_native_keyring_store::protected::Store::new()
        .map_err(|error| format!("The system keychain is not available: {error}"))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "android")]
fn install_keychain() -> Result<(), String> {
    let store = android_native_keyring_store::Store::new()
        .map_err(|error| format!("The system keychain is not available: {error}"))?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "ios", target_os = "android")))]
fn install_keychain() -> Result<(), String> {
    Err("This build has no supported platform keychain.".to_owned())
}
