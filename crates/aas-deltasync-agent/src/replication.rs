//! Replication layer for delta dissemination.

use aas_deltasync_proto::{DocDelta, TopicScheme};
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use std::time::Duration;

/// Replication manager for delta dissemination.
pub struct ReplicationManager {
    client: AsyncClient,
    topic_scheme: TopicScheme,
}

impl ReplicationManager {
    /// Create a new replication manager.
    ///
    /// # Errors
    ///
    /// Returns error if MQTT connection fails.
    pub fn new(
        mqtt_broker: &str,
        client_id: &str,
        topic_scheme: TopicScheme,
    ) -> Result<(Self, EventLoop), ReplicationError> {
        let (host, port) = parse_mqtt_url(mqtt_broker)?;

        let mut mqtt_options = MqttOptions::new(client_id, host, port);
        mqtt_options.set_keep_alive(Duration::from_secs(30));

        let (client, eventloop) = AsyncClient::new(mqtt_options, 100);

        Ok((
            Self {
                client,
                topic_scheme,
            },
            eventloop,
        ))
    }

    /// Subscribe to delta topics for a document.
    ///
    /// # Errors
    ///
    /// Returns error if subscription fails.
    pub async fn subscribe(&self, doc_hash: &str) -> Result<(), ReplicationError> {
        let topic = self.topic_scheme.doc_wildcard(doc_hash);

        tracing::info!(topic, "Subscribing to replication topic");

        self.client
            .subscribe(&topic, QoS::AtLeastOnce)
            .await
            .map_err(|e| ReplicationError::Subscribe(e.to_string()))?;

        Ok(())
    }

    /// Publish a delta.
    ///
    /// # Errors
    ///
    /// Returns error if publish fails.
    #[allow(dead_code)]
    pub async fn publish_delta(
        &self,
        doc_hash: &str,
        delta: &DocDelta,
    ) -> Result<(), ReplicationError> {
        let topic = self.topic_scheme.delta(doc_hash);
        let payload = delta
            .to_cbor()
            .map_err(|e| ReplicationError::Serialize(e.to_string()))?;

        tracing::debug!(topic, payload_len = payload.len(), "Publishing delta");

        self.client
            .publish(&topic, QoS::AtLeastOnce, false, payload)
            .await
            .map_err(|e| ReplicationError::Publish(e.to_string()))?;

        Ok(())
    }
}

/// Parse MQTT URL into host and port.
fn parse_mqtt_url(url: &str) -> Result<(String, u16), ReplicationError> {
    let url = url
        .strip_prefix("tcp://")
        .or_else(|| url.strip_prefix("mqtt://"))
        .unwrap_or(url);

    let parts: Vec<&str> = url.split(':').collect();

    let host = parts.first().unwrap_or(&"localhost").to_string();
    let port = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(1883);

    Ok((host, port))
}

/// Errors for replication operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReplicationError {
    /// Subscription failed
    #[error("subscription error: {0}")]
    Subscribe(String),
    /// Publish failed
    #[error("publish error: {0}")]
    #[allow(dead_code)]
    Publish(String),
    /// Serialization failed
    #[error("serialize error: {0}")]
    #[allow(dead_code)]
    Serialize(String),
}
