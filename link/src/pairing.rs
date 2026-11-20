//! The server-side pairing session and confirmation code.

use std::fmt;

use subtle::ConstantTimeEq;
use zeroize::{Zeroize, Zeroizing};

use crate::limits::PAIRING_MAX_ATTEMPTS;
use crate::ticket::PairTicket;
use crate::{EndpointId, PairingError};

/// A live pairing session on the computer.
///
/// A session accepts exactly one correct secret before its expiry time. Wrong
/// secrets are counted, and the session is cancelled after the attempt limit.
/// The secret is cleared from memory when the session ends or is dropped.
pub struct PairingSession {
    secret: Zeroizing<[u8; 32]>,
    expires_at: u64,
    attempts: u8,
    used: bool,
    cancelled: bool,
}

impl PairingSession {
    /// Creates a session with a secret and a Unix expiry time.
    pub fn new(secret: [u8; 32], expires_at: u64) -> Self {
        Self {
            secret: Zeroizing::new(secret),
            expires_at,
            attempts: 0,
            used: false,
            cancelled: false,
        }
    }

    /// Creates a session from a pairing ticket.
    pub fn from_ticket(ticket: &PairTicket) -> Self {
        Self::new(*ticket.secret(), ticket.expires_at())
    }

    /// Returns the Unix expiry time.
    pub fn expires_at(&self) -> u64 {
        self.expires_at
    }

    /// Returns the number of wrong secrets presented so far.
    pub fn attempts(&self) -> u8 {
        self.attempts
    }

    /// Returns true when the session has reached its expiry time.
    pub fn is_expired(&self, now: u64) -> bool {
        now >= self.expires_at
    }

    /// Returns true when a secret was accepted.
    pub fn is_used(&self) -> bool {
        self.used
    }

    /// Returns true when the session can no longer be used.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Cancels the session without accepting a secret.
    pub fn cancel(&mut self) {
        self.cancelled = true;
        self.secret.zeroize();
    }

    /// Checks one presented secret.
    ///
    /// The check is constant time. A correct secret marks the session used, so
    /// a replay returns [`PairingError::Used`]. A wrong secret is counted, and
    /// the session is cancelled after the attempt limit.
    ///
    /// # Errors
    ///
    /// Rejects a cancelled, used or expired session and a wrong secret.
    pub fn verify(&mut self, presented: &[u8; 32], now: u64) -> Result<(), PairingError> {
        if self.cancelled {
            return Err(PairingError::Cancelled);
        }
        if self.used {
            return Err(PairingError::Used);
        }
        if self.is_expired(now) {
            self.cancelled = true;
            self.secret.zeroize();
            return Err(PairingError::Expired);
        }
        let expected: &[u8] = &self.secret[..];
        if bool::from(expected.ct_eq(&presented[..])) {
            self.used = true;
            self.secret.zeroize();
            return Ok(());
        }
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts >= PAIRING_MAX_ATTEMPTS {
            self.cancelled = true;
            self.secret.zeroize();
            return Err(PairingError::TooManyAttempts);
        }
        Err(PairingError::SecretMismatch)
    }
}

impl fmt::Debug for PairingSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingSession")
            .field("secret", &"<redacted>")
            .field("expires_at", &self.expires_at)
            .field("attempts", &self.attempts)
            .field("used", &self.used)
            .field("cancelled", &self.cancelled)
            .finish()
    }
}

/// Computes the six-digit code that both devices show during pairing.
///
/// The code is the first four bytes of a keyed BLAKE3 hash of the phone id
/// followed by the desktop id. The hash is reduced modulo one million and
/// padded to six digits.
pub fn pairing_code(secret: &[u8; 32], phone_id: &EndpointId, desktop_id: &EndpointId) -> String {
    let mut input = [0u8; 64];
    input[..32].copy_from_slice(phone_id.as_bytes());
    input[32..].copy_from_slice(desktop_id.as_bytes());
    let hash = blake3::keyed_hash(secret, &input);
    let value = u32::from_be_bytes(
        hash.as_bytes()[..4]
            .try_into()
            .expect("the hash has at least four bytes"),
    );
    format!("{:06}", value % 1_000_000)
}

#[cfg(test)]
mod tests {
    use iroh::SecretKey;

    use super::*;

    fn id(byte: u8) -> EndpointId {
        SecretKey::from_bytes(&[byte; 32]).public()
    }

    #[test]
    fn correct_secret_is_accepted_once() {
        let mut session = PairingSession::new([3; 32], 120);
        assert_eq!(session.verify(&[3; 32], 10), Ok(()));
        assert!(session.is_used());
        assert_eq!(session.verify(&[3; 32], 11), Err(PairingError::Used));
    }

    #[test]
    fn expired_ticket_is_rejected_and_cancelled() {
        let mut session = PairingSession::new([4; 32], 120);
        assert!(!session.is_expired(119));
        assert_eq!(session.verify(&[4; 32], 120), Err(PairingError::Expired));
        assert!(session.is_cancelled());
        assert!(!session.is_used());
        assert_eq!(session.verify(&[4; 32], 121), Err(PairingError::Cancelled));
    }

    #[test]
    fn wrong_secret_is_counted_and_the_session_cancels() {
        let mut session = PairingSession::new([5; 32], 120);

        for attempt in 1..PAIRING_MAX_ATTEMPTS {
            assert_eq!(
                session.verify(&[0; 32], 1),
                Err(PairingError::SecretMismatch)
            );
            assert_eq!(session.attempts(), attempt);
            assert!(!session.is_cancelled());
        }

        assert_eq!(
            session.verify(&[0; 32], 1),
            Err(PairingError::TooManyAttempts)
        );
        assert_eq!(session.attempts(), PAIRING_MAX_ATTEMPTS);
        assert!(session.is_cancelled());
        assert_eq!(session.verify(&[5; 32], 1), Err(PairingError::Cancelled));
    }

    #[test]
    fn cancel_rejects_even_the_correct_secret() {
        let mut session = PairingSession::new([6; 32], 120);
        session.cancel();
        assert_eq!(session.verify(&[6; 32], 1), Err(PairingError::Cancelled));
    }

    #[test]
    fn session_from_ticket_uses_the_ticket_secret_and_expiry() {
        let addr = crate::EndpointAddr::new(id(1));
        let ticket = PairTicket::new(addr, [7; 32], "Studio computer", 500).expect("ticket");
        let mut session = PairingSession::from_ticket(&ticket);
        assert_eq!(session.expires_at(), 500);
        assert_eq!(session.verify(&[7; 32], 499), Ok(()));
    }

    #[test]
    fn pairing_code_is_six_digits_and_deterministic() {
        let phone = id(2);
        let desktop = id(3);
        let code = pairing_code(&[8; 32], &phone, &desktop);
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(code, pairing_code(&[8; 32], &phone, &desktop));
    }
}
