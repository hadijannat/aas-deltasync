//! Replication layer for delta dissemination.

use aas_deltasync_proto::{DocDelta, TopicScheme};
use rumqttc::{AsyncClient, EventLoop, MqttOptions, QoS, Transport};
use std::fs;
use std::path::Path;
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
        mqtt_ca_path: Option<&Path>,
        client_id: &str,
        topic_scheme: TopicScheme,
    ) -> Result<(Self, EventLoop), ReplicationError> {
        let endpoint = parse_mqtt_url(mqtt_broker)?;

        let mut mqtt_options = MqttOptions::new(client_id, endpoint.host, endpoint.port);
        mqtt_options.set_keep_alive(Duration::from_secs(30));
        configure_tls(&mut mqtt_options, endpoint.tls, mqtt_ca_path)?;

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

#[derive(Clone, Copy, Debug)]
struct SchemeDefaults {
    port: u16,
    tls: bool,
}

#[derive(Debug)]
struct MqttEndpoint {
    host: String,
    port: u16,
    tls: bool,
}

/// Parse MQTT URL into host, port, and TLS flag.
fn parse_mqtt_url(input: &str) -> Result<MqttEndpoint, ReplicationError> {
    if input.contains("://") {
        let url = Url::parse(input)
            .map_err(|e| ReplicationError::InvalidBrokerUrl(format!("{input}: {e}")))?;

        let defaults = match url.scheme() {
            "tcp" | "mqtt" => SchemeDefaults {
                port: 1883,
                tls: false,
            },
            "ssl" | "mqtts" => SchemeDefaults {
                port: 8883,
                tls: true,
            },
            scheme => {
                return Err(ReplicationError::InvalidBrokerUrl(format!(
                    "{input}: unsupported scheme '{scheme}'"
                )));
            }
        };

        let host = url
            .host_str()
            .ok_or_else(|| ReplicationError::InvalidBrokerUrl(format!("{input}: missing host")))?;
        let port = url.port().unwrap_or(defaults.port);

        return Ok(MqttEndpoint {
            host: host.to_string(),
            port,
            tls: defaults.tls,
        });
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

    Ok(MqttEndpoint {
        host: host.to_string(),
        port,
        tls: false,
    })
}

fn configure_tls(
    mqtt_options: &mut MqttOptions,
    use_tls: bool,
    ca_path: Option<&Path>,
) -> Result<(), ReplicationError> {
    if !use_tls {
        return Ok(());
    }

    let transport = if let Some(path) = ca_path {
        let ca = fs::read(path).map_err(|err| {
            ReplicationError::Tls(format!("failed to read CA file {}: {err}", path.display()))
        })?;
        Transport::tls(ca, None, None)
    } else {
        Transport::tls_with_default_config()
    };

    mqtt_options.set_transport(transport);
    Ok(())
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
    /// TLS configuration error
    #[error("TLS configuration error: {0}")]
    Tls(String),
    /// Serialization failed
    #[error("serialize error: {0}")]
    #[allow(dead_code)]
    Serialize(String),
}
