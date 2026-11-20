//! Bounded reconnect delays and terminal link failures.
//!
//! A dropped connection retries after 0.5 s, 1 s, 2 s, 4 s, 8 s, 15 s and then
//! 30 s, with ±20% jitter. A connection that stayed up for 30 seconds resets
//! the sequence. A revoked or unknown device does not retry.

use std::time::Duration;

use crate::ClientError;

/// Delays between reconnect attempts.
pub const RECONNECT_DELAYS: [Duration; 7] = [
    Duration::from_millis(500),
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
    Duration::from_secs(8),
    Duration::from_secs(15),
    Duration::from_secs(30),
];

/// A connection that stayed up this long resets the delay sequence.
pub const RECONNECT_RESET: Duration = Duration::from_secs(30);

/// Returns the next reconnect delay for a dropped connection.
///
/// The delay grows from half a second to thirty seconds and keeps the last
/// value. Jitter stays within ±20% of the base delay and never repeats the
/// same factor, so many phones do not retry at the same moment.
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    attempt: usize,
    seed: u64,
}

impl ReconnectPolicy {
    /// Creates a policy with a seed taken from the clock.
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self::with_seed(seed)
    }

    /// Creates a policy with an explicit seed. Tests use this for stable
    /// delays.
    pub fn with_seed(seed: u64) -> Self {
        Self { attempt: 0, seed }
    }

    /// Returns the number of delays returned since the last reset.
    pub fn attempt(&self) -> usize {
        self.attempt
    }

    /// Returns the next delay and moves to the next step.
    pub fn next_delay(&mut self) -> Duration {
        let base = RECONNECT_DELAYS[self.attempt.min(RECONNECT_DELAYS.len() - 1)];
        if self.attempt < RECONNECT_DELAYS.len() - 1 {
            self.attempt += 1;
        }
        jitter(base, self.next_fraction())
    }

    /// Resets the sequence after a stable connection.
    pub fn record_connection(&mut self, connected_for: Duration) {
        if connected_for >= RECONNECT_RESET {
            self.reset();
        }
    }

    /// Resets the sequence to the first delay.
    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    fn next_fraction(&mut self) -> f64 {
        self.seed = self
            .seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let unit = (self.seed >> 33) as f64 / f64::from(u32::MAX);
        // The range is 0.8 to 1.2, so the delay stays within ±20%.
        0.8 + unit * 0.4
    }
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self::new()
    }
}

/// Returns true when the link must stop retrying and the user must act.
pub fn is_terminal(error: &ClientError) -> bool {
    matches!(error, ClientError::Revoked | ClientError::NotPaired)
}

/// Returns the delay before the next attempt, or `None` when retrying stops.
///
/// A stable connection resets the delay sequence before the next value is
/// chosen.
pub fn next_retry(
    error: &ClientError,
    connected_for: Duration,
    policy: &mut ReconnectPolicy,
) -> Option<Duration> {
    if is_terminal(error) {
        return None;
    }
    policy.record_connection(connected_for);
    Some(policy.next_delay())
}

fn jitter(base: Duration, fraction: f64) -> Duration {
    base.mul_f64(fraction)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delays_grow_to_thirty_seconds_and_stay() {
        let mut policy = ReconnectPolicy::with_seed(7);
        let bases = [500u64, 1_000, 2_000, 4_000, 8_000, 15_000, 30_000, 30_000];
        for base in bases {
            let delay = policy.next_delay().as_millis() as u64;
            let low = base * 8 / 10;
            let high = base * 12 / 10;
            assert!(
                (low..=high).contains(&delay),
                "delay {delay} is outside {low}..={high}"
            );
        }
    }

    #[test]
    fn jitter_stays_within_twenty_percent() {
        let mut policy = ReconnectPolicy::with_seed(11);
        for _ in 0..100 {
            let delay = policy.next_delay().as_millis();
            assert!((400..=600).contains(&delay), "first delay was {delay}");
            policy.reset();
        }
    }

    #[test]
    fn a_stable_connection_resets_the_sequence() {
        let mut policy = ReconnectPolicy::with_seed(3);
        policy.next_delay();
        policy.next_delay();
        assert_eq!(policy.attempt(), 2);
        policy.record_connection(Duration::from_secs(29));
        assert_eq!(policy.attempt(), 2);
        policy.record_connection(RECONNECT_RESET);
        assert_eq!(policy.attempt(), 0);
    }

    #[test]
    fn a_revoked_device_stops_retrying() {
        let mut policy = ReconnectPolicy::with_seed(1);
        assert_eq!(
            next_retry(&ClientError::Revoked, Duration::ZERO, &mut policy),
            None
        );
        assert_eq!(
            next_retry(&ClientError::NotPaired, Duration::ZERO, &mut policy),
            None
        );
        assert!(next_retry(&ClientError::ConnectionClosed, Duration::ZERO, &mut policy).is_some());
    }
}
