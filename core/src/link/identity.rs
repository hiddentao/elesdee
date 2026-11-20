//! The persistent desktop device identity.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use elesdee_link::DeviceIdentity;

use crate::private_fs;

/// Name of the private key file under the app data directory.
pub const IDENTITY_FILE: &str = "link.key";

/// Returns the path of the device key under the app data directory.
pub fn identity_path(data_dir: &Path) -> PathBuf {
    data_dir.join(IDENTITY_FILE)
}

/// Loads the desktop identity, creating and saving one when it is absent.
///
/// The key file holds 32 bytes and is private to the user. A file with another
/// length is replaced. The key is never written to the state file or a log.
pub fn load_or_create_identity(data_dir: &Path) -> io::Result<DeviceIdentity> {
    private_fs::create_private_dir_all(data_dir)?;
    let path = identity_path(data_dir);
    match fs::read(&path) {
        Ok(bytes) if bytes.len() == 32 => {
            private_fs::restrict_file_permissions(&path)?;
            DeviceIdentity::from_slice(&bytes)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))
        }
        Ok(bytes) => {
            tracing::error!(
                path = %path.display(),
                len = bytes.len(),
                "the device key has the wrong length; creating a new identity"
            );
            create_identity(&path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => create_identity(&path),
        Err(error) => Err(error),
    }
}

fn create_identity(path: &Path) -> io::Result<DeviceIdentity> {
    let identity = DeviceIdentity::generate();
    let bytes = identity.to_bytes();
    private_fs::write_private_file_atomic(path, &bytes[..])?;
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "elesdee-link-identity-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&path).expect("create temporary folder");
        path
    }

    #[test]
    fn identity_is_created_once_and_reused() {
        let directory = temp_dir("reuse");
        let first = load_or_create_identity(&directory).expect("create identity");
        let second = load_or_create_identity(&directory).expect("load identity");

        assert_eq!(first.endpoint_id(), second.endpoint_id());
        let path = identity_path(&directory);
        assert_eq!(fs::read(&path).expect("read key").len(), 32);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_invalid_key_is_replaced() {
        let directory = temp_dir("replace");
        fs::write(identity_path(&directory), b"short").expect("write invalid key");
        let identity = load_or_create_identity(&directory).expect("create identity");
        assert_eq!(
            fs::read(identity_path(&directory)).expect("read key").len(),
            32
        );
        assert_eq!(identity.to_bytes().len(), 32);
        let _ = fs::remove_dir_all(&directory);
    }
}
