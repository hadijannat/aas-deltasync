//! Replication layer for delta dissemination.

use aas_deltasync_proto::{DocDelta, TopicScheme};
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS};
use std::time::Duration;
use url::Url;

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
fn parse_mqtt_url(input: &str) -> Result<(String, u16), ReplicationError> {
    if input.contains("://") {
        let url = Url::parse(input)
            .map_err(|e| ReplicationError::InvalidBrokerUrl(format!("{input}: {e}")))?;

        match url.scheme() {
            "tcp" | "mqtt" => {}
            scheme => {
                return Err(ReplicationError::InvalidBrokerUrl(format!(
                    "{input}: unsupported scheme '{scheme}'"
                )));
            }
        }

        let host = url
            .host_str()
            .ok_or_else(|| ReplicationError::InvalidBrokerUrl(format!("{input}: missing host")))?;
        let port = url.port().unwrap_or(1883);

        return Ok((host.to_string(), port));
    }

    let mut parts = input.split(':');
    let host = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ReplicationError::InvalidBrokerUrl(format!("{input}: missing host")))?;
    let port = match parts.next() {
        None => 1883,
        Some(port) => port.parse().map_err(|_| {
            ReplicationError::InvalidBrokerUrl(format!("{input}: invalid port '{port}'"))
        })?,
    };
    if parts.next().is_some() {
        return Err(ReplicationError::InvalidBrokerUrl(format!(
            "{input}: too many ':' separators"
        )));
    }

    Ok((host.to_string(), port))
}

/// Errors for replication operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ReplicationError {
    /// Subscription failed
    #[error("subscription error: {0}")]
    Subscribe(String),
    /// Invalid MQTT broker URL
    #[error("invalid MQTT broker URL: {0}")]
    InvalidBrokerUrl(String),
    /// Publish failed
    #[error("publish error: {0}")]
    #[allow(dead_code)]
    Publish(String),
    /// Serialization failed
    #[error("serialize error: {0}")]
    #[allow(dead_code)]
    Serialize(String),
}
