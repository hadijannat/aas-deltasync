//! Protocol messages for delta replication.

use aas_deltasync_core::Timestamp;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Agent discovery and capability advertisement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentHello {
    /// Unique agent identifier
    pub agent_id: Uuid,
    /// Supported capabilities (AAS service profile identifiers)
    pub capabilities: Vec<String>,
    /// Clock summary for anti-entropy (serialized vector clock or digest)
    pub clock_summary: Vec<u8>,
    /// Agent version
    pub version: String,
}

impl AgentHello {
    /// Create a new hello message.
    #[must_use]
    pub fn new(agent_id: Uuid, capabilities: Vec<String>) -> Self {
        Self {
            agent_id,
            capabilities,
            clock_summary: Vec::new(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Serialize to CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails.
    pub fn to_cbor(&self) -> Result<Vec<u8>, MessageError> {
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|e| MessageError::Serialize(e.to_string()))?;
        Ok(bytes)
    }

    /// Deserialize from CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, MessageError> {
        ciborium::from_reader(bytes).map_err(|e| MessageError::Deserialize(e.to_string()))
    }
}

/// A document delta for incremental replication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocDelta {
    /// Document identifier (serialized DocId)
    pub doc_id: String,
    /// Delta identifier (HLC timestamp bytes)
    pub delta_id: Vec<u8>,
    /// CBOR-encoded delta payload
    pub delta_payload: Vec<u8>,
    /// Optional Ed25519 signature
    pub signature: Option<Vec<u8>>,
}

impl DocDelta {
    /// Create a new delta message.
    #[must_use]
    pub fn new(doc_id: String, timestamp: Timestamp, payload: Vec<u8>) -> Self {
        Self {
            doc_id,
            delta_id: timestamp.to_bytes(),
            delta_payload: payload,
            signature: None,
        }
    }

    /// Get the timestamp from delta_id.
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails.
    pub fn timestamp(&self) -> Result<Timestamp, MessageError> {
        Timestamp::from_bytes(&self.delta_id).map_err(|e| MessageError::Deserialize(e.to_string()))
    }

    /// Serialize to CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails.
    pub fn to_cbor(&self) -> Result<Vec<u8>, MessageError> {
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|e| MessageError::Serialize(e.to_string()))?;
        Ok(bytes)
    }

    /// Deserialize from CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, MessageError> {
        ciborium::from_reader(bytes).map_err(|e| MessageError::Deserialize(e.to_string()))
    }
}

/// Anti-entropy synchronization request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyRequest {
    /// Document identifier
    pub doc_id: String,
    /// Digest of local state (for comparison)
    pub have_summary: Vec<u8>,
    /// Range of deltas being requested (optional)
    pub want_range: Option<DeltaRange>,
}

/// A range of deltas identified by timestamps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaRange {
    /// Start timestamp (inclusive)
    pub from: Vec<u8>,
    /// End timestamp (exclusive, if any)
    pub to: Option<Vec<u8>>,
}

impl AntiEntropyRequest {
    /// Create a new anti-entropy request.
    #[must_use]
    pub fn new(doc_id: String, have_summary: Vec<u8>) -> Self {
        Self {
            doc_id,
            have_summary,
            want_range: None,
        }
    }

    /// Serialize to CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails.
    pub fn to_cbor(&self) -> Result<Vec<u8>, MessageError> {
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|e| MessageError::Serialize(e.to_string()))?;
        Ok(bytes)
    }

    /// Deserialize from CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, MessageError> {
        ciborium::from_reader(bytes).map_err(|e| MessageError::Deserialize(e.to_string()))
    }
}

/// Anti-entropy synchronization response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntiEntropyResponse {
    /// Document identifier
    pub doc_id: String,
    /// Deltas that the requester is missing
    pub deltas: Vec<DocDelta>,
    /// Full state snapshot (if delta set would be too large)
    pub snapshot: Option<Vec<u8>>,
}

impl AntiEntropyResponse {
    /// Create a new response with deltas.
    #[must_use]
    pub fn with_deltas(doc_id: String, deltas: Vec<DocDelta>) -> Self {
        Self {
            doc_id,
            deltas,
            snapshot: None,
        }
    }

    /// Create a new response with a full snapshot.
    #[must_use]
    pub fn with_snapshot(doc_id: String, snapshot: Vec<u8>) -> Self {
        Self {
            doc_id,
            deltas: Vec::new(),
            snapshot: Some(snapshot),
        }
    }

    /// Serialize to CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if serialization fails.
    pub fn to_cbor(&self) -> Result<Vec<u8>, MessageError> {
        let mut bytes = Vec::new();
        ciborium::into_writer(self, &mut bytes)
            .map_err(|e| MessageError::Serialize(e.to_string()))?;
        Ok(bytes)
    }

    /// Deserialize from CBOR bytes.
    ///
    /// # Errors
    ///
    /// Returns error if deserialization fails.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, MessageError> {
        ciborium::from_reader(bytes).map_err(|e| MessageError::Deserialize(e.to_string()))
    }
}

/// Errors for message serialization/deserialization.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MessageError {
    /// Serialization failed
    #[error("serialization failed: {0}")]
    Serialize(String),
    /// Deserialization failed
    #[error("deserialization failed: {0}")]
    Deserialize(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_hello_cbor_roundtrip() {
        let hello = AgentHello::new(
            Uuid::new_v4(),
            vec!["SubmodelRepositoryServiceSpecification".to_string()],
        );

        let bytes = hello.to_cbor().unwrap();
        let decoded = AgentHello::from_cbor(&bytes).unwrap();

        assert_eq!(hello.agent_id, decoded.agent_id);
        assert_eq!(hello.capabilities, decoded.capabilities);
    }

    #[test]
    fn doc_delta_cbor_roundtrip() {
        let ts = Timestamp {
            physical_ms: 1704067200000,
            logical: 42,
            actor_id: Uuid::new_v4(),
        };

        let delta = DocDelta::new("doc1".to_string(), ts, vec![1, 2, 3]);

        let bytes = delta.to_cbor().unwrap();
        let decoded = DocDelta::from_cbor(&bytes).unwrap();

        assert_eq!(delta.doc_id, decoded.doc_id);
        assert_eq!(delta.delta_payload, decoded.delta_payload);
    }
}
