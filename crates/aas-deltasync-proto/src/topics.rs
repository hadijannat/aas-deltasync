//! MQTT topic scheme for delta replication.
//!
//! Topic structure: `aas-deltasync/v1/{tenant}/{doc_hash}/{message_type}`
//!
//! This allows:
//! - Tenant isolation
//! - Topic sharding by document hash
//! - Message-type filtering

use serde::{Deserialize, Serialize};

/// Protocol version for topic scheme.
pub const PROTOCOL_VERSION: &str = "v1";

/// Topic scheme configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopicScheme {
    /// Tenant identifier
    pub tenant: String,
    /// Topic prefix (default: "aas-deltasync")
    pub prefix: String,
}

impl Default for TopicScheme {
    fn default() -> Self {
        Self {
            tenant: "default".to_string(),
            prefix: "aas-deltasync".to_string(),
        }
    }
}

impl TopicScheme {
    /// Create a new topic scheme with the given tenant.
    #[must_use]
    pub fn new(tenant: impl Into<String>) -> Self {
        Self {
            tenant: tenant.into(),
            prefix: "aas-deltasync".to_string(),
        }
    }

    /// Build the base topic path.
    fn base(&self, doc_hash: &str) -> String {
        format!(
            "{}/{}/{}/{}",
            self.prefix, PROTOCOL_VERSION, self.tenant, doc_hash
        )
    }

    /// Topic for agent hello messages.
    #[must_use]
    pub fn hello(&self, doc_hash: &str) -> String {
        format!("{}/hello", self.base(doc_hash))
    }

    /// Topic for delta messages.
    #[must_use]
    pub fn delta(&self, doc_hash: &str) -> String {
        format!("{}/delta", self.base(doc_hash))
    }

    /// Topic for anti-entropy requests.
    #[must_use]
    pub fn ae_request(&self, doc_hash: &str) -> String {
        format!("{}/ae/request", self.base(doc_hash))
    }

    /// Topic for anti-entropy responses.
    #[must_use]
    pub fn ae_response(&self, doc_hash: &str) -> String {
        format!("{}/ae/response", self.base(doc_hash))
    }

    /// Wildcard subscription for all messages of a document.
    #[must_use]
    pub fn doc_wildcard(&self, doc_hash: &str) -> String {
        format!("{}/#", self.base(doc_hash))
    }

    /// Wildcard subscription for all messages in the tenant.
    #[must_use]
    pub fn tenant_wildcard(&self) -> String {
        format!("{}/{}/{}/#", self.prefix, PROTOCOL_VERSION, self.tenant)
    }

    /// Parse a topic to extract components.
    ///
    /// Returns `(doc_hash, message_type)` if valid.
    #[must_use]
    pub fn parse(&self, topic: &str) -> Option<(String, MessageType)> {
        let expected_prefix = format!("{}/{}/{}/", self.prefix, PROTOCOL_VERSION, self.tenant);
        if !topic.starts_with(&expected_prefix) {
            return None;
        }

        let remainder = &topic[expected_prefix.len()..];
        let parts: Vec<&str> = remainder.split('/').collect();

        if parts.len() < 2 {
            return None;
        }

        let doc_hash = parts[0].to_string();
        let msg_type = match parts[1..].join("/").as_str() {
            "hello" => MessageType::Hello,
            "delta" => MessageType::Delta,
            "ae/request" => MessageType::AntiEntropyRequest,
            "ae/response" => MessageType::AntiEntropyResponse,
            _ => return None,
        };

        Some((doc_hash, msg_type))
    }
}

/// Message types in the protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageType {
    /// Agent hello
    Hello,
    /// Delta replication
    Delta,
    /// Anti-entropy request
    AntiEntropyRequest,
    /// Anti-entropy response
    AntiEntropyResponse,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topic_generation() {
        let scheme = TopicScheme::new("factory-a");
        let doc_hash = "abc123def456";

        assert_eq!(
            scheme.hello(doc_hash),
            "aas-deltasync/v1/factory-a/abc123def456/hello"
        );
        assert_eq!(
            scheme.delta(doc_hash),
            "aas-deltasync/v1/factory-a/abc123def456/delta"
        );
        assert_eq!(
            scheme.ae_request(doc_hash),
            "aas-deltasync/v1/factory-a/abc123def456/ae/request"
        );
    }

    #[test]
    fn topic_parsing() {
        let scheme = TopicScheme::new("factory-a");

        let topic = "aas-deltasync/v1/factory-a/abc123/delta";
        let (doc_hash, msg_type) = scheme.parse(topic).unwrap();

        assert_eq!(doc_hash, "abc123");
        assert_eq!(msg_type, MessageType::Delta);
    }

    #[test]
    fn topic_parsing_ae() {
        let scheme = TopicScheme::new("site-b");

        let topic = "aas-deltasync/v1/site-b/xyz789/ae/request";
        let (doc_hash, msg_type) = scheme.parse(topic).unwrap();

        assert_eq!(doc_hash, "xyz789");
        assert_eq!(msg_type, MessageType::AntiEntropyRequest);
    }

    #[test]
    fn wildcard_topics() {
        let scheme = TopicScheme::new("tenant1");

        assert_eq!(
            scheme.doc_wildcard("doc1"),
            "aas-deltasync/v1/tenant1/doc1/#"
        );
        assert_eq!(scheme.tenant_wildcard(), "aas-deltasync/v1/tenant1/#");
    }
}
