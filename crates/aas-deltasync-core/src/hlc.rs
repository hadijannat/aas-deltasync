//! Hybrid Logical Clock (HLC) implementation for distributed timestamps.
//!
//! HLC provides globally ordered timestamps that combine:
//! - Physical wall-clock time (milliseconds)
//! - Logical counter for events at the same physical time
//! - Actor ID for deterministic tiebreaking
//!
//! # References
//!
//! Kulkarni, Demirbas, et al. "Logical Physical Clocks and Consistent Snapshots
//! in Globally Distributed Databases" (2014)

use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

/// A globally unique timestamp combining physical time, logical counter, and actor ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Timestamp {
    /// Physical wall-clock time in milliseconds since UNIX epoch
    pub physical_ms: u64,
    /// Logical counter for events at the same physical time
    pub logical: u32,
    /// Actor ID for deterministic tiebreaking
    pub actor_id: Uuid,
}

impl Timestamp {
    /// Create a new timestamp with the current wall clock time.
    #[must_use]
    pub fn now(actor_id: Uuid) -> Self {
        Self {
            physical_ms: current_time_ms(),
            logical: 0,
            actor_id,
        }
    }

    /// Serialize to bytes for wire transmission.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(28);
        bytes.extend_from_slice(&self.physical_ms.to_be_bytes());
        bytes.extend_from_slice(&self.logical.to_be_bytes());
        bytes.extend_from_slice(self.actor_id.as_bytes());
        bytes
    }

    /// Deserialize from bytes.
    ///
    /// # Errors
    ///
    /// Returns error if bytes are insufficient.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TimestampError> {
        if bytes.len() < 28 {
            return Err(TimestampError::InsufficientBytes {
                expected: 28,
                actual: bytes.len(),
            });
        }

        let physical_ms = u64::from_be_bytes(bytes[0..8].try_into().map_err(|_| {
            TimestampError::InsufficientBytes {
                expected: 28,
                actual: bytes.len(),
            }
        })?);
        let logical = u32::from_be_bytes(bytes[8..12].try_into().map_err(|_| {
            TimestampError::InsufficientBytes {
                expected: 28,
                actual: bytes.len(),
            }
        })?);
        let actor_id = Uuid::from_bytes(bytes[12..28].try_into().map_err(|_| {
            TimestampError::InsufficientBytes {
                expected: 28,
                actual: bytes.len(),
            }
        })?);

        Ok(Self {
            physical_ms,
            logical,
            actor_id,
        })
    }
}

impl Ord for Timestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        // First compare physical time
        match self.physical_ms.cmp(&other.physical_ms) {
            Ordering::Equal => {}
            ord => return ord,
        }
        // Then logical counter
        match self.logical.cmp(&other.logical) {
            Ordering::Equal => {}
            ord => return ord,
        }
        // Finally actor ID for deterministic tiebreaking
        self.actor_id.cmp(&other.actor_id)
    }
}

impl PartialOrd for Timestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Hybrid Logical Clock state machine.
#[derive(Debug, Clone)]
pub struct Hlc {
    /// Current timestamp state
    last: Timestamp,
}

impl Hlc {
    /// Create a new HLC with the given actor ID.
    #[must_use]
    pub fn new(actor_id: Uuid) -> Self {
        Self {
            last: Timestamp::now(actor_id),
        }
    }

    /// Get the actor ID for this clock.
    #[must_use]
    pub fn actor_id(&self) -> Uuid {
        self.last.actor_id
    }

    /// Generate a new timestamp for a local event.
    ///
    /// Guarantees the returned timestamp is greater than any previously
    /// generated or received timestamp.
    pub fn tick(&mut self) -> Timestamp {
        let now_ms = current_time_ms();

        if now_ms > self.last.physical_ms {
            // Wall clock advanced, reset logical counter
            self.last.physical_ms = now_ms;
            self.last.logical = 0;
        } else {
            // Wall clock hasn't advanced, increment logical counter
            self.last.logical = self.last.logical.saturating_add(1);
        }

        self.last
    }

    /// Update the clock upon receiving a remote timestamp.
    ///
    /// Ensures the local clock advances past the received timestamp.
    pub fn update(&mut self, received: Timestamp) {
        let now_ms = current_time_ms();

        if now_ms > self.last.physical_ms && now_ms > received.physical_ms {
            // Wall clock is ahead of both, use it
            self.last.physical_ms = now_ms;
            self.last.logical = 0;
        } else if self.last.physical_ms == received.physical_ms {
            // Same physical time, take max logical and increment
            self.last.logical = self.last.logical.max(received.logical).saturating_add(1);
        } else if received.physical_ms > self.last.physical_ms {
            // Received is ahead, sync to it
            self.last.physical_ms = received.physical_ms;
            self.last.logical = received.logical.saturating_add(1);
        } else {
            // Local is ahead, just increment
            self.last.logical = self.last.logical.saturating_add(1);
        }
    }

    /// Get the current timestamp without advancing the clock.
    #[must_use]
    pub fn current(&self) -> Timestamp {
        self.last
    }
}

/// Errors that can occur with timestamp operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum TimestampError {
    /// Insufficient bytes for deserialization
    #[error("insufficient bytes: expected {expected}, got {actual}")]
    InsufficientBytes {
        /// Expected byte count
        expected: usize,
        /// Actual byte count
        actual: usize,
    },
}

/// Get current wall clock time in milliseconds since UNIX epoch.
fn current_time_ms() -> u64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before UNIX epoch")
        .as_millis();
    u64::try_from(millis).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hlc_monotonic() {
        let actor = Uuid::new_v4();
        let mut hlc = Hlc::new(actor);

        let t1 = hlc.tick();
        let t2 = hlc.tick();
        let t3 = hlc.tick();

        assert!(t1 < t2);
        assert!(t2 < t3);
    }

    #[test]
    fn hlc_update_advances() {
        let actor_a = Uuid::new_v4();
        let actor_b = Uuid::new_v4();

        let mut hlc_a = Hlc::new(actor_a);
        let mut hlc_b = Hlc::new(actor_b);

        // A generates a timestamp
        let t_a = hlc_a.tick();

        // B receives it and updates
        hlc_b.update(t_a);

        // B's next timestamp should be ahead of A's
        let t_b = hlc_b.tick();
        assert!(t_b > t_a);
    }

    #[test]
    fn timestamp_serialization_roundtrip() {
        let ts = Timestamp {
            physical_ms: 1_704_067_200_000,
            logical: 42,
            actor_id: Uuid::new_v4(),
        };

        let bytes = ts.to_bytes();
        let decoded = Timestamp::from_bytes(&bytes).unwrap();

        assert_eq!(ts, decoded);
    }

    #[test]
    fn timestamp_ordering_tiebreaker() {
        let actor_a = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
        let actor_b = Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap();

        let t1 = Timestamp {
            physical_ms: 1000,
            logical: 0,
            actor_id: actor_a,
        };

        let t2 = Timestamp {
            physical_ms: 1000,
            logical: 0,
            actor_id: actor_b,
        };

        // Same time and counter, so actor_id breaks tie
        assert!(t1 < t2);
    }
}
