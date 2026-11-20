//! The long-lived device identity.

use std::fmt;

use iroh::{EndpointId, PublicKey, SecretKey};
use zeroize::Zeroizing;

use crate::LinkError;

/// A long-lived Ed25519 identity for one endpoint.
///
/// The desktop keeps its key in a private file and the phone keeps its key in
/// app-private storage. The public key is the device identity. The secret is
/// cleared from memory when the identity is dropped.
pub struct DeviceIdentity {
    secret: SecretKey,
}

impl DeviceIdentity {
    /// Creates a new identity from the operating system random source.
    pub fn generate() -> Self {
        Self {
            secret: SecretKey::generate(),
        }
    }

    /// Creates an identity from a 32-byte secret key.
    pub fn from_bytes(bytes: &[u8; 32]) -> Self {
        Self {
            secret: SecretKey::from_bytes(bytes),
        }
    }

    /// Creates an identity from a stored key, checking the length.
    ///
    /// # Errors
    ///
    /// Returns [`LinkError::InvalidIdentityKey`] when the slice is not exactly
    /// 32 bytes.
    pub fn from_slice(bytes: &[u8]) -> Result<Self, LinkError> {
        let key: &[u8; 32] = bytes
            .try_into()
            .map_err(|_| LinkError::InvalidIdentityKey)?;
        Ok(Self::from_bytes(key))
    }

    /// Returns the secret key for endpoint construction.
    pub fn secret_key(&self) -> &SecretKey {
        &self.secret
    }

    /// Returns the public key, which is the device identity.
    pub fn public_key(&self) -> PublicKey {
        self.secret.public()
    }

    /// Returns the endpoint id, which is the public key.
    pub fn endpoint_id(&self) -> EndpointId {
        self.secret.public()
    }

    /// Returns the endpoint id as a z-base-32 string.
    ///
    /// The interface shows the first eight characters.
    pub fn endpoint_id_string(&self) -> String {
        self.secret.public().to_z32()
    }

    /// Returns the secret key bytes, cleared from memory when dropped.
    pub fn to_bytes(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.secret.to_bytes())
    }
}

impl fmt::Debug for DeviceIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DeviceIdentity")
            .field("endpoint_id", &self.endpoint_id_string())
            .field("secret", &"<redacted>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_round_trips_through_its_key_bytes() {
        let identity = DeviceIdentity::generate();
        let bytes = identity.to_bytes();
        let restored = DeviceIdentity::from_slice(&bytes[..]).expect("key is 32 bytes");
        assert_eq!(restored.public_key(), identity.public_key());
        assert_eq!(restored.endpoint_id(), identity.endpoint_id());
    }

    #[test]
    fn endpoint_id_string_is_lowercase_z_base_32() {
        let identity = DeviceIdentity::from_bytes(&[9; 32]);
        let id = identity.endpoint_id_string();
        assert_eq!(id.len(), 52);
        assert!(
            id.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
    }

    #[test]
    fn wrong_key_length_is_rejected() {
        assert!(matches!(
            DeviceIdentity::from_slice(&[0; 31]),
            Err(LinkError::InvalidIdentityKey)
        ));
        assert!(matches!(
            DeviceIdentity::from_slice(&[0; 33]),
            Err(LinkError::InvalidIdentityKey)
        ));
    }

    #[test]
    fn debug_output_redacts_the_secret() {
        let text = format!("{:?}", DeviceIdentity::from_bytes(&[0xCD; 32]));
        assert!(text.contains("<redacted>"));
        assert!(!text.contains("cdcd"));
    }
}
