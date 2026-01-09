//! AAS Document model mapped to CRDT structures.
//!
//! Maps AAS Submodel structure to an OR-Map where:
//! - Keys are canonical idShortPath strings
//! - Values are LWW registers holding JSON values

use crate::crdt::{Delta, OrMap};
use crate::hlc::Hlc;
use serde::{Deserialize, Serialize};

/// The type of view being replicated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum View {
    /// Full submodel (Normal serialization)
    Normal,
    /// Value-only view ($value modifier)
    #[default]
    Value,
    /// Metadata-only view ($metadata modifier)
    Metadata,
}

impl std::fmt::Display for View {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            View::Normal => write!(f, "normal"),
            View::Value => write!(f, "$value"),
            View::Metadata => write!(f, "$metadata"),
        }
    }
}

/// Unique identifier for a CRDT document.
///
/// Combines AAS identifier, Submodel identifier, and view type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocId {
    /// AAS identifier (e.g., "urn:example:aas:asset1")
    pub aas_id: String,
    /// Submodel identifier (e.g., "urn:example:sm:operationaldata")
    pub submodel_id: String,
    /// The serialization view being replicated
    pub view: View,
}

impl DocId {
    /// Create a new document ID.
    #[must_use]
    pub fn new(aas_id: impl Into<String>, submodel_id: impl Into<String>, view: View) -> Self {
        Self {
            aas_id: aas_id.into(),
            submodel_id: submodel_id.into(),
            view,
        }
    }

    /// Create a document ID for the $value view (most common case).
    #[must_use]
    pub fn value_view(aas_id: impl Into<String>, submodel_id: impl Into<String>) -> Self {
        Self::new(aas_id, submodel_id, View::Value)
    }

    /// Generate a hash for MQTT topic sharding.
    #[must_use]
    pub fn topic_hash(&self) -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

impl std::fmt::Display for DocId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.aas_id, self.submodel_id, self.view)
    }
}

/// A CRDT-backed AAS document.
///
/// The document state is an OR-Map where:
/// - Keys are canonical idShortPath strings (e.g., "TechnicalProperties.MaxTemperature")
/// - Values are JSON values serialized as strings
pub struct CrdtDocument {
    /// Document identifier
    pub id: DocId,
    /// CRDT state: path -> value
    pub state: OrMap<String, serde_json::Value>,
    /// Hybrid logical clock for this document
    pub clock: Hlc,
}

impl CrdtDocument {
    /// Create a new empty document.
    #[must_use]
    pub fn new(id: DocId, clock: Hlc) -> Self {
        Self {
            id,
            state: OrMap::new(),
            clock,
        }
    }

    /// Set a value at the given path.
    ///
    /// Returns a delta representing this change.
    #[must_use]
    pub fn set(
        &mut self,
        path: &str,
        value: serde_json::Value,
    ) -> Delta<String, serde_json::Value> {
        let timestamp = self.clock.tick();
        self.state
            .insert(path.to_string(), value.clone(), timestamp);

        let mut delta = Delta::new();
        delta.add_insert(path.to_string(), value, timestamp);
        tracing::debug!(
            doc_id = %self.id,
            path,
            timestamp = ?timestamp,
            "Created delta (set)"
        );
        delta
    }

    /// Get a value at the given path.
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&serde_json::Value> {
        self.state.get(&path.to_string())
    }

    /// Remove a value at the given path.
    ///
    /// Returns a delta representing this change.
    #[must_use]
    pub fn remove(&mut self, path: &str) -> Delta<String, serde_json::Value> {
        let timestamp = self.clock.tick();
        self.state.remove(&path.to_string(), timestamp);

        let mut delta = Delta::new();
        delta.add_remove(path.to_string(), timestamp);
        tracing::debug!(
            doc_id = %self.id,
            path,
            timestamp = ?timestamp,
            "Created delta (remove)"
        );
        delta
    }

    /// Apply a delta from another replica.
    pub fn apply_delta(&mut self, delta: &Delta<String, serde_json::Value>) {
        let before_len = self.state.len();
        // Update clock based on delta timestamps
        for (_, _, timestamp) in &delta.inserts {
            self.clock.update(*timestamp);
        }
        for (_, timestamp) in &delta.removes {
            self.clock.update(*timestamp);
        }

        delta.apply_to(&mut self.state);
        let after_len = self.state.len();
        tracing::debug!(
            doc_id = %self.id,
            inserts = delta.inserts.len(),
            removes = delta.removes.len(),
            before_len,
            after_len,
            "Applied delta"
        );
    }

    /// Merge with another document's state.
    pub fn merge(&mut self, other: &Self) {
        self.clock.update(other.clock.current());
        self.state.merge(&other.state);
    }

    /// Get all paths in the document.
    pub fn paths(&self) -> impl Iterator<Item = &String> {
        self.state.iter().map(|(k, _)| k)
    }

    /// Get the number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.state.len()
    }

    /// Check if the document is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn doc_id_display() {
        let id = DocId::value_view("urn:example:aas:1", "urn:example:sm:data");
        assert_eq!(
            id.to_string(),
            "urn:example:aas:1:urn:example:sm:data:$value"
        );
    }

    #[test]
    fn crdt_document_set_get() {
        let id = DocId::value_view("aas1", "sm1");
        let clock = Hlc::new(Uuid::new_v4());
        let mut doc = CrdtDocument::new(id, clock);

        let _ = doc.set("Temperature", serde_json::json!(25.5));
        let _ = doc.set("Status", serde_json::json!("Running"));

        assert_eq!(doc.get("Temperature"), Some(&serde_json::json!(25.5)));
        assert_eq!(doc.get("Status"), Some(&serde_json::json!("Running")));
        assert_eq!(doc.len(), 2);
    }

    #[test]
    fn crdt_document_merge_convergence() {
        let id = DocId::value_view("aas1", "sm1");

        let mut doc_a = CrdtDocument::new(id.clone(), Hlc::new(Uuid::new_v4()));
        let mut doc_b = CrdtDocument::new(id.clone(), Hlc::new(Uuid::new_v4()));

        // Concurrent updates to same path
        let delta_a = doc_a.set("X", serde_json::json!(10));
        let delta_b = doc_b.set("X", serde_json::json!(20));

        // Cross-apply deltas
        doc_a.apply_delta(&delta_b);
        doc_b.apply_delta(&delta_a);

        // Should converge (deterministic based on timestamp + actor)
        assert_eq!(doc_a.get("X"), doc_b.get("X"));
    }
}
